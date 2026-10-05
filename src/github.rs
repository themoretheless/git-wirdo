use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub head_ref_name: String,
    pub base_ref_name: String,
    pub head_ref_oid: String,
    pub is_draft: bool,
}

#[cfg(test)]
thread_local! {
    static CAPTURE: std::cell::RefCell<Option<Vec<Vec<String>>>> = const { std::cell::RefCell::new(None) };
}

pub fn run(root: &Path, args: &[&str]) -> Result<String> {
    #[cfg(test)]
    if CAPTURE.with(|capture| {
        if let Some(calls) = capture.borrow_mut().as_mut() {
            calls.push(args.iter().map(|arg| (*arg).to_owned()).collect());
            true
        } else {
            false
        }
    }) {
        return Ok("fixture result".into());
    }

    run_program(root, std::ffi::OsStr::new("gh"), args)
}

fn run_program(root: &Path, program: &std::ffi::OsStr, args: &[&str]) -> Result<String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_PAGER", "cat")
        .env("NO_COLOR", "1");
    for variable in [
        "GH_REPO",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_NAMESPACE",
        "GIT_PREFIX",
        "GIT_SHALLOW_FILE",
    ] {
        command.env_remove(variable);
    }
    let output = crate::process::output(&mut command)
        .context("GitHub CLI unavailable; install gh and run gh auth login outside the TUI")?;
    ensure!(
        output.status.success(),
        "GitHub command failed: {}",
        if output.stderr.is_empty() {
            String::from_utf8_lossy(&output.stdout)
        } else {
            String::from_utf8_lossy(&output.stderr)
        }
        .trim()
    );
    Ok(String::from_utf8_lossy(if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    })
    .into_owned())
}

pub fn list(root: &Path) -> Result<Vec<PullRequest>> {
    serde_json::from_str(&run(
        root,
        &[
            "pr",
            "list",
            "--state",
            "open",
            "--limit",
            "100",
            "--json",
            "number,title,url,headRefName,baseRefName,headRefOid,isDraft",
        ],
    )?)
    .context("Invalid PR response from gh")
}

pub fn detail(root: &Path, number: u64) -> Result<String> {
    let number = number.to_string();
    let json: serde_json::Value = serde_json::from_str(&run(
        root,
        &[
            "pr",
            "view",
            &number,
            "--json",
            "number,title,url,body,state,reviewDecision,mergeable,statusCheckRollup,comments,reviews,headRefName,baseRefName",
        ],
    )?)?;
    Ok(format_detail(&json))
}

fn format_detail(json: &serde_json::Value) -> String {
    let text = |key: &str| json[key].as_str().filter(|s| !s.is_empty()).unwrap_or("—");
    let mut result = format!(
        "#{} {}\n{}\n{} → {}\nState: {} | Review: {} | Merge: {}\n\n{}\n\nChecks\n",
        json["number"],
        text("title"),
        text("url"),
        text("headRefName"),
        text("baseRefName"),
        text("state"),
        text("reviewDecision"),
        text("mergeable"),
        text("body")
    );
    if let Some(checks) = json["statusCheckRollup"].as_array() {
        for check in checks {
            let name = check["name"]
                .as_str()
                .or_else(|| check["context"].as_str())
                .unwrap_or("Check");
            let state = check["conclusion"]
                .as_str()
                .filter(|s| !s.is_empty())
                .or_else(|| check["status"].as_str())
                .or_else(|| check["state"].as_str())
                .unwrap_or("pending");
            result.push_str(&format!("{name}: {state}\n"));
        }
        if checks.is_empty() {
            result.push_str("No checks reported\n");
        }
    }
    for (key, heading) in [("reviews", "Reviews"), ("comments", "Comments")] {
        result.push_str(&format!("\n{heading}\n"));
        if let Some(entries) = json[key].as_array() {
            for entry in entries {
                result.push_str(&format!(
                    "{} {}\n{}\n\n",
                    entry["author"]["login"].as_str().unwrap_or("unknown"),
                    entry["state"].as_str().unwrap_or(""),
                    entry["body"].as_str().unwrap_or("")
                ));
            }
        }
    }
    result
}

pub fn checkout(root: &Path, number: u64) -> Result<String> {
    run(root, &["pr", "checkout", &number.to_string()])
}

pub fn create(root: &Path, title: &str, body: &str, base: &str, head: &str) -> Result<String> {
    ensure!(
        !title.trim().is_empty() && !base.trim().is_empty() && !head.trim().is_empty(),
        "Title, base and head are required"
    );
    run(
        root,
        &[
            "pr", "create", "--draft", "--title", title, "--body", body, "--base", base, "--head",
            head,
        ],
    )
}

pub fn merge(root: &Path, pr: &PullRequest) -> Result<String> {
    run(
        root,
        &[
            "pr",
            "merge",
            &pr.number.to_string(),
            "--squash",
            "--match-head-commit",
            &pr.head_ref_oid,
        ],
    )
}

pub fn review(root: &Path, number: u64, verdict: &str, body: &str) -> Result<String> {
    ensure!(
        ["--approve", "--request-changes", "--comment"].contains(&verdict),
        "Invalid review verdict"
    );
    run(
        root,
        &["pr", "review", &number.to_string(), verdict, "--body", body],
    )
}

pub fn comment(root: &Path, number: u64, body: &str) -> Result<String> {
    ensure!(!body.trim().is_empty(), "Comment cannot be empty");
    run(
        root,
        &["pr", "comment", &number.to_string(), "--body", body],
    )
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubRepo {
    pub name_with_owner: String,
    pub url: String,
    pub is_private: bool,
}

pub fn repositories(root: &Path) -> Result<Vec<GitHubRepo>> {
    serde_json::from_str(&run(
        root,
        &[
            "repo",
            "list",
            "--limit",
            "100",
            "--json",
            "nameWithOwner,url,isPrivate",
        ],
    )?)
    .context("Invalid repository response")
}

pub fn clone_repository(root: &Path, name: &str, destination: &Path) -> Result<String> {
    ensure!(!destination.exists(), "Clone destination already exists");
    let destination = destination
        .to_str()
        .context("GitHub clone path must be UTF-8")?;
    run(root, &["repo", "clone", name, destination])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publishing_is_explicit_and_merge_matches_reviewed_head() {
        let root = Path::new(".");
        CAPTURE.with(|c| *c.borrow_mut() = Some(Vec::new()));
        assert!(create(root, "", "body", "main", "topic").is_err());
        assert!(review(root, 7, "--admin", "body").is_err());
        assert!(comment(root, 7, " ").is_err());
        create(
            root,
            "Title",
            "literal $(touch unexpected); body",
            "main",
            "owner:topic",
        )
        .unwrap();
        let pr = PullRequest {
            number: 7,
            title: "Title".into(),
            url: "url".into(),
            head_ref_name: "topic".into(),
            base_ref_name: "main".into(),
            head_ref_oid: "reviewed-sha".into(),
            is_draft: false,
        };
        merge(root, &pr).unwrap();
        review(root, 7, "--request-changes", "Fix regression").unwrap();
        comment(root, 7, "Please check").unwrap();
        let calls = CAPTURE.with(|c| c.borrow_mut().take().unwrap());
        assert_eq!(calls.len(), 4);
        assert!(calls[0].contains(&"--draft".into()));
        assert!(calls[0].contains(&"literal $(touch unexpected); body".into()));
        assert!(calls[0].contains(&"owner:topic".into()));
        assert_eq!(
            calls[1],
            [
                "pr",
                "merge",
                "7",
                "--squash",
                "--match-head-commit",
                "reviewed-sha"
            ]
        );
        assert_eq!(
            calls[2],
            [
                "pr",
                "review",
                "7",
                "--request-changes",
                "--body",
                "Fix regression"
            ]
        );
        assert_eq!(calls[3], ["pr", "comment", "7", "--body", "Please check"]);
    }

    #[test]
    fn check_runs_and_legacy_statuses_are_readable() {
        let json = serde_json::json!({"number": 7, "title": "Change", "headRefName": "topic", "baseRefName": "main", "statusCheckRollup": [{"name":"Tests", "conclusion":"SUCCESS", "status":"COMPLETED"}, {"context":"Lint", "state":"FAILURE"}], "reviews":[{"author":{"login":"reviewer"}, "state":"CHANGES_REQUESTED", "body":"Fix bug"}], "comments":[]});
        let detail = format_detail(&json);
        assert!(detail.contains("Tests: SUCCESS") && detail.contains("Lint: FAILURE"));
        assert!(detail.contains("reviewer CHANGES_REQUESTED") && detail.contains("Fix bug"));
        assert!(detail.contains("topic → main"));
    }

    #[test]
    fn pr_json_requires_identity_and_head_commit() {
        let json = r#"[{"number":7,"title":"review","url":"https://github.com/o/r/pull/7","headRefName":"topic","baseRefName":"main","headRefOid":"abc","isDraft":true}]"#;
        let prs: Vec<PullRequest> = serde_json::from_str(json).unwrap();
        assert_eq!(prs[0].number, 7);
        assert!(
            serde_json::from_str::<Vec<PullRequest>>(&json.replace("\"headRefOid\":\"abc\",", ""))
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn command_transport_preserves_arguments_cwd_and_reports_failures() {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("wirdo-gh-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        let program = path.join("fake-gh");
        std::fs::write(&program, "#!/bin/sh\n[ \"$GH_PROMPT_DISABLED\" = 1 ] || exit 9\n[ \"$1\" = fail ] && { echo 'auth failed' >&2; exit 1; }\nprintf '%s\\n' \"$PWD\" \"$@\"\n").unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        let output = run_program(
            &path,
            program.as_os_str(),
            &[
                "pr",
                "comment",
                "--body",
                "literal $(echo secret); two words",
            ],
        )
        .unwrap();
        assert!(output.contains("literal $(echo secret); two words"));
        assert!(output.contains(path.to_str().unwrap()));
        assert!(
            run_program(&path, program.as_os_str(), &["fail"])
                .unwrap_err()
                .to_string()
                .contains("auth failed")
        );
        std::fs::remove_dir_all(path).unwrap();
    }
}
