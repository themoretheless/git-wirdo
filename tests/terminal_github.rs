#![cfg(unix)]
mod support;
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use support::{TempDirectory, TestRepo, terminal::TerminalFixture};

struct GitHubFixture {
    repo: TestRepo,
    directory: TempDirectory,
    head: String,
}
impl GitHubFixture {
    fn new() -> Self {
        let repo = TestRepo::new();
        repo.write("base", "base\n");
        repo.commit_all("base");
        repo.git(&["switch", "-c", "topic"]);
        repo.write("topic", "topic\n");
        repo.commit_all("topic change");
        let head = repo.git(&["rev-parse", "HEAD"]).trim().to_owned();
        repo.git(&["switch", "main"]);
        let directory = TempDirectory::new();
        let state = serde_json::json!({
            "source": repo.path,
            "pr": {"number":7, "title":"Fixture PR", "url":"https://example.invalid/pr/7", "body":"Fixture description", "headRefName":"topic", "baseRefName":"main", "headRefOid":head, "isDraft":true, "state":"OPEN", "reviewDecision":"REVIEW_REQUIRED", "mergeable":"MERGEABLE", "statusCheckRollup":[{"name":"Fixture check", "conclusion":"SUCCESS"}], "comments":[], "reviews":[]},
            "files": [
                {"filename":"review [a]\nfile.rs", "sha":"1".repeat(40), "status":"renamed", "previous_filename":"before.rs", "patch":"@@ -4,2 +4,3 @@ function\n same\n-old\n+new\n+added"},
                {"filename":"binary.bin", "sha":"2".repeat(40), "status":"added", "previous_filename":null, "patch":null}
            ],
            "inline": [{"user":{"login":"reviewer"},"path":"before.rs","side":"RIGHT","line":4,"body":"Existing inline","html_url":"https://example.invalid/inline/1"}]
        });
        std::fs::write(
            directory.0.join("state.json"),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
        let script = directory.0.join("gh");
        std::fs::write(&script, include_str!("fixtures/gh_terminal.py")).unwrap();
        std::fs::set_permissions(script, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            repo,
            directory,
            head,
        }
    }
    fn terminal(&self) -> TerminalFixture {
        let mut paths = vec![self.directory.0.clone()];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        TerminalFixture::spawn_with(
            Path::new(env!("CARGO_BIN_EXE_git-wirdo")),
            &self.repo.path,
            |command| {
                command
                    .env("PATH", std::env::join_paths(paths).unwrap())
                    .env("GH_FIXTURE_DIR", &self.directory.0);
            },
        )
    }
    fn state(&self) -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(self.directory.0.join("state.json")).unwrap())
            .unwrap()
    }
    fn write_state(&self, state: &serde_json::Value) {
        std::fs::write(
            self.directory.0.join("state.json"),
            serde_json::to_vec(state).unwrap(),
        )
        .unwrap();
    }
    fn calls(&self) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.directory.0.join("calls.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
    fn posts(&self) -> Vec<serde_json::Value> {
        self.calls()
            .into_iter()
            .filter(|call| !call["payload"].is_null())
            .collect()
    }
}

#[test]
fn pr_filter_edit_draft_review_comments_stale_head_and_merge_use_native_cli_transport() {
    let fixture = GitHubFixture::new();
    let mut terminal = fixture.terminal();
    terminal.view("PullRequests");
    terminal.expect("Fixture PR");
    terminal.key(b"/", "Find: ");
    let screen = terminal.field("Existing inline", "Action: Find: Existing inline");
    assert!(screen.contains("https://example.invalid/inline/1"));
    terminal.key(b"G", "Fixture authenticated");
    terminal.key(b"r", "Action: Refreshed");
    terminal.key(b"U", "PR state");
    terminal.send(b"\x15"); // Ctrl-U clears the prefilled open state
    terminal.field("all", "GitHub search query");
    terminal.field("author:@me", "Updated PR filter");
    terminal.key(b"+", "Loaded 1 PRs (limit 200)");
    terminal.key(b"L", "PR title:");
    terminal.send(b"\x15");
    let title = "Edited literal $(echo nope)";
    terminal.field(title, "PR body");
    terminal.send(b"\x15");
    let body = "Multiline ü body\nqPcs $(echo untouched)";
    terminal.field(body, "Base branch:");
    terminal.key(b"\r", "Edited PR fixture");
    assert_eq!(fixture.state()["pr"]["title"], title);
    assert_eq!(fixture.state()["pr"]["body"], body);
    terminal.key(b"T", "Type selected PR number");
    terminal.field("7", "Updated draft fixture");
    assert_eq!(fixture.state()["pr"]["isDraft"], false);
    terminal.key(b"T", "Type selected PR number");
    terminal.field("7", "Updated draft fixture");
    assert_eq!(fixture.state()["pr"]["isDraft"], true);
    terminal.key(b"A", "Approve review body:");
    terminal.field("Approved\nreview", "Submitted review fixture");
    terminal.key(b"R", "Requested changes:");
    terminal.field("Fix the fixture\nplease", "Submitted review fixture");
    terminal.key(b"C", "PR comment:");
    terminal.field("Conversation\ncomment", "Commented PR fixture");
    terminal.key(b"i", "View: PrFiles");
    terminal.expect("review [a]\\nfile.rs");
    terminal.key(b"C", "Diff side: LEFT or RIGHT:");
    terminal.field("RIGHT", "Displayed old/new line number:");
    terminal.field("6", "Inline comment body:");
    let inline = "Line comment ü\nwith qPcs";
    terminal.field(inline, "Posted inline fixture");
    let posts = fixture.posts();
    assert_eq!(posts.len(), 3);
    assert_eq!(posts[0]["payload"]["commit_id"], fixture.head);
    assert_eq!(posts[0]["payload"]["event"], "APPROVE");
    assert_eq!(posts[1]["payload"]["event"], "REQUEST_CHANGES");
    assert_eq!(posts[2]["payload"]["path"], "review [a]\nfile.rs");
    assert_eq!(posts[2]["payload"]["body"], inline);
    assert_eq!(posts[2]["payload"]["line"], 6);
    terminal.key(b"C", "Diff side: LEFT or RIGHT:");
    terminal.field("RIGHT", "Displayed old/new line number:");
    terminal.field("99", "Inline comment body:");
    terminal.field("invalid location", "Line is not in the displayed diff");
    assert_eq!(fixture.posts().len(), 3);
    terminal.key(b"j", "binary.bin");
    terminal.expect("Patch unavailable");
    terminal.key(b"C", "Diff side: LEFT or RIGHT:");
    terminal.field("RIGHT", "Displayed old/new line number:");
    terminal.field("1", "Inline comment body:");
    terminal.field("binary location", "Line is not in the displayed diff");
    terminal.key(b"k", "review [a]\\nfile.rs");
    terminal.key(b"C", "Diff side: LEFT or RIGHT:");
    terminal.field("RIGHT", "Displayed old/new line number:");
    terminal.field("6", "Inline comment body:");
    let mut state = fixture.state();
    state["pr"]["headRefOid"] = serde_json::json!("b".repeat(40));
    fixture.write_state(&state);
    terminal.field("stale review", "PR head changed");
    assert_eq!(fixture.posts().len(), 3);
    terminal.view("PullRequests");
    terminal.key(b"M", "Confirm merge: enter PR number:");
    terminal.field("8", "Merge method: merge, squash, rebase:");
    terminal.field("rebase", "Merge cancelled");
    assert!(
        !fixture
            .calls()
            .iter()
            .any(|call| call["args"][0] == "pr" && call["args"][1] == "merge")
    );
    terminal.key(b"M", "Confirm merge: enter PR number:");
    terminal.field("7", "Merge method: merge, squash, rebase:");
    terminal.field("rebase", "Merge fixture complete");
    let calls = fixture.calls();
    let merged = calls
        .iter()
        .find(|call| call["args"][0] == "pr" && call["args"][1] == "merge")
        .unwrap();
    let args: Vec<_> = merged["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    assert!(
        args.contains(&"--rebase")
            && args.contains(&"--match-head-commit")
            && args.contains(&"b".repeat(40).as_str())
    );
    assert!(calls.iter().any(|call| {
        call["args"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "author:@me")
    }));
    assert!(!fixture.repo.path.join("untouched").exists());
    terminal.quit();
}

#[test]
fn draft_creation_checkout_repository_listing_and_local_clone_are_end_to_end() {
    let fixture = GitHubFixture::new();
    let mut terminal = fixture.terminal();
    terminal.view("PullRequests");
    terminal.key(b"N", "Draft PR title:");
    terminal.field("New draft fixture", "PR body:");
    terminal.field("Created body\nwith newline", "Base branch:");
    terminal.field("main", "Published head branch");
    terminal.field("topic", "Created PR fixture");
    assert_eq!(fixture.state()["pr"]["isDraft"], true);
    terminal.key(b"\r", "Checked out PR #8");
    assert_eq!(
        fixture.repo.git(&["branch", "--show-current"]).trim(),
        "topic"
    );
    assert_eq!(
        fixture.repo.git(&["rev-parse", "HEAD"]).trim(),
        fixture.head
    );
    terminal.view("GitHubRepositories");
    terminal.expect("fixture/repo");
    terminal.key(b"\r", "Clone destination (new directory):");
    let clone = fixture.repo.directory.0.join("github fixture clone");
    terminal.field(clone.to_str().unwrap(), "Opened repository");
    terminal.expect("github fixture clone");
    assert!(clone.join(".git").is_dir());
    assert_eq!(
        support::git(&clone, &["rev-parse", "HEAD"]).trim(),
        fixture.head
    );
    let calls = fixture.calls();
    let cloned = calls
        .iter()
        .find(|call| call["args"][0] == "repo" && call["args"][1] == "clone")
        .unwrap();
    assert_eq!(PathBuf::from(cloned["args"][3].as_str().unwrap()), clone);
    terminal.quit();
}
