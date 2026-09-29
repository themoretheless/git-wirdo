mod support;

use std::fs;
use std::path::Path;

use git_wirdo::git::{ConflictSide, Repository};
use support::{TempDirectory, TestRepo, git, git_output, init, merge_conflict, rebase_conflict};

#[test]
fn unborn_repository_can_stage_and_unstage_without_removing_working_files() {
    let fixture = TestRepo::new();
    fixture.write("new file.txt", "hello\n");
    let repo = fixture.open();
    let state = repo.load_state().unwrap();
    assert_eq!(state.branch, "main");
    assert!(state.commits.is_empty());
    assert!(state.files[0].unstaged && !state.files[0].staged);
    repo.stage(&state.files[0]).unwrap();
    let staged = repo.load_state().unwrap();
    assert_eq!(staged.files[0].status, "A ");
    repo.unstage(&staged.files[0]).unwrap();
    assert_eq!(repo.load_state().unwrap().files[0].status, "??");
    assert_eq!(
        fs::read_to_string(fixture.path.join("new file.txt")).unwrap(),
        "hello\n"
    );
}

#[test]
fn first_unstaged_record_keeps_both_status_columns() {
    let fixture = TestRepo::new();
    fixture.write("file", "before\n");
    fixture.commit_all("initial");
    fixture.write("file", "after\n");
    let repo = fixture.open();
    let state = repo.load_state().unwrap();
    assert_eq!(state.files[0].status, " M");
    assert!(!state.files[0].staged);
    assert!(state.files[0].unstaged);
    repo.stage(&state.files[0]).unwrap();
    repo.commit("an explicit commit message").unwrap();
    let state = repo.load_state().unwrap();
    assert_eq!(state.summary(), "clean");
    assert_eq!(state.commits.len(), 2);
    assert_eq!(state.commits[0].subject, "an explicit commit message");
}

#[test]
fn stage_and_unstage_use_literal_paths_including_pathspec_magic() {
    let fixture = TestRepo::new();
    fixture.write("[abc].txt", "literal\n");
    fixture.write("a.txt", "must remain untracked\n");
    fixture.write("-option.txt", "option-like\n");
    fixture.write(" leading space.txt", "space\n");
    fixture.write("данные.txt", "unicode\n");
    let repo = fixture.open();
    for name in [
        "[abc].txt",
        "-option.txt",
        " leading space.txt",
        "данные.txt",
    ] {
        let state = repo.load_state().unwrap();
        let file = state
            .files
            .iter()
            .find(|file| file.path == Path::new(name))
            .unwrap();
        repo.stage(file).unwrap();
        let state = repo.load_state().unwrap();
        assert_eq!(state.files.iter().filter(|file| file.staged).count(), 1);
        assert_eq!(
            state
                .files
                .iter()
                .find(|file| file.path == Path::new("a.txt"))
                .unwrap()
                .status,
            "??"
        );
        repo.unstage(state.files.iter().find(|file| file.staged).unwrap())
            .unwrap();
    }
}

#[cfg(unix)]
#[test]
fn arbitrary_filename_bytes_are_preserved_for_git_operations() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::path::PathBuf;

    let fixture = TestRepo::new();
    let names = [
        PathBuf::from("line\nbreak\t\".txt"),
        PathBuf::from("trailing space "),
        PathBuf::from("*.txt"),
        PathBuf::from(":(glob)*"),
        PathBuf::from(OsString::from_vec(b"non-utf8-\xff.txt".to_vec())),
    ];
    for name in &names {
        fixture.write(name, "content\n");
    }
    let repo = fixture.open();
    for name in &names {
        let state = repo.load_state().unwrap();
        let file = state.files.iter().find(|file| file.path == *name).unwrap();
        repo.stage(file).unwrap();
        let state = repo.load_state().unwrap();
        assert_eq!(state.files.iter().filter(|file| file.staged).count(), 1);
        repo.unstage(state.files.iter().find(|file| file.staged).unwrap())
            .unwrap();
        assert!(fixture.path.join(name).exists());
    }
}

#[test]
fn renames_can_be_staged_again_and_unstaged_as_one_change() {
    let fixture = TestRepo::new();
    fixture.write("old name.txt", "first\nsecond\nthird\n");
    fixture.commit_all("initial");
    fixture.git(&["mv", "old name.txt", "new name.txt"]);
    fixture.write("new name.txt", "first\nsecond\nthird\nfourth\n");
    let repo = fixture.open();
    let state = repo.load_state().unwrap();
    let rename = &state.files[0];
    assert_eq!(rename.path, Path::new("new name.txt"));
    assert_eq!(
        rename.original_path.as_deref(),
        Some(Path::new("old name.txt"))
    );
    assert!(rename.staged && rename.unstaged);
    assert!(
        repo.file_detail(rename)
            .unwrap()
            .contains("rename from old name.txt")
    );
    repo.stage(rename).unwrap();
    let state = repo.load_state().unwrap();
    assert!(state.files[0].staged && !state.files[0].unstaged);
    repo.unstage(&state.files[0]).unwrap();
    assert!(fixture.git(&["diff", "--cached", "--name-only"]).is_empty());
    assert!(fixture.path.join("new name.txt").exists());
    assert!(!fixture.path.join("old name.txt").exists());
}

#[test]
fn opening_a_subdirectory_normalizes_operations_to_the_repository_root() {
    let fixture = TestRepo::new();
    fixture.write("nested/file.txt", "nested\n");
    let repo = Repository::open(&fixture.path.join("nested")).unwrap();
    assert_eq!(
        fs::canonicalize(repo.root()).unwrap(),
        fs::canonicalize(&fixture.path).unwrap()
    );
    let file = repo.load_state().unwrap().files.remove(0);
    assert_eq!(file.path, Path::new("nested/file.txt"));
    repo.stage(&file).unwrap();
    assert!(repo.load_state().unwrap().files[0].staged);
}

#[cfg(unix)]
#[test]
fn repository_paths_are_not_trimmed_or_split_at_newlines() {
    let directory = TempDirectory::new();
    let path = directory.0.join("repo with trailing newline\n");
    init(&path);
    let repo = Repository::open(&path).unwrap();
    assert_eq!(
        fs::canonicalize(repo.root()).unwrap(),
        fs::canonicalize(&path).unwrap()
    );
    assert_eq!(repo.load_state().unwrap().branch, "main");
}

#[test]
fn branch_listing_excludes_detached_head_pseudo_branches() {
    let fixture = TestRepo::new();
    fixture.write("file", "base\n");
    fixture.commit_all("base");
    let repo = fixture.open();
    repo.switch_or_create_branch("feature/test").unwrap();
    repo.switch_branch("main").unwrap();
    repo.switch_or_create_branch("feature/test").unwrap();
    assert_eq!(repo.load_state().unwrap().branch, "feature/test");
    fixture.git(&["switch", "--detach", "HEAD"]);
    let state = repo.load_state().unwrap();
    assert!(state.branch.starts_with("HEAD ("));
    assert_eq!(state.branches.len(), 2);
    assert!(state.branches.iter().all(|branch| !branch.current));
    assert!(
        state
            .branches
            .iter()
            .all(|branch| !branch.name.contains("HEAD"))
    );
}

#[test]
fn failed_branch_switch_does_not_create_or_overwrite_a_branch() {
    let fixture = TestRepo::new();
    fixture.write("file", "base\n");
    fixture.commit_all("base");
    fixture.git(&["switch", "-c", "topic"]);
    fixture.write("file", "topic\n");
    fixture.commit_all("topic");
    let topic_tip = fixture.git(&["rev-parse", "topic"]);
    fixture.git(&["switch", "main"]);
    fixture.write("file", "uncommitted work\n");
    let repo = fixture.open();
    assert!(repo.switch_or_create_branch("topic").is_err());
    assert_eq!(repo.load_state().unwrap().branch, "main");
    assert_eq!(fixture.git(&["rev-parse", "topic"]), topic_tip);
    assert_eq!(
        fs::read_to_string(fixture.path.join("file")).unwrap(),
        "uncommitted work\n"
    );
}

#[test]
fn diff_ignores_external_diff_and_color_settings() {
    let fixture = TestRepo::new();
    fixture.write("file", "before\n");
    fixture.commit_all("base");
    fixture.git(&["config", "diff.external", "this-command-does-not-exist"]);
    fixture.git(&["config", "color.diff", "always"]);
    fixture.write("file", "after\n");
    let repo = fixture.open();
    let detail = repo
        .file_detail(&repo.load_state().unwrap().files[0])
        .unwrap();
    assert!(detail.contains("+after"));
    assert!(!detail.contains('\x1b'));
}

#[test]
fn untracked_text_binary_and_large_files_have_safe_previews() {
    let fixture = TestRepo::new();
    fixture.write("text", "hello\n");
    fs::write(fixture.path.join("binary"), b"a\0b").unwrap();
    fixture.write("large", &"a".repeat(300_000));
    let repo = fixture.open();
    let state = repo.load_state().unwrap();
    let detail = |name: &str| {
        repo.file_detail(
            state
                .files
                .iter()
                .find(|file| file.path == Path::new(name))
                .unwrap(),
        )
        .unwrap()
    };
    assert!(detail("text").contains("hello"));
    assert!(detail("binary").contains("Binary"));
    let large = detail("large");
    assert!(large.contains("truncated"));
    assert!(large.len() < 270_000);
}

#[cfg(unix)]
#[test]
fn untracked_symlinks_are_not_followed() {
    use std::os::unix::fs::symlink;
    let fixture = TestRepo::new();
    let outside = fixture.directory.0.join("outside.txt");
    fs::write(&outside, "private contents outside the working tree").unwrap();
    symlink(outside, fixture.path.join("link")).unwrap();
    let repo = fixture.open();
    let detail = repo
        .file_detail(&repo.load_state().unwrap().files[0])
        .unwrap();
    assert!(detail.contains("SYMLINK"));
    assert!(!detail.contains("private contents"));
}

#[test]
fn merge_conflicts_can_be_resolved_and_continued_without_an_editor() {
    let fixture = merge_conflict();
    let repo = fixture.open();
    let state = repo.load_state().unwrap();
    assert!(state.merge_state.merge_in_progress);
    assert_eq!(state.merge_state.conflicts, [Path::new("conflict.txt")]);
    assert!(repo.continue_operation().is_err());
    repo.resolve_side(Path::new("conflict.txt"), ConflictSide::Theirs)
        .unwrap();
    assert_eq!(
        fs::read_to_string(fixture.path.join("conflict.txt")).unwrap(),
        "theirs\n"
    );
    repo.continue_operation().unwrap();
    let state = repo.load_state().unwrap();
    assert!(!state.merge_state.merge_in_progress);
    assert!(state.merge_state.conflicts.is_empty());
    assert!(state.files.is_empty());
}

#[test]
fn taking_a_deleted_side_resolves_modify_delete_conflicts() {
    let fixture = TestRepo::new();
    fixture.write("file", "base\n");
    fixture.commit_all("base");
    fixture.git(&["switch", "-c", "topic"]);
    fixture.write("file", "changed\n");
    fixture.commit_all("modify");
    fixture.git(&["switch", "main"]);
    fixture.git(&["rm", "file"]);
    fixture.commit_all("delete");
    fixture.fail(&["merge", "topic"]);
    let repo = fixture.open();
    repo.resolve_side(Path::new("file"), ConflictSide::Ours)
        .unwrap();
    assert!(!fixture.path.join("file").exists());
    assert!(repo.load_state().unwrap().merge_state.conflicts.is_empty());
    repo.continue_operation().unwrap();
}

#[test]
fn stale_conflict_selection_never_deletes_a_non_conflicted_file() {
    let fixture = TestRepo::new();
    fixture.write("file", "keep me\n");
    fixture.commit_all("base");
    assert!(
        fixture
            .open()
            .resolve_side(Path::new("file"), ConflictSide::Ours)
            .is_err()
    );
    assert_eq!(
        fs::read_to_string(fixture.path.join("file")).unwrap(),
        "keep me\n"
    );
}

#[test]
fn linked_worktree_uses_its_own_merge_state_and_branch_names() {
    let fixture = TestRepo::new();
    fixture.write("file", "base\n");
    fixture.commit_all("base");
    let worktree = fixture.directory.0.join("linked worktree");
    fixture.git(&["worktree", "add", "-b", "topic", worktree.to_str().unwrap()]);
    fs::write(worktree.join("file"), "topic\n").unwrap();
    git(&worktree, &["commit", "-am", "topic change"]);
    fixture.write("file", "main\n");
    fixture.commit_all("main change");
    assert!(!git_output(&worktree, &["merge", "main"]).status.success());
    assert!(worktree.join(".git").is_file());
    let repo = Repository::open(&worktree).unwrap();
    let state = repo.load_state().unwrap();
    assert_eq!(state.branch, "topic");
    assert!(state.merge_state.merge_in_progress);
    assert_eq!(state.merge_state.conflicts, [Path::new("file")]);
    assert!(state.branches.iter().any(|branch| branch.name == "main"));
    assert!(
        !fixture
            .open()
            .load_state()
            .unwrap()
            .merge_state
            .merge_in_progress
    );
    repo.abort_operation().unwrap();
    assert!(repo.load_state().unwrap().files.is_empty());
}

#[test]
fn rebase_abort_restores_the_original_branch_and_tip() {
    let fixture = rebase_conflict();
    let original_tip = fixture.git(&["rev-parse", "topic"]);
    let repo = fixture.open();
    assert!(repo.load_state().unwrap().merge_state.rebase_in_progress);
    repo.abort_operation().unwrap();
    let state = repo.load_state().unwrap();
    assert!(!state.merge_state.rebase_in_progress);
    assert_eq!(state.branch, "topic");
    assert_eq!(fixture.git(&["rev-parse", "HEAD"]), original_tip);
    assert!(state.files.is_empty());
}

#[test]
fn non_repositories_and_bare_repositories_fail_explicitly() {
    let directory = TempDirectory::new();
    assert!(Repository::open(&directory.0).is_err());
    git(&directory.0, &["init", "--bare", "--quiet"]);
    let error = Repository::open(&directory.0).unwrap_err();
    assert!(error.to_string().contains("non-bare"));
}

#[test]
fn local_remote_fetch_pull_and_push_are_supported() {
    let fixture = TestRepo::new();
    fixture.write("file", "base\n");
    fixture.commit_all("base");
    let remote = fixture.directory.0.join("remote.git");
    fs::create_dir(&remote).unwrap();
    git(&remote, &["init", "--bare", "--quiet"]);
    fixture.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
    fixture.git(&["push", "--set-upstream", "origin", "main"]);
    fixture.write("file", "second\n");
    fixture.commit_all("second");
    let repo = fixture.open();
    repo.push().unwrap();
    let tip = fixture.git(&["rev-parse", "HEAD"]);
    assert_eq!(git(&remote, &["rev-parse", "refs/heads/main"]), tip);
    // This reset is confined to a disposable test repository with no user data.
    fixture.git(&["reset", "--hard", "HEAD~1"]);
    repo.fetch().unwrap();
    repo.pull().unwrap();
    assert_eq!(fixture.git(&["rev-parse", "HEAD"]), tip);
}

#[test]
fn already_staged_deletions_are_a_safe_noop_when_staged_again() {
    let fixture = TestRepo::new();
    fixture.write("file", "content\n");
    fixture.commit_all("base");
    fixture.git(&["rm", "file"]);
    let repo = fixture.open();
    let file = repo.load_state().unwrap().files.remove(0);
    assert_eq!(file.status, "D ");
    repo.stage(&file).unwrap();
    assert_eq!(repo.load_state().unwrap().files[0].status, "D ");
}

#[test]
fn large_unicode_previews_do_not_misclassify_partial_utf8_as_binary() {
    let fixture = TestRepo::new();
    fixture.write("unicode", &"界".repeat(100_000));
    let repo = fixture.open();
    let preview = repo
        .file_detail(&repo.load_state().unwrap().files[0])
        .unwrap();
    assert!(preview.starts_with("[UNTRACKED]\n界"));
    assert!(preview.ends_with("[Preview truncated at 256 KiB]"));
    assert!(preview.len() < 270_000);
}
