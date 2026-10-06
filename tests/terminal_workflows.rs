#![cfg(unix)]
mod support;
use std::path::Path;
use support::{TestRepo, terminal::TerminalFixture};

fn tui(repo: &TestRepo) -> TerminalFixture {
    TerminalFixture::spawn(Path::new(env!("CARGO_BIN_EXE_git-wirdo")), &repo.path)
}

#[test]
fn staging_commits_amend_branch_switch_rename_and_delete_through_terminal() {
    let repo = TestRepo::new();
    repo.write("literal [a] ü.txt", "first\n");
    let mut terminal = tui(&repo);
    terminal.expect("literal [a] ü.txt");
    terminal.key(b"s", "Action: Staged");
    assert!(
        repo.git(&["diff", "--cached", "--name-only"])
            .contains("literal [a]")
    );
    terminal.key(b"u", "Action: Unstaged");
    assert!(repo.git(&["diff", "--cached", "--name-only"]).is_empty());
    terminal.key(b"s", "Action: Staged");
    terminal.key(b"c", "Commit message:");
    terminal.field(
        "terminal base\nmultiline body with bqcs",
        "Action: Committed changes",
    );
    assert_eq!(
        repo.git(&["log", "-1", "--format=%B"]).trim(),
        "terminal base\nmultiline body with bqcs"
    );
    terminal.key(b"E", "Amend commit message:");
    terminal.field("terminal amended", "Action: Committed changes");
    assert_eq!(repo.git(&["rev-list", "--count", "HEAD"]).trim(), "1");
    assert_eq!(
        repo.git(&["log", "-1", "--format=%s"]).trim(),
        "terminal amended"
    );
    terminal.key(b"b", "Create or switch branch:");
    terminal.field("topic", "Branch: topic");
    assert_eq!(repo.git(&["branch", "--show-current"]).trim(), "topic");
    terminal.view("Branches");
    // Sorted branches: main, topic. Select topic explicitly through the UI.
    terminal.key(b"j", "Current branch: topic");
    terminal.key(b"B", "New branch name:");
    terminal.field("renamed", "Branch: renamed");
    terminal.key(b"k", "Branch: main");
    terminal.key(b"\r", "Switched to main");
    terminal.key(b"j", "Branch: renamed");
    terminal.key(b"D", "Type delete to remove a merged branch:");
    terminal.field("delete", "Deleted merged branch");
    assert!(
        repo.fail(&["show-ref", "--verify", "refs/heads/renamed"])
            .stdout
            .is_empty()
    );
    terminal.quit();
}

#[test]
fn hunk_staging_unstaging_and_index_head_restore_through_terminal() {
    let repo = TestRepo::new();
    let base = (1..=30).map(|n| format!("line {n}\n")).collect::<String>();
    repo.write("file", &base);
    repo.commit_all("base");
    let modified = base
        .replace("line 2\n", "first changed\n")
        .replace("line 28\n", "second changed\n");
    repo.write("file", &modified);
    let mut terminal = tui(&repo);
    terminal.key(b"i", "Unstaged hunk 1/2");
    terminal.key(b"s", "Updated selected hunk in index");
    let staged = repo.git(&["show", ":file"]);
    assert!(staged.contains("first changed") && !staged.contains("second changed"));
    terminal.key(b"d", "Staged hunk 1/1");
    terminal.key(b"u", "Updated selected hunk in index");
    terminal.wait(|screen| screen.contains("No text hunks"));
    assert!(repo.git(&["diff", "--cached"]).is_empty());
    terminal.view("Files");
    terminal.key(b"s", "Action: Staged file");
    repo.write("file", "extra unstaged version\n");
    terminal.key(b"r", "Action: Refreshed");
    terminal.key(b"/", "Find: ");
    let screen = terminal.field(
        "extra unstaged version",
        "Action: Find: extra unstaged version",
    );
    assert!(screen.contains("+extra unstaged version"), "{screen}");
    terminal.key(b"w", "Type discard to confirm selected file:");
    terminal.field("discard", "Restored selected file");
    assert_eq!(
        std::fs::read_to_string(repo.path.join("file")).unwrap(),
        modified
    );
    assert_eq!(repo.git(&["show", ":file"]), modified);
    terminal.key(b"D", "Type discard to confirm selected file:");
    terminal.field("discard", "No changes");
    assert_eq!(
        std::fs::read_to_string(repo.path.join("file")).unwrap(),
        base
    );
    assert!(repo.git(&["status", "--porcelain"]).is_empty());
    terminal.resize(20, 100);
    terminal.expect("Help");
    terminal.quit();
}

#[test]
fn stash_save_apply_drop_and_pop_keep_index_and_untracked_files() {
    let repo = TestRepo::new();
    repo.write("tracked", "base\n");
    repo.commit_all("base");
    repo.write("tracked", "staged change\n");
    repo.git(&["add", "tracked"]);
    repo.write("untracked", "unsaved\n");
    let mut terminal = tui(&repo);
    terminal.key(b"S", "Stash message");
    terminal.field("terminal stash", "Saved stash including untracked files");
    assert!(repo.git(&["status", "--porcelain"]).is_empty());
    terminal.view("Stashes");
    terminal.expect("terminal stash");
    terminal.key(b"y", "Type apply to restore stash and index:");
    terminal.field("apply", "Applied stash; entry retained");
    assert_eq!(repo.git(&["show", ":tracked"]), "staged change\n");
    assert_eq!(
        std::fs::read_to_string(repo.path.join("untracked")).unwrap(),
        "unsaved\n"
    );
    terminal.key(b"D", "Type drop to delete selected stash:");
    terminal.field("drop", "Dropped selected stash");
    assert!(repo.git(&["stash", "list"]).is_empty());
    terminal.key(b"S", "Stash message");
    terminal.field("pop fixture", "Saved stash including untracked files");
    terminal.key(b"T", "Type pop to restore and remove on success:");
    terminal.field("pop", "Applied and removed stash");
    assert!(repo.git(&["stash", "list"]).is_empty());
    assert_eq!(repo.git(&["show", ":tracked"]), "staged change\n");
    assert!(repo.path.join("untracked").exists());
    terminal.quit();
}

fn divergent_merge() -> TestRepo {
    let repo = TestRepo::new();
    repo.write("conflict.txt", "base\n");
    repo.commit_all("base");
    repo.git(&["switch", "-c", "topic"]);
    repo.write("conflict.txt", "topic\n");
    repo.commit_all("topic edit");
    repo.git(&["switch", "main"]);
    repo.write("conflict.txt", "main\n");
    repo.commit_all("main edit");
    repo
}

#[test]
fn merge_start_conflict_side_continue_and_abort_use_real_terminal_controls() {
    for side in [
        Some(("t", "Theirs", "topic\n")),
        Some(("o", "Ours", "main\n")),
        None,
    ] {
        let repo = divergent_merge();
        let before = repo.git(&["rev-parse", "HEAD"]);
        let mut terminal = tui(&repo);
        terminal.view("Branches");
        terminal.key(b"j", "Branch: topic");
        terminal.key(b"m", "Type merge to merge into the current branch:");
        terminal.field("merge", "Merge in progress");
        assert!(repo.path.join(".git/MERGE_HEAD").exists());
        terminal.view("Conflicts");
        terminal.expect("conflict.txt");
        if let Some((key, name, expected)) = side {
            terminal.key(
                key.as_bytes(),
                &format!("Resolved conflict.txt with {name}"),
            );
            terminal.key(b"e", "Continued Git operation");
            assert_eq!(
                std::fs::read_to_string(repo.path.join("conflict.txt")).unwrap(),
                expected
            );
            assert_eq!(
                repo.git(&["rev-list", "--parents", "-n", "1", "HEAD"])
                    .split_whitespace()
                    .count(),
                3
            );
        } else {
            terminal.key(b"x", "Aborted Git operation");
            assert_eq!(repo.git(&["rev-parse", "HEAD"]), before);
            assert_eq!(
                std::fs::read_to_string(repo.path.join("conflict.txt")).unwrap(),
                "main\n"
            );
        }
        assert!(!repo.path.join(".git/MERGE_HEAD").exists());
        assert!(repo.git(&["status", "--porcelain"]).is_empty());
        terminal.quit();
    }
}

#[test]
fn cherry_pick_and_revert_conflicts_can_be_manually_resolved_or_aborted_in_terminal() {
    for revert in [false, true] {
        for continue_operation in [false, true] {
            let repo = TestRepo::new();
            repo.write("file", "base\n");
            repo.commit_all("base");
            if !revert {
                repo.git(&["switch", "-c", "topic"]);
            }
            repo.write("file", "selected change\n");
            repo.commit_all("selected operation commit");
            let selected = repo.git(&["rev-parse", "HEAD"]).trim().to_owned();
            if !revert {
                repo.git(&["switch", "main"]);
            }
            repo.write("file", "current change\n");
            repo.commit_all("current change");
            let before = repo.git(&["rev-parse", "HEAD"]);
            let mut terminal = tui(&repo);
            terminal.view("History");
            terminal.key(b"/", "Find: ");
            terminal.field("selected operation", "History match: selected operation");
            terminal.key(
                if revert { b"Z" } else { b"Y" },
                "Type full selected commit ID to confirm:",
            );
            terminal.field(&selected, "Mainline parent for merge commit");
            terminal.field(
                "",
                if revert {
                    "Revert in progress"
                } else {
                    "Cherry-pick in progress"
                },
            );
            terminal.view("Conflicts");
            if continue_operation {
                repo.write("file", "manually resolved\n");
                terminal.key(b"a", "Marked file as resolved");
                terminal.key(b"e", "Continued Git operation");
                assert_eq!(
                    std::fs::read_to_string(repo.path.join("file")).unwrap(),
                    "manually resolved\n"
                );
                assert_ne!(repo.git(&["rev-parse", "HEAD"]), before);
            } else {
                terminal.key(b"x", "Aborted Git operation");
                assert_eq!(repo.git(&["rev-parse", "HEAD"]), before);
                assert_eq!(
                    std::fs::read_to_string(repo.path.join("file")).unwrap(),
                    "current change\n"
                );
            }
            assert!(!repo.path.join(".git/CHERRY_PICK_HEAD").exists());
            assert!(!repo.path.join(".git/REVERT_HEAD").exists());
            assert!(repo.git(&["status", "--porcelain"]).is_empty());
            terminal.quit();
        }
    }
}

fn divergent_rebase() -> TestRepo {
    let repo = TestRepo::new();
    repo.write("first.txt", "base\n");
    repo.write("second.txt", "base\n");
    repo.commit_all("base");
    repo.git(&["switch", "-c", "topic"]);
    repo.write("first.txt", "topic first\n");
    repo.commit_all("first topic");
    repo.write("second.txt", "topic second\n");
    repo.commit_all("second topic");
    repo.git(&["switch", "main"]);
    repo.write("first.txt", "main first\n");
    repo.write("second.txt", "main second\n");
    repo.commit_all("main changes");
    repo.git(&["switch", "topic"]);
    repo
}

#[test]
fn rebase_start_multiple_conflicts_continue_skip_and_abort_use_terminal_controls() {
    for mode in ["continue", "skip", "abort"] {
        let repo = divergent_rebase();
        let before = repo.git(&["rev-parse", "HEAD"]);
        let mut terminal = tui(&repo);
        terminal.view("Branches");
        terminal.key(
            b"z",
            "Type rebase to rebase the current branch onto selection:",
        );
        terminal.field("rebase", "Rebase in progress");
        terminal.view("Conflicts");
        terminal.expect("first.txt");
        if mode == "abort" {
            terminal.key(b"x", "Aborted Git operation");
            assert_eq!(repo.git(&["rev-parse", "HEAD"]), before);
        } else {
            if mode == "skip" {
                terminal.key(b"K", "second.txt");
            } else {
                terminal.key(b"t", "Resolved first.txt with Theirs");
                terminal.key(b"e", "second.txt");
            }
            terminal.key(b"t", "Resolved second.txt with Theirs");
            terminal.key(b"e", "Continued Git operation");
            assert_eq!(
                std::fs::read_to_string(repo.path.join("first.txt")).unwrap(),
                if mode == "skip" {
                    "main first\n"
                } else {
                    "topic first\n"
                }
            );
            assert_eq!(
                std::fs::read_to_string(repo.path.join("second.txt")).unwrap(),
                "topic second\n"
            );
        }
        assert_eq!(repo.git(&["branch", "--show-current"]).trim(), "topic");
        assert!(!repo.path.join(".git/rebase-merge").exists());
        assert!(repo.git(&["status", "--porcelain"]).is_empty());
        terminal.quit();
    }
}

#[test]
fn remote_publish_fetch_pull_push_upstream_review_and_tag_lifecycle_are_end_to_end() {
    let repo = TestRepo::new();
    repo.write("base", "base\n");
    repo.commit_all("base");
    let remote = repo.directory.0.join("remote with spaces.git");
    std::fs::create_dir(&remote).unwrap();
    support::git(&remote, &["init", "--bare", "--initial-branch=main"]);
    let url = remote.to_str().unwrap();
    let mut terminal = tui(&repo);
    terminal.view("Remotes");
    terminal.key(b"N", "Remote name:");
    terminal.field("origin", "Remote URL:");
    terminal.field(url, "Added remote");
    terminal.key(b"W", "Remote to publish current branch:");
    terminal.field("origin", "Published current branch and set upstream");
    let writer = TestRepo::new();
    writer.git(&["remote", "add", "origin", url]);
    writer.git(&["fetch", "origin"]);
    writer.git(&["reset", "--hard", "origin/main"]);
    writer.write("remote-added", "from another checkout\n");
    writer.commit_all("remote addition");
    writer.git(&["push", "origin", "main"]);
    terminal.key(b"f", "Fetched selected remote");
    terminal.expect("origin/main +0 -1");
    terminal.key(b"l", "Pull strategy: ff-only, merge or rebase:");
    terminal.field("ff-only", "Pulled changes");
    assert_eq!(
        repo.git(&["rev-parse", "HEAD"]),
        writer.git(&["rev-parse", "HEAD"])
    );
    terminal.key(b"L", "New fetch URL:");
    terminal.field(url, "Updated fetch URL");
    terminal.key(b"H", "New push URL:");
    terminal.field(url, "Updated push URL");
    terminal.key(b"U", "Upstream ref");
    terminal.field("origin/main", "Updated upstream");
    terminal.view("Files");
    repo.write("local-added", "terminal publication\n");
    terminal.key(b"r", "local-added");
    terminal.key(b"s", "Staged local-added");
    terminal.key(b"c", "Commit message:");
    terminal.field("local publication", "Committed changes");
    terminal.key(b"d", "Compared with upstream merge base");
    terminal.expect("local-added");
    terminal.key(b"v", "Seen 1/1");
    terminal.key(b"d", "Working changes");
    terminal.key(b"P", "Pushed changes");
    assert_eq!(
        support::git(&remote, &["rev-parse", "main"]),
        repo.git(&["rev-parse", "HEAD"])
    );
    terminal.view("Tags");
    terminal.key(b"N", "Tag name:");
    terminal.field("terminal-v1", "Target commit");
    terminal.field("", "Annotation");
    terminal.field("terminal annotation", "Created tag");
    assert_eq!(
        repo.git(&["cat-file", "-t", "refs/tags/terminal-v1"])
            .trim(),
        "tag"
    );
    terminal.key(b"W", "Remote to publish selected tag:");
    terminal.field("origin", "Published selected tag");
    assert_eq!(
        support::git(&remote, &["rev-parse", "refs/tags/terminal-v1"]),
        repo.git(&["rev-parse", "refs/tags/terminal-v1"])
    );
    terminal.key(b"X", "Remote to delete selected tag from:");
    terminal.field("origin", "Type delete to confirm remote deletion:");
    terminal.field("delete", "Deleted remote tag; local tag retained");
    assert!(
        !support::git_output(&remote, &["show-ref", "--verify", "refs/tags/terminal-v1"])
            .status
            .success()
    );
    assert!(
        repo.git(&["show-ref", "--verify", "refs/tags/terminal-v1"])
            .contains("terminal-v1")
    );
    terminal.key(b"D", "Type delete to remove local tag:");
    terminal.field("delete", "Deleted selected local tag");
    terminal.view("Remotes");
    terminal.key(b"D", "Type remove to remove remote and tracking refs:");
    terminal.field("remove", "Removed remote");
    assert!(repo.git(&["remote"]).is_empty());
    terminal.quit();
}

#[test]
fn history_graph_metadata_search_cherry_pick_and_revert_are_end_to_end() {
    let repo = TestRepo::new();
    repo.write("base", "base\n");
    repo.commit_all("base");
    repo.git(&["switch", "-c", "topic"]);
    repo.write("topic", "topic change\n");
    repo.commit_all("topic useful change");
    let sha = repo.git(&["rev-parse", "HEAD"]).trim().to_owned();
    repo.git(&["switch", "main"]);
    let mut terminal = tui(&repo);
    terminal.view("History");
    terminal.key(b"+", "Loaded 2 commits across branches");
    terminal.key(b"/", "Find: ");
    terminal.field("topic useful", "History match: topic useful");
    terminal.key(b"g", "HEAD -> main");
    let patch = terminal.key(b"g", "diff --git");
    assert!(
        patch.contains("Author:")
            && patch.contains("CommitDate:")
            && patch.contains("+topic change")
    );
    terminal.key(b"Y", "Type full selected commit ID to confirm:");
    terminal.field(&sha, "Mainline parent for merge commit");
    terminal.field("", "Cherry-picked commit");
    let picked = repo.git(&["rev-parse", "HEAD"]).trim().to_owned();
    assert_eq!(
        std::fs::read_to_string(repo.path.join("topic")).unwrap(),
        "topic change\n"
    );
    terminal.key(b"/", "Find: ");
    terminal.field("topic useful", "History match: topic useful");
    terminal.key(b"Z", "Type full selected commit ID to confirm:");
    terminal.field(&picked, "Mainline parent for merge commit");
    terminal.field("", "Reverted commit");
    assert!(!repo.path.join("topic").exists());
    assert!(
        repo.git(&["log", "-1", "--format=%s"])
            .starts_with("Revert")
    );
    assert!(repo.git(&["status", "--porcelain"]).is_empty());
    terminal.quit();
}

#[test]
fn reset_modes_and_reflog_recovery_keep_untracked_work_and_previous_commits() {
    for mode in ["soft", "mixed", "hard"] {
        let repo = TestRepo::new();
        repo.write("file", "base\n");
        repo.commit_all("base target");
        let base = repo.git(&["rev-parse", "HEAD"]).trim().to_owned();
        repo.write("file", "second\n");
        repo.commit_all("second commit");
        let head = repo.git(&["rev-parse", "HEAD"]).trim().to_owned();
        repo.write("file", "staged\n");
        repo.git(&["add", "file"]);
        repo.write("file", "working\n");
        repo.write("untracked", "retain\n");
        let mut terminal = tui(&repo);
        terminal.view("History");
        terminal.key(b"/", "Find: ");
        terminal.field("base target", "History match: base target");
        terminal.key(b"F", "Reset mode: soft, mixed, hard:");
        terminal.field(mode, "Type full target commit ID to confirm:");
        terminal.field(&base, &format!("Reset {mode} to"));
        assert_eq!(repo.git(&["rev-parse", "HEAD"]).trim(), base);
        assert_eq!(
            repo.git(&["show", ":file"]),
            if mode == "soft" { "staged\n" } else { "base\n" }
        );
        assert_eq!(
            std::fs::read_to_string(repo.path.join("file")).unwrap(),
            if mode == "hard" {
                "base\n"
            } else {
                "working\n"
            }
        );
        assert_eq!(
            std::fs::read_to_string(repo.path.join("untracked")).unwrap(),
            "retain\n"
        );
        assert_eq!(
            repo.git(&[
                "for-each-ref",
                "--format=%(objectname)",
                "refs/git-wirdo/recovery/"
            ])
            .trim(),
            head
        );
        terminal.view("Reflog");
        // HEAD@{0} is the reset target; HEAD@{1} is the previous HEAD.
        terminal.key(b"j", "second commit");
        terminal.key(b"N", "New recovery branch name");
        terminal.field("recovered", "Recovered");
        assert_eq!(repo.git(&["rev-parse", "recovered"]).trim(), head);
        assert_eq!(repo.git(&["rev-parse", "HEAD"]).trim(), base);
        assert_eq!(repo.git(&["branch", "--show-current"]).trim(), "main");
        terminal.quit();
    }
}

#[test]
fn linked_worktree_creation_open_recent_return_and_guarded_removal_are_end_to_end() {
    let repo = TestRepo::new();
    repo.write("file", "base\n");
    repo.commit_all("base");
    let workspace = repo.directory.0.join("linked terminal workspace");
    let mut terminal = tui(&repo);
    terminal.view("Workspaces");
    terminal.key(b"N", "Workspace path");
    terminal.field(workspace.to_str().unwrap(), "New branch name:");
    terminal.field("topic", "Workspace created");
    assert!(workspace.join(".git").is_file());
    // worktree list returns the primary checkout followed by its linked tree.
    terminal.key(b"j", "Branch: topic");
    terminal.key(b"\r", "Branch: topic");
    terminal.wait(|screen| {
        screen.contains("Repo:")
            && screen.contains("linked terminal workspace")
            && screen.contains("View: Files")
    });
    std::fs::write(workspace.join("unsaved"), "retain linked work\n").unwrap();
    terminal.key(b"r", "unsaved");
    terminal.key(b"I", "View: RecentRepositories");
    terminal.key(b"j", "Saved view: Workspaces");
    terminal.key(b"\r", "View: Workspaces");
    terminal.key(b"j", "Branch: topic");
    terminal.key(b"D", "Type remove to confirm:");
    terminal.field("remove", "contains modified");
    assert!(workspace.join("unsaved").exists());
    std::fs::remove_file(workspace.join("unsaved")).unwrap();
    terminal.key(b"r", "Action: Refreshed");
    terminal.key(b"D", "Type remove to confirm:");
    terminal.field("remove", "Workspace removed; branch retained");
    assert!(!workspace.exists());
    assert!(
        repo.git(&["show-ref", "--verify", "refs/heads/topic"])
            .contains("topic")
    );
    terminal.quit();
}

#[test]
fn remote_tracking_checkout_is_created_through_the_terminal() {
    let writer = TestRepo::new();
    writer.write("remote", "content\n");
    writer.commit_all("published base");
    let remote = writer.directory.0.join("bare.git");
    std::fs::create_dir(&remote).unwrap();
    support::git(&remote, &["init", "--bare", "--initial-branch=main"]);
    writer.git(&["push", remote.to_str().unwrap(), "main"]);
    let reader = TestRepo::new();
    reader.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
    let mut terminal = tui(&reader);
    terminal.key(b"f", "Fetched remotes");
    terminal.view("RemoteBranches");
    terminal.expect("origin/main");
    terminal.key(b"\r", "New local tracking branch name:");
    terminal.field("tracking", "Checked out tracking branch");
    assert_eq!(reader.git(&["branch", "--show-current"]).trim(), "tracking");
    assert_eq!(
        reader
            .git(&["rev-parse", "--abbrev-ref", "@{upstream}"])
            .trim(),
        "origin/main"
    );
    assert_eq!(
        std::fs::read_to_string(reader.path.join("remote")).unwrap(),
        "content\n"
    );
    terminal.quit();
}
