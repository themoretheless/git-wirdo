use crate::github::{PullRequest, get_pr, run};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct PrFile {
    pub filename: String,
    pub sha: String,
    pub status: String,
    pub previous_filename: Option<String>,
    pub patch: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub old: Option<u32>,
    pub new: Option<u32>,
    pub text: String,
}
impl PrFile {
    pub fn lines(&self) -> Result<Vec<DiffLine>> {
        let Some(patch) = &self.patch else {
            return Ok(Vec::new());
        };
        let mut result = Vec::new();
        let mut old = None;
        let mut new = None;
        let mut old_left = 0_u32;
        let mut new_left = 0_u32;
        for text in patch.lines() {
            if text.starts_with("@@ ") {
                ensure!(old_left == 0 && new_left == 0, "Truncated diff hunk");
                let header = text.split(" @@").next().context("Invalid diff hunk")?;
                let fields: Vec<_> = header.split_whitespace().collect();
                ensure!(
                    fields.len() == 3 && fields[0] == "@@",
                    "Invalid diff hunk header"
                );
                let parse = |value: &str, prefix: char| -> Result<(u32, u32)> {
                    let value = value.strip_prefix(prefix).context("Invalid hunk side")?;
                    let (start, count) = value.split_once(',').unwrap_or((value, "1"));
                    Ok((start.parse()?, count.parse()?))
                };
                let (start, count) = parse(fields[1], '-')?;
                old = Some(start);
                old_left = count;
                let (start, count) = parse(fields[2], '+')?;
                new = Some(start);
                new_left = count;
                result.push(DiffLine {
                    old: None,
                    new: None,
                    text: text.into(),
                });
                continue;
            }
            let mut line = DiffLine {
                old: None,
                new: None,
                text: text.into(),
            };
            match text.as_bytes().first() {
                Some(b'-') => {
                    ensure!(old_left > 0, "Invalid removed line");
                    line.old = old;
                    old = old.and_then(|n| n.checked_add(1));
                    old_left -= 1;
                }
                Some(b'+') => {
                    ensure!(new_left > 0, "Invalid added line");
                    line.new = new;
                    new = new.and_then(|n| n.checked_add(1));
                    new_left -= 1;
                }
                Some(b' ') => {
                    ensure!(old_left > 0 && new_left > 0, "Invalid context line");
                    line.old = old;
                    line.new = new;
                    old = old.and_then(|n| n.checked_add(1));
                    new = new.and_then(|n| n.checked_add(1));
                    old_left -= 1;
                    new_left -= 1;
                }
                Some(b'\\') if text == "\\ No newline at end of file" => {}
                _ => anyhow::bail!("Invalid diff line"),
            }
            result.push(line);
        }
        ensure!(old_left == 0 && new_left == 0, "Truncated diff hunk");
        Ok(result)
    }
    pub fn detail(&self, pr: &PullRequest) -> Result<String> {
        let mut text = format!(
            "PR #{} {}\n{}\nHead: {}\nOld line | New line | patch\nC comments on a displayed LEFT/RIGHT line.\n\n",
            pr.number,
            self.filename.escape_debug(),
            self.status,
            pr.head_ref_oid
        );
        if self.patch.is_none() {
            text.push_str("Patch unavailable (binary, large or truncated); inline line comments are unavailable.");
        }
        for line in self.lines()? {
            text.push_str(&format!(
                "{:>8} | {:>8} | {}\n",
                line.old.map(|n| n.to_string()).unwrap_or_default(),
                line.new.map(|n| n.to_string()).unwrap_or_default(),
                line.text
            ));
        }
        Ok(text)
    }
}
pub fn files(root: &Path, number: u64) -> Result<Vec<PrFile>> {
    let endpoint = format!("repos/{{owner}}/{{repo}}/pulls/{number}/files?per_page=100");
    let pages: Vec<Vec<PrFile>> =
        serde_json::from_str(&run(root, &["api", &endpoint, "--paginate", "--slurp"])?)
            .context("Invalid PR file pages")?;
    Ok(pages.into_iter().flatten().collect())
}
pub fn reviewed_files(root: &Path, pr: &PullRequest) -> Result<Vec<PrFile>> {
    ensure!(
        get_pr(root, pr.number)?.head_ref_oid == pr.head_ref_oid,
        "PR head changed; refresh the PR list"
    );
    let files = files(root, pr.number)?;
    ensure!(
        get_pr(root, pr.number)?.head_ref_oid == pr.head_ref_oid,
        "PR head changed while loading files; refresh the PR list"
    );
    Ok(files)
}
pub fn inline_comment(
    root: &Path,
    pr: &PullRequest,
    file: &PrFile,
    side: &str,
    line: u32,
    body: &str,
) -> Result<String> {
    ensure!(
        ["LEFT", "RIGHT"].contains(&side),
        "Side must be LEFT or RIGHT"
    );
    ensure!(
        line > 0 && !body.trim().is_empty(),
        "Positive line and non-empty comment required"
    );
    ensure!(
        file.lines()?.iter().any(|l| if side == "LEFT" {
            l.old == Some(line)
        } else {
            l.new == Some(line)
        }),
        "Line is not in the displayed diff"
    );
    let fresh = reviewed_files(root, pr)?;
    ensure!(
        fresh.iter().any(|current| current == file),
        "File diff changed; refresh before commenting"
    );
    let endpoint = format!("repos/{{owner}}/{{repo}}/pulls/{}/comments", pr.number);
    let body = serde_json::to_vec(
        &serde_json::json!({"commit_id": pr.head_ref_oid, "path": file.filename, "side": side, "line":line, "body":body}),
    )?;
    crate::github::run_json(
        root,
        &["api", &endpoint, "--method", "POST", "--input", "-"],
        &body,
    )
}
pub fn comments(root: &Path, number: u64) -> Result<String> {
    let endpoint = format!("repos/{{owner}}/{{repo}}/pulls/{number}/comments?per_page=100");
    let pages: Vec<Vec<serde_json::Value>> =
        serde_json::from_str(&run(root, &["api", &endpoint, "--paginate", "--slurp"])?)
            .context("Invalid review comment pages")?;
    let mut text = String::from("Inline review comments\n");
    for comment in pages.into_iter().flatten() {
        text.push_str(&format!(
            "{} {} {}:{}\n{}\n{}\n\n",
            comment["user"]["login"].as_str().unwrap_or("unknown"),
            comment["side"].as_str().unwrap_or(""),
            comment["path"].as_str().unwrap_or(""),
            comment["line"]
                .as_u64()
                .or_else(|| comment["original_line"].as_u64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "outdated/file".into()),
            comment["body"].as_str().unwrap_or(""),
            comment["html_url"].as_str().unwrap_or("")
        ));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::Mock;
    fn pr() -> PullRequest {
        PullRequest {
            number: 7,
            title: "Review".into(),
            url: "https://github.com/o/r/pull/7".into(),
            head_ref_name: "topic".into(),
            base_ref_name: "main".into(),
            head_ref_oid: "a".repeat(40),
            is_draft: false,
        }
    }
    fn pr_json(pr: &PullRequest) -> String {
        serde_json::json!({"number":pr.number,"title":pr.title,"url":pr.url,"headRefName":pr.head_ref_name,"baseRefName":pr.base_ref_name,"headRefOid":pr.head_ref_oid,"isDraft":pr.is_draft}).to_string()
    }
    fn file() -> PrFile {
        PrFile {
            filename: "odd\nfile [1].rs".into(),
            sha: "blob".into(),
            status: "modified".into(),
            previous_filename: None,
            patch: Some(
                "@@ -4,2 +4,3 @@ function\n same\n-old\n+new\n+added\n\\ No newline at end of file"
                    .into(),
            ),
        }
    }
    fn file_json(file: &PrFile) -> serde_json::Value {
        serde_json::json!({"filename":file.filename,"sha":file.sha,"status":file.status,"previous_filename":file.previous_filename,"patch":file.patch})
    }
    #[test]
    fn patch_line_mapping_distinguishes_context_additions_and_removals() {
        let lines = file().lines().unwrap();
        assert_eq!((lines[1].old, lines[1].new), (Some(4), Some(4)));
        assert_eq!((lines[2].old, lines[2].new), (Some(5), None));
        assert_eq!((lines[3].old, lines[3].new), (None, Some(5)));
        assert_eq!((lines[4].old, lines[4].new), (None, Some(6)));
        assert_eq!((lines[5].old, lines[5].new), (None, None));
        let detail = file().detail(&pr()).unwrap();
        assert!(detail.contains("odd\\nfile") && detail.contains("Old line | New line"));
    }
    #[test]
    fn new_deleted_multiple_hunks_and_truncated_patches_have_explicit_boundaries() {
        let mut f = file();
        for patch in [
            "@@ -0,0 +1 @@\n+new",
            "@@ -1 +0,0 @@\n-old",
            "@@ -1 +1 @@\n-old\n+new\n@@ -20 +20 @@\n same",
        ] {
            f.patch = Some(patch.into());
            assert!(!f.lines().unwrap().is_empty());
        }
        for patch in [
            "@@ -1,2 +1,2 @@\n-only",
            "@@ broken @@\n x",
            "+without hunk",
            "@@ -1 +1 @@\n x\n+overflow",
        ] {
            f.patch = Some(patch.into());
            assert!(f.lines().is_err());
        }
        f.patch = None;
        assert!(
            f.lines().unwrap().is_empty() && f.detail(&pr()).unwrap().contains("Patch unavailable")
        );
    }
    #[test]
    fn files_follow_all_api_pages_without_parsing_quoted_diff_paths() {
        let mut renamed = file();
        renamed.filename = "new name".into();
        renamed.previous_filename = Some("old name".into());
        renamed.status = "renamed".into();
        let mock = Mock::new([
            serde_json::json!([[file_json(&file())], [file_json(&renamed)]]).to_string(),
        ]);
        let result = files(Path::new("."), 7).unwrap();
        assert_eq!(result, [file(), renamed]);
        let args = &mock.calls()[0];
        assert!(args.contains(&"--paginate".into()) && args.contains(&"--slurp".into()));
    }
    #[test]
    fn inline_comment_pins_head_filename_side_and_line_with_structured_stdin() {
        let pr = pr();
        let f = file();
        let mock = Mock::new([
            pr_json(&pr),
            serde_json::json!([[file_json(&f)]]).to_string(),
            pr_json(&pr),
            "posted".into(),
        ]);
        let text = "literal $(touch unexpected)\nsecond \"line\"";
        inline_comment(Path::new("."), &pr, &f, "RIGHT", 6, text).unwrap();
        let calls = mock.calls();
        assert_eq!(calls.len(), 4);
        assert_eq!(
            calls[3],
            [
                "api",
                "repos/{owner}/{repo}/pulls/7/comments",
                "--method",
                "POST",
                "--input",
                "-"
            ]
        );
        let payload: serde_json::Value = serde_json::from_slice(&mock.payloads()[0]).unwrap();
        assert_eq!(payload["commit_id"], pr.head_ref_oid);
        assert_eq!(payload["path"], f.filename);
        assert_eq!(payload["body"], text);
        assert_eq!(payload["side"], "RIGHT");
        assert_eq!(payload["line"], 6);
    }
    #[test]
    fn stale_head_or_patch_prevents_comment_publication() {
        for head_changed in [true, false] {
            let pr = pr();
            let f = file();
            let mut changed_pr = pr.clone();
            changed_pr.head_ref_oid = "b".repeat(40);
            let mut changed_file = f.clone();
            changed_file.patch = Some(f.patch.clone().unwrap().replace("+new", "+changed"));
            let responses = if head_changed {
                vec![pr_json(&changed_pr)]
            } else {
                vec![
                    pr_json(&pr),
                    serde_json::json!([[file_json(&changed_file)]]).to_string(),
                    pr_json(&pr),
                ]
            };
            let mock = Mock::new(responses);
            assert!(inline_comment(Path::new("."), &pr, &f, "LEFT", 5, "comment").is_err());
            assert!(!mock.calls().iter().flatten().any(|arg| arg == "POST"));
            assert!(mock.payloads().is_empty());
        }
    }
    #[test]
    fn invalid_or_undisplayed_lines_never_contact_github() {
        let mock = Mock::new([]);
        let pr = pr();
        let f = file();
        for (side, line, body) in [
            ("RIGHT", 100, "text"),
            ("LEFT", 6, "text"),
            ("WRONG", 4, "text"),
            ("RIGHT", 0, "text"),
            ("RIGHT", 4, ""),
        ] {
            assert!(inline_comment(Path::new("."), &pr, &f, side, line, body).is_err());
        }
        assert!(mock.calls().is_empty());
    }
    #[test]
    fn a_head_change_during_page_loading_invalidates_the_entire_review() {
        let pr = pr();
        let mut changed = pr.clone();
        changed.head_ref_oid = "c".repeat(40);
        let mock = Mock::new([
            pr_json(&pr),
            serde_json::json!([[file_json(&file())]]).to_string(),
            pr_json(&changed),
        ]);
        assert!(reviewed_files(Path::new("."), &pr).is_err());
        assert_eq!(mock.calls().len(), 3);
    }
    #[test]
    fn inline_comment_display_preserves_outdated_location_and_threads() {
        let mock=Mock::new([serde_json::json!([[{"user":{"login":"reviewer"},"path":"file","original_line":8,"line":null,"side":"LEFT","body":"first","html_url":"url"}],[{"user":{"login":"author"},"path":"file","line":8,"side":"LEFT","body":"reply"}]]).to_string()]);
        let detail = comments(Path::new("."), 7).unwrap();
        assert!(detail.contains("reviewer LEFT file:8") && detail.contains("reply"));
        assert!(mock.calls()[0].contains(&"--paginate".into()));
    }
}
