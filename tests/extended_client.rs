mod support;
use git_wirdo::app::{Action, App};
use std::{path::Path, process::Command};
use support::TestRepo;

#[test]
fn tracked_browser_reads_clean_files_preserves_pending_work_and_uses_original_rename_path() {
    let repo = TestRepo::new();
    repo.write(
        "clean [a] ü.txt",
        "committed content\n[UNTRACKED] literal\n",
    );
    repo.commit_all("base");
    let mut app = App::new(repo.open()).unwrap();
    assert!(app.displayed_files().is_empty());
    app.handle(Action::BrowseTracked);
    assert!(app.browse_tracked);
    assert_eq!(app.displayed_files()[0].path, Path::new("clean [a] ü.txt"));
    assert!(
        app.detail_text
            .contains("committed content\n[UNTRACKED] literal")
    );
    app.handle(Action::FileHistory);
    assert!(app.detail_text.contains("base"));
    app.handle(Action::FileBlame);
    assert!(app.detail_text.contains("committed content"));
    repo.git(&["mv", "clean [a] ü.txt", "renamed [a] ü.txt"]);
    repo.write("renamed [a] ü.txt", "pending edit");
    let index = repo.git(&["diff", "--cached"]);
    app.handle(Action::Refresh);
    assert!(app.detail_text.contains("committed content"));
    app.handle(Action::Stage);
    app.handle(Action::Unstage);
    app.handle(Action::Remove);
    app.handle(Action::OpenHunks);
    assert_eq!(repo.git(&["diff", "--cached"]), index);
    assert!(app.prompt.is_none());
    assert_eq!(
        std::fs::read_to_string(repo.path.join("renamed [a] ü.txt")).unwrap(),
        "pending edit"
    );
    app.handle(Action::BrowseTracked);
    assert!(!app.browse_tracked && app.detail_text.contains("pending edit"));
}

#[test]
fn exported_patch_applies_rename_binary_changes_and_message_in_an_independent_repository() {
    let source = TestRepo::new();
    let target = TestRepo::new();
    for repo in [&source, &target] {
        repo.write("old [a] ü.txt", "base\n");
        repo.commit_all("base");
    }
    source.git(&["mv", "old [a] ü.txt", "new [a] ü.txt"]);
    std::fs::write(source.path.join("binary.bin"), [0, 255, 1, 2, 0]).unwrap();
    source.commit_all("portable commit\n\nmultiline ü body");
    source.write("new [a] ü.txt", "pending source");
    source.write("unrelated", "staged content");
    source.git(&["add", "unrelated"]);
    let index = source.git(&["diff", "--cached"]);
    let head = source.git(&["rev-parse", "HEAD"]);
    let patch = source.directory.0.join("commit ü.patch");
    source.open().export_commit("HEAD", &patch).unwrap();
    target.git(&["am", patch.to_str().unwrap()]);
    assert!(!target.path.join("old [a] ü.txt").exists());
    assert_eq!(
        std::fs::read_to_string(target.path.join("new [a] ü.txt")).unwrap(),
        "base\n"
    );
    assert_eq!(
        std::fs::read(target.path.join("binary.bin")).unwrap(),
        [0, 255, 1, 2, 0]
    );
    assert!(
        target
            .git(&["log", "-1", "--format=%B"])
            .contains("multiline ü body")
    );
    assert_eq!(source.git(&["diff", "--cached"]), index);
    assert_eq!(source.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(
        std::fs::read_to_string(source.path.join("new [a] ü.txt")).unwrap(),
        "pending source"
    );
    let bytes = std::fs::read(&patch).unwrap();
    assert!(source.open().export_commit("HEAD", &patch).is_err());
    assert_eq!(std::fs::read(&patch).unwrap(), bytes);
}

#[test]
fn root_patch_and_cli_export_work_without_overwriting_and_merge_export_is_refused() {
    let repo = TestRepo::new();
    repo.write("file", "initial");
    repo.commit_all("root commit");
    let patch = repo.directory.0.join("root.patch");
    let invoke = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_git-wirdo"))
            .arg("--repo")
            .arg(&repo.path)
            .args(args)
            .output()
            .unwrap()
    };
    let exported = invoke(&[
        "--export-patch",
        "HEAD",
        "--output",
        patch.to_str().unwrap(),
    ]);
    assert!(
        exported.status.success(),
        "{}",
        String::from_utf8_lossy(&exported.stderr)
    );
    let target = TestRepo::new();
    target.git(&["am", patch.to_str().unwrap()]);
    assert_eq!(
        std::fs::read_to_string(target.path.join("file")).unwrap(),
        "initial"
    );
    assert!(
        !invoke(&[
            "--export-patch",
            "HEAD",
            "--output",
            patch.to_str().unwrap()
        ])
        .status
        .success()
    );
    assert!(String::from_utf8_lossy(&invoke(&["--list-files"]).stdout).contains("file"));
    repo.git(&["switch", "-c", "topic"]);
    repo.write("topic", "topic");
    repo.commit_all("topic");
    repo.git(&["switch", "main"]);
    repo.write("main", "main");
    repo.commit_all("main");
    repo.git(&["merge", "--no-ff", "topic", "-m", "merge"]);
    let refused = repo.directory.0.join("merge.patch");
    assert!(repo.open().export_commit("HEAD", &refused).is_err());
    assert!(!refused.exists());
}
