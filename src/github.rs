use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
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
    static RESPONSES: std::cell::RefCell<std::collections::VecDeque<String>> = const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
    static PAYLOADS: std::cell::RefCell<Vec<Vec<u8>>> = const { std::cell::RefCell::new(Vec::new()) };
    static CAPTURE: std::cell::RefCell<Option<Vec<Vec<String>>>> = const { std::cell::RefCell::new(None) };
}

pub fn run(root: &Path, args: &[&str]) -> Result<String> {
    run_input(root, args, None)
}

pub(crate) fn run_json(root: &Path, args: &[&str], body: &[u8]) -> Result<String> {
    run_input(root, args, Some(body))
}

fn run_input(root: &Path, args: &[&str], input: Option<&[u8]>) -> Result<String> {
    #[cfg(test)]
    if CAPTURE.with(|capture| {
        if let Some(calls) = capture.borrow_mut().as_mut() {
            calls.push(args.iter().map(|arg| (*arg).to_owned()).collect());
            true
        } else {
            false
        }
    }) {
        if let Some(input) = input {
            PAYLOADS.with(|p| p.borrow_mut().push(input.to_vec()));
        }
        return Ok(RESPONSES.with(|r| {
            r.borrow_mut()
                .pop_front()
                .unwrap_or_else(|| "fixture result".into())
        }));
    }

    run_program_input(root, std::ffi::OsStr::new("gh"), args, input)
}

#[cfg(all(test, unix))]
fn run_program(root: &Path, program: &std::ffi::OsStr, args: &[&str]) -> Result<String> {
    run_program_input(root, program, args, None)
}

fn run_program_input(
    root: &Path,
    program: &std::ffi::OsStr,
    args: &[&str],
    input: Option<&[u8]>,
) -> Result<String> {
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
    let output = crate::process::output_with_input(&mut command, input)
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrFilter {
    pub state: String,
    pub search: String,
    pub limit: usize,
}
impl Default for PrFilter {
    fn default() -> Self {
        Self {
            state: "open".into(),
            search: String::new(),
            limit: 100,
        }
    }
}
impl PrFilter {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            ["open", "closed", "merged", "all"].contains(&self.state.as_str()),
            "PR state must be open, closed, merged or all"
        );
        ensure!(self.limit > 0, "PR limit must be positive");
        Ok(())
    }
}
pub fn list(root: &Path) -> Result<Vec<PullRequest>> {
    list_filtered(root, &PrFilter::default())
}
pub fn list_filtered(root: &Path, filter: &PrFilter) -> Result<Vec<PullRequest>> {
    filter.validate()?;
    let limit = filter.limit.to_string();
    let mut args = vec![
        "pr",
        "list",
        "--state",
        &filter.state,
        "--limit",
        &limit,
        "--json",
        "number,title,url,headRefName,baseRefName,headRefOid,isDraft",
    ];
    if !filter.search.is_empty() {
        args.extend(["--search", &filter.search]);
    }
    serde_json::from_str(&run(root, &args)?).context("Invalid PR response from gh")
}
pub fn get_pr(root: &Path, number: u64) -> Result<PullRequest> {
    serde_json::from_str(&run(
        root,
        &[
            "pr",
            "view",
            &number.to_string(),
            "--json",
            "number,title,url,headRefName,baseRefName,headRefOid,isDraft",
        ],
    )?)
    .context("Invalid selected PR response")
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}
impl MergeMethod {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "merge" => Ok(Self::Merge),
            "squash" => Ok(Self::Squash),
            "rebase" => Ok(Self::Rebase),
            _ => anyhow::bail!("Choose merge, squash or rebase"),
        }
    }
    fn flag(self) -> &'static str {
        match self {
            Self::Merge => "--merge",
            Self::Squash => "--squash",
            Self::Rebase => "--rebase",
        }
    }
}
pub fn merge(root: &Path, pr: &PullRequest) -> Result<String> {
    merge_with_method(root, pr, MergeMethod::Squash)
}
pub fn merge_with_method(root: &Path, pr: &PullRequest, method: MergeMethod) -> Result<String> {
    run(
        root,
        &[
            "pr",
            "merge",
            &pr.number.to_string(),
            method.flag(),
            "--match-head-commit",
            &pr.head_ref_oid,
        ],
    )
}
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EditablePr {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub base_ref_name: String,
}
pub fn editable(root: &Path, number: u64) -> Result<EditablePr> {
    serde_json::from_str(&run(
        root,
        &[
            "pr",
            "view",
            &number.to_string(),
            "--json",
            "number,title,body,baseRefName",
        ],
    )?)
    .context("Invalid PR edit response")
}
pub fn edit(
    root: &Path,
    expected: &EditablePr,
    title: &str,
    body: &str,
    base: &str,
) -> Result<String> {
    ensure!(
        !title.trim().is_empty() && !base.trim().is_empty(),
        "PR title and base are required"
    );
    ensure!(
        &editable(root, expected.number)? == expected,
        "PR metadata changed; reopen the edit form"
    );
    run(
        root,
        &[
            "pr",
            "edit",
            &expected.number.to_string(),
            "--title",
            title,
            "--body",
            body,
            "--base",
            base,
        ],
    )
}
pub fn set_ready(root: &Path, pr: &PullRequest) -> Result<String> {
    let current = get_pr(root, pr.number)?;
    ensure!(
        current.head_ref_oid == pr.head_ref_oid && current.is_draft == pr.is_draft,
        "PR head or draft state changed; refresh before confirming"
    );
    let number = pr.number.to_string();
    let mut args = vec!["pr", "ready", &number];
    if !pr.is_draft {
        args.push("--undo");
    }
    run(root, &args)
}

pub fn review_selected(root: &Path, pr: &PullRequest, verdict: &str, body: &str) -> Result<String> {
    let event = match verdict {
        "--approve" => "APPROVE",
        "--request-changes" => "REQUEST_CHANGES",
        "--comment" => "COMMENT",
        _ => anyhow::bail!("Invalid review verdict"),
    };
    ensure!(
        event == "APPROVE" || !body.trim().is_empty(),
        "Review body is required for comments or requested changes"
    );
    ensure!(
        get_pr(root, pr.number)?.head_ref_oid == pr.head_ref_oid,
        "PR head changed; refresh and review before submitting"
    );
    let endpoint = format!("repos/{{owner}}/{{repo}}/pulls/{}/reviews", pr.number);
    let payload = serde_json::to_vec(
        &serde_json::json!({"commit_id":pr.head_ref_oid,"event":event,"body":body}),
    )?;
    run_json(
        root,
        &["api", &endpoint, "--method", "POST", "--input", "-"],
        &payload,
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
pub(crate) struct Mock;
#[cfg(test)]
impl Mock {
    pub(crate) fn new(responses: impl IntoIterator<Item = String>) -> Self {
        CAPTURE.with(|c| *c.borrow_mut() = Some(Vec::new()));
        RESPONSES.with(|r| *r.borrow_mut() = responses.into_iter().collect());
        PAYLOADS.with(|p| p.borrow_mut().clear());
        Self
    }
    pub(crate) fn calls(&self) -> Vec<Vec<String>> {
        CAPTURE.with(|c| c.borrow().as_ref().unwrap().clone())
    }
    pub(crate) fn payloads(&self) -> Vec<Vec<u8>> {
        PAYLOADS.with(|p| p.borrow().clone())
    }
}
#[cfg(test)]
impl Drop for Mock {
    fn drop(&mut self) {
        CAPTURE.with(|c| *c.borrow_mut() = None);
        RESPONSES.with(|r| r.borrow_mut().clear());
        PAYLOADS.with(|p| p.borrow_mut().clear());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn press(app: &mut crate::app::App, text: &str) {
        for c in text.chars() {
            app.handle_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(c),
                crossterm::event::KeyModifiers::NONE,
            ));
        }
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ));
    }
    fn selected_app() -> (crate::test_support::TestRepo, crate::app::App) {
        let repo = crate::test_support::TestRepo::new();
        let mut app = crate::app::App::new(repo.open()).unwrap();
        app.view = crate::model::ViewMode::PullRequests;
        app.pull_requests.push(PullRequest {
            number: 7,
            title: "Title".into(),
            url: "url".into(),
            head_ref_name: "topic".into(),
            base_ref_name: "main".into(),
            head_ref_oid: "a".repeat(40),
            is_draft: true,
        });
        (repo, app)
    }
    #[test]
    fn tui_edit_prefills_current_fields_and_submits_multiline_body_without_shell_interpretation() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let (_repo, mut app) = selected_app();
        let metadata =
            serde_json::json!({"number":7,"title":"Title","body":"Old\nbody","baseRefName":"main"})
                .to_string();
        let mock = Mock::new([metadata.clone(), metadata, "edited".into(), "[]".into()]);
        app.handle(crate::app::Action::EditRemote);
        assert_eq!(app.prompt.as_ref().unwrap().text, "Title");
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.prompt.as_ref().unwrap().text, "Old\nbody");
        app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        app.handle_paste("new literal $(not a command)\nsecond line");
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
        app.handle_paste("third line");
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.prompt.as_ref().unwrap().text, "main");
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(
            !app.message_is_error && app.prompt.is_none(),
            "{}",
            app.message
        );
        assert!(
            mock.calls()[2]
                .contains(&"new literal $(not a command)\nsecond line\nthird line".into())
        );
    }
    #[test]
    fn tui_merge_requires_number_and_method_and_preserves_head_pin() {
        let (_repo, mut app) = selected_app();
        let mock = Mock::new(["merged".into(), "[]".into()]);
        app.handle(crate::app::Action::MergePr);
        press(&mut app, "7");
        assert!(mock.calls().is_empty());
        assert!(app.prompt.as_ref().unwrap().labels[1].contains("Merge method"));
        press(&mut app, "rebase");
        assert!(!app.message_is_error, "{}", app.message);
        assert_eq!(&mock.calls()[0][3], "--rebase");
        assert!(mock.calls()[0].contains(&"a".repeat(40)));
    }
    #[test]
    fn invalid_filter_submission_keeps_previous_list_and_never_calls_provider() {
        let (_repo, mut app) = selected_app();
        let mock = Mock::new([]);
        app.handle(crate::app::Action::SetUpstream);
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('u'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        press(&mut app, "invalid");
        press(&mut app, "query");
        assert!(
            app.message_is_error
                && app.pr_filter.state == "open"
                && app.pull_requests[0].number == 7
        );
        assert!(mock.calls().is_empty());
    }

    #[test]
    fn selected_approval_pins_the_reviewed_commit_and_rejects_changed_head() {
        let (_repo, app) = selected_app();
        let pr = app.pull_requests[0].clone();
        let json = serde_json::json!({"number":7,"title":"Title","url":"url","headRefName":"topic","baseRefName":"main","headRefOid":pr.head_ref_oid,"isDraft":true});
        let mock = Mock::new([json.to_string(), "approved".into()]);
        review_selected(Path::new("."), &pr, "--approve", "").unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&mock.payloads()[0]).unwrap();
        assert_eq!(payload["event"], "APPROVE");
        assert_eq!(payload["commit_id"], pr.head_ref_oid);
        drop(mock);
        let mut changed = json;
        changed["headRefOid"] = serde_json::json!("b".repeat(40));
        let mock = Mock::new([changed.to_string()]);
        assert!(review_selected(Path::new("."), &pr, "--approve", "").is_err());
        assert_eq!(mock.calls().len(), 1);
        assert!(mock.payloads().is_empty());
    }

    #[test]
    fn filters_are_validated_and_incremental_limits_are_sent_to_gh() {
        let mock = Mock::new(["[]".into()]);
        let f = PrFilter {
            state: "merged".into(),
            search: "author:@me label:bug".into(),
            limit: 250,
        };
        list_filtered(Path::new("."), &f).unwrap();
        assert!(mock.calls()[0].contains(&"250".into()) && mock.calls()[0].contains(&f.search));
        let mut invalid = f;
        invalid.state = "--admin".into();
        assert!(list_filtered(Path::new("."), &invalid).is_err());
        assert_eq!(mock.calls().len(), 1);
    }
    #[test]
    fn all_merge_methods_match_the_reviewed_head_without_admin_bypass() {
        let mock = Mock::new([]);
        let pr = PullRequest {
            number: 7,
            title: "Title".into(),
            url: "url".into(),
            head_ref_name: "topic".into(),
            base_ref_name: "main".into(),
            head_ref_oid: "a".repeat(40),
            is_draft: false,
        };
        for (name, flag) in [
            ("merge", "--merge"),
            ("squash", "--squash"),
            ("rebase", "--rebase"),
        ] {
            merge_with_method(Path::new("."), &pr, MergeMethod::parse(name).unwrap()).unwrap();
            let call = mock.calls().last().unwrap().clone();
            assert!(call.contains(&flag.into()) && call.contains(&pr.head_ref_oid));
            assert!(!call.contains(&"--admin".into()));
        }
        assert!(MergeMethod::parse("--admin").is_err());
    }
    #[test]
    fn editing_checks_current_metadata_and_preserves_multiline_literal_body() {
        let expected = EditablePr {
            number: 7,
            title: "Original".into(),
            body: "Old".into(),
            base_ref_name: "main".into(),
        };
        let json =
            serde_json::json!({"number":7,"title":"Original","body":"Old","baseRefName":"main"})
                .to_string();
        let mock = Mock::new([json, "edited".into()]);
        let body = "line one\n$(literal) \"quote\"";
        edit(Path::new("."), &expected, "Replacement", body, "develop").unwrap();
        assert_eq!(
            mock.calls()[1],
            [
                "pr",
                "edit",
                "7",
                "--title",
                "Replacement",
                "--body",
                body,
                "--base",
                "develop"
            ]
        );
        drop(mock);
        let mock = Mock::new([
            serde_json::json!({"number":7,"title":"External","body":"Old","baseRefName":"main"})
                .to_string(),
        ]);
        assert!(edit(Path::new("."), &expected, "Replacement", body, "main").is_err());
        assert_eq!(mock.calls().len(), 1);
    }
    #[test]
    fn draft_transition_checks_head_and_state_and_supports_undo() {
        for draft in [true, false] {
            let pr = PullRequest {
                number: 7,
                title: "Title".into(),
                url: "url".into(),
                head_ref_name: "topic".into(),
                base_ref_name: "main".into(),
                head_ref_oid: "a".repeat(40),
                is_draft: draft,
            };
            let json=serde_json::json!({"number":7,"title":"Title","url":"url","headRefName":"topic","baseRefName":"main","headRefOid":pr.head_ref_oid,"isDraft":draft}).to_string();
            let mock = Mock::new([json, "ready".into()]);
            set_ready(Path::new("."), &pr).unwrap();
            assert_eq!(mock.calls()[1].contains(&"--undo".into()), !draft);
        }
    }

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
