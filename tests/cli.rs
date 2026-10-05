mod support;

use std::process::Command;

use support::{TempDirectory, TestRepo};

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_git-wirdo"))
}

#[test]
fn help_documents_repository_and_headless_options() {
    let output = binary().arg("--help").output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("--repo"));
    assert!(text.contains("--headless"));
}

#[test]
fn headless_mode_works_without_a_tty_in_an_unborn_repository() {
    let fixture = TestRepo::new();
    fixture.write("nested/new.txt", "hello\n");
    let output = binary()
        .current_dir(fixture.path.join("nested"))
        .arg("--headless")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("branch: main"));
    assert!(text.contains("?? false true nested/new.txt"));
    assert!(!text.contains('\x1b'));
}

#[test]
fn invalid_repository_errors_do_not_enter_raw_mode() {
    let directory = TempDirectory::new();
    let output = binary().arg("--repo").arg(&directory.0).output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("Cannot open repository"));
    assert!(!error.contains('\x1b'));
}

#[test]
fn interactive_mode_without_a_terminal_suggests_headless_mode() {
    let fixture = TestRepo::new();
    let output = binary().arg("--repo").arg(&fixture.path).output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("--headless"));
    assert!(!error.contains('\x1b'));
}

#[test]
fn explicit_repository_wins_over_inherited_git_environment() {
    let target = TestRepo::new();
    let other = TestRepo::new();
    target.write("target.txt", "target\n");
    other.write("other.txt", "other\n");
    let output = binary()
        .args(["--headless", "--repo"])
        .arg(&target.path)
        .env("GIT_DIR", other.path.join(".git"))
        .env("GIT_WORK_TREE", &other.path)
        .env("GIT_INDEX_FILE", other.path.join(".git/index"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("target.txt"));
    assert!(!text.contains("other.txt"));
}

#[test]
fn workspace_listing_is_read_only_and_noninteractive() {
    let fixture = TestRepo::new();
    fixture.write("file", "base");
    fixture.commit_all("base");
    let workspace = fixture.directory.0.join("another checkout");
    fixture
        .open()
        .create_workspace(&workspace, "topic")
        .unwrap();
    let before = fixture.git(&["status", "--porcelain"]);
    let output = binary()
        .arg("--repo")
        .arg(&fixture.path)
        .arg("--list-workspaces")
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("topic") && text.contains("another checkout"));
    assert!(!text.contains('\x1b'));
    assert_eq!(fixture.git(&["status", "--porcelain"]), before);
}

#[test]
fn stash_listing_does_not_apply_or_remove_entries() {
    let fixture = TestRepo::new();
    fixture.write("file", "base");
    fixture.commit_all("base");
    fixture.write("untracked", "saved");
    fixture.open().save_stash("CLI stash").unwrap();
    let output = binary()
        .arg("--repo")
        .arg(&fixture.path)
        .arg("--list-stashes")
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("stash@{0}") && text.contains("CLI stash"));
    assert_eq!(fixture.open().stashes().unwrap().len(), 1);
    assert!(!fixture.path.join("untracked").exists());
}

#[test]
fn tag_listing_is_noninteractive_and_keeps_refs() {
    let fixture = TestRepo::new();
    fixture.write("file", "base");
    fixture.commit_all("base");
    fixture.open().create_tag("v1", "HEAD", "").unwrap();
    let before = fixture.git(&["rev-parse", "refs/tags/v1"]);
    let output = binary()
        .arg("--repo")
        .arg(&fixture.path)
        .arg("--list-tags")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8(output.stdout).unwrap().contains("v1"));
    assert_eq!(fixture.git(&["rev-parse", "refs/tags/v1"]), before);
}
