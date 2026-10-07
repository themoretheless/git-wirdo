mod support;
use git_wirdo::{
    app::{Action, App, FileDetail},
    git::Repository,
};
use std::{path::Path, process::Command};
use support::TestRepo;

fn fixture() -> TestRepo {
    let repo = TestRepo::new();
    repo.write("old [a] ü.txt", "original line\nsecond line\n");
    repo.commit_all("original creation");
    repo.git(&["mv", "old [a] ü.txt", "new [a] ü.txt"]);
    repo.commit_all("rename only");
    repo.write("new [a] ü.txt", "original line\nchanged line\n");
    repo.commit_all("changed second line");
    repo.write("new [a] ü.txt", "pending edit\nchanged line\n");
    repo
}

#[test]
fn history_follows_literal_renames_and_limits_commits_without_changing_pending_work() {
    let repo = fixture();
    repo.write("new a ü.txt", "unrelated");
    repo.git(&["add", "new a ü.txt"]);
    let before = repo.git(&["diff", "--cached"]);
    let history = repo
        .open()
        .file_history(Path::new("new [a] ü.txt"), 100)
        .unwrap();
    assert!(
        history.contains("original creation")
            && history.contains("rename only")
            && history.contains("changed second line")
    );
    assert!(!history.contains("pending edit"));
    let short = repo
        .open()
        .file_history(Path::new("new [a] ü.txt"), 1)
        .unwrap();
    assert!(short.contains("changed second line") && !short.contains("original creation"));
    assert_eq!(repo.git(&["diff", "--cached"]), before);
    assert_eq!(
        std::fs::read_to_string(repo.path.join("new [a] ü.txt")).unwrap(),
        "pending edit\nchanged line\n"
    );
}

#[test]
fn blame_reads_head_authorship_and_tui_toggles_back_to_diff() {
    let repo = fixture();
    let original = repo.git(&["rev-list", "--max-parents=0", "HEAD"]);
    let changed = repo.git(&["rev-parse", "HEAD"]);
    let blame = repo.open().file_blame(Path::new("new [a] ü.txt")).unwrap();
    assert!(blame.contains(&original[..12]) && blame.contains(&changed[..12]));
    assert!(blame.contains("original line") && !blame.contains("pending edit"));
    let mut app = App::new(repo.open()).unwrap();
    app.handle(Action::FileHistory);
    assert_eq!(app.file_detail, FileDetail::History);
    assert!(app.detail_text.contains("original creation"));
    app.handle(Action::FileBlame);
    assert!(
        app.detail_text.contains("Line authorship at HEAD")
            && app.detail_text.contains("original line")
    );
    app.handle(Action::FileBlame);
    assert_eq!(app.file_detail, FileDetail::Diff);
    assert!(app.detail_text.contains("pending edit"));
}

#[test]
fn cli_inspects_clean_files_and_rejects_traversal_and_conflicting_modes() {
    let repo = fixture();
    repo.git(&["restore", "new [a] ü.txt"]);
    let invoke = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_git-wirdo"))
            .arg("--repo")
            .arg(&repo.path)
            .args(args)
            .output()
            .unwrap()
    };
    let history = invoke(&[
        "--no-state",
        "--file-history",
        "new [a] ü.txt",
        "--file-limit",
        "1",
    ]);
    assert!(
        history.status.success(),
        "{}",
        String::from_utf8_lossy(&history.stderr)
    );
    assert!(String::from_utf8_lossy(&history.stdout).contains("changed second line"));
    let blame = invoke(&["--no-state", "--blame", "new [a] ü.txt"]);
    assert!(blame.status.success());
    assert!(String::from_utf8_lossy(&blame.stdout).contains("original line"));
    assert!(!invoke(&["--blame", "../outside"]).status.success());
    assert!(
        !invoke(&["--blame", "new [a] ü.txt", "--list-tags"])
            .status
            .success()
    );
    assert!(
        !invoke(&["--file-history", "new [a] ü.txt", "--file-limit", "0"])
            .status
            .success()
    );
    assert!(
        Repository::open(&repo.path)
            .unwrap()
            .file_blame(Path::new("../outside"))
            .is_err()
    );
}

#[test]
fn staged_rename_inspects_original_head_path_and_deleted_history_remains_readable() {
    let repo = fixture();
    repo.git(&["restore", "new [a] ü.txt"]);
    repo.git(&["mv", "new [a] ü.txt", "pending [a] ü.txt"]);
    let index = repo.git(&["diff", "--cached"]);
    let mut app = App::new(repo.open()).unwrap();
    app.handle(Action::FileBlame);
    assert!(
        app.detail_text.contains("original line"),
        "{}",
        app.detail_text
    );
    app.handle(Action::Unstage);
    assert_eq!(repo.git(&["diff", "--cached"]), index);
    repo.commit_all("pending rename committed");
    repo.git(&["rm", "pending [a] ü.txt"]);
    repo.commit_all("deleted file");
    let history = repo
        .open()
        .file_history(Path::new("pending [a] ü.txt"), 100)
        .unwrap();
    assert!(history.contains("deleted file") && history.contains("original creation"));
    assert!(!repo.path.join("pending [a] ü.txt").exists());
}
