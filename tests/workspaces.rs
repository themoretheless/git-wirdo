mod support;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use git_wirdo::app::{Action, App};
use git_wirdo::model::ViewMode;
use support::TestRepo;

fn type_text(app: &mut App, value: &str) {
    for c in value.chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
}

#[test]
fn create_switch_and_remove_workspaces_preserves_dirty_files_and_branches() {
    let repo = TestRepo::new();
    repo.write("file", "base\n");
    repo.commit_all("base");
    let path = repo.directory.0.join("workspace with spaces");
    repo.open().create_workspace(&path, "topic").unwrap();
    let entries = repo.open().workspaces().unwrap();
    let w = entries.iter().find(|w| w.branch == "topic").unwrap();
    assert_eq!(w.path.canonicalize().unwrap(), path.canonicalize().unwrap());
    std::fs::write(path.join("file"), "keep me\n").unwrap();
    assert!(repo.open().remove_workspace(w).is_err());
    assert_eq!(
        std::fs::read_to_string(path.join("file")).unwrap(),
        "keep me\n"
    );
    repo.git(&["worktree", "lock", path.to_str().unwrap()]);
    assert!(
        repo.open()
            .workspaces()
            .unwrap()
            .iter()
            .find(|w| w.branch == "topic")
            .unwrap()
            .locked
    );
    repo.git(&["worktree", "unlock", path.to_str().unwrap()]);
    let mut app = App::new(repo.open()).unwrap();
    app.set_view(ViewMode::Worktrees);
    app.refresh().unwrap();
    app.select(
        app.workspaces()
            .items
            .iter()
            .position(|w| w.branch == "topic")
            .unwrap(),
    );
    app.handle(Action::SwitchBranch);
    assert_eq!(
        app.repository().root().canonicalize().unwrap(),
        path.canonicalize().unwrap()
    );
    assert!(app.detail_text().contains("keep me"));
    app.handle(Action::OpenRepository);
    type_text(&mut app, repo.path.to_str().unwrap());
    assert_eq!(
        app.repository().root().canonicalize().unwrap(),
        repo.path.canonicalize().unwrap()
    );
    let workspace_repo = git_wirdo::git::Repository::open(&path).unwrap();
    assert!(workspace_repo.ensure_clean().is_err());
    support::git(&path, &["add", "--all"]);
    support::git(&path, &["commit", "-m", "retain work"]);
    repo.open().remove_workspace(w).unwrap();
    assert!(!path.exists());
    assert!(repo.git(&["show", "topic:file"]).contains("keep me"));
}

#[test]
fn prompts_consume_shortcuts_and_require_explicit_remove_confirmation() {
    let repo = TestRepo::new();
    repo.write("file", "base");
    repo.commit_all("base");
    let path = repo.directory.0.join("review");
    let mut app = App::new(repo.open()).unwrap();
    app.set_view(ViewMode::Worktrees);
    app.refresh().unwrap();
    app.handle(Action::New);
    type_text(&mut app, path.to_str().unwrap());
    type_text(&mut app, "qsc-topic");
    assert!(app.running() && path.exists());
    app.select(
        app.workspaces()
            .items
            .iter()
            .position(|w| w.branch == "qsc-topic")
            .unwrap(),
    );
    app.handle(Action::Remove);
    type_text(&mut app, "yes");
    assert!(path.exists() && app.message_is_error());
    app.handle(Action::Remove);
    type_text(&mut app, "remove");
    assert!(!path.exists());
    assert!(
        repo.git(&["branch", "--list", "qsc-topic"])
            .contains("qsc-topic")
    );
}

#[test]
fn failed_open_preserves_current_repo_and_escape_cancels_prompt() {
    let repo = TestRepo::new();
    let mut app = App::new(repo.open()).unwrap();
    app.handle(Action::OpenRepository);
    type_text(&mut app, "nonexistent");
    assert_eq!(
        app.repository().root().canonicalize().unwrap(),
        repo.path.canonicalize().unwrap()
    );
    assert!(app.running() && app.message_is_error());
    app.handle(Action::OpenRepository);
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.prompt().is_none());
}

#[test]
fn removal_confirmation_rejects_a_different_branch_at_the_selected_path() {
    let repo = TestRepo::new();
    repo.write("file", "base\n");
    repo.commit_all("base");
    let path = repo.directory.0.join("review");
    repo.open().create_workspace(&path, "topic").unwrap();
    let mut app = App::new(repo.open()).unwrap();
    app.set_view(ViewMode::Worktrees);
    app.refresh().unwrap();
    app.select(
        app.workspaces()
            .items
            .iter()
            .position(|w| w.branch == "topic")
            .unwrap(),
    );
    app.handle(Action::Remove);
    support::git(&path, &["switch", "-c", "replacement"]);
    type_text(&mut app, "remove");
    assert!(app.message_is_error(), "{}", app.message());
    assert!(path.join("file").exists());
    assert_eq!(
        support::git(&path, &["branch", "--show-current"]).trim(),
        "replacement"
    );
}

#[test]
fn removal_confirmation_rejects_new_head_and_keeps_unique_commit() {
    let repo = TestRepo::new();
    repo.write("file", "base\n");
    repo.commit_all("base");
    let path = repo.directory.0.join("review");
    let repository = repo.open();
    repository.create_workspace(&path, "topic").unwrap();
    let selected = repository
        .workspaces()
        .unwrap()
        .into_iter()
        .find(|w| w.branch == "topic")
        .unwrap();
    let request = repository.prepare_worktree_removal(&selected).unwrap();
    support::git(&path, &["commit", "--allow-empty", "-m", "unique"]);
    let head = support::git(&path, &["rev-parse", "HEAD"]);
    assert!(repository.remove_prepared_worktree(&request).is_err());
    assert!(path.join(".git").is_file());
    assert_eq!(support::git(&path, &["rev-parse", "HEAD"]), head);
    assert_eq!(repo.git(&["rev-parse", "topic"]), head);
}

#[test]
fn lock_or_new_untracked_file_after_confirmation_prevents_removal() {
    let repo = TestRepo::new();
    repo.write("file", "base\n");
    repo.commit_all("base");
    let path = repo.directory.0.join("review");
    let repository = repo.open();
    repository.create_workspace(&path, "topic").unwrap();
    let selected = repository
        .workspaces()
        .unwrap()
        .into_iter()
        .find(|w| w.branch == "topic")
        .unwrap();
    let request = repository.prepare_worktree_removal(&selected).unwrap();
    repo.git(&["worktree", "lock", path.to_str().unwrap()]);
    assert!(repository.remove_prepared_worktree(&request).is_err());
    repo.git(&["worktree", "unlock", path.to_str().unwrap()]);
    std::fs::write(path.join("new"), "retain\n").unwrap();
    assert!(repository.remove_prepared_worktree(&request).is_err());
    assert_eq!(
        std::fs::read_to_string(path.join("new")).unwrap(),
        "retain\n"
    );
    assert_eq!(support::git(&path, &["show", ":file"]), "base\n");
}

#[test]
fn ignored_files_are_retained_when_removing_a_worktree() {
    let repo = TestRepo::new();
    repo.write(".gitignore", "private/\n");
    repo.commit_all("base");
    let path = repo.directory.0.join("review");
    let repository = repo.open();
    repository.create_workspace(&path, "topic").unwrap();
    std::fs::create_dir(path.join("private")).unwrap();
    std::fs::write(path.join("private/notes"), "retain ignored notes\n").unwrap();
    let selected = repository
        .workspaces()
        .unwrap()
        .into_iter()
        .find(|w| w.branch == "topic")
        .unwrap();
    assert!(repository.remove_workspace(&selected).is_err());
    assert_eq!(
        std::fs::read_to_string(path.join("private/notes")).unwrap(),
        "retain ignored notes\n"
    );
}

#[test]
fn assume_unchanged_edits_are_not_deleted_with_a_worktree() {
    let repo = TestRepo::new();
    repo.write("file", "base\n");
    repo.commit_all("base");
    let path = repo.directory.0.join("review");
    let repository = repo.open();
    repository.create_workspace(&path, "topic").unwrap();
    support::git(&path, &["update-index", "--assume-unchanged", "file"]);
    std::fs::write(path.join("file"), "hidden edits\n").unwrap();
    let selected = repository
        .workspaces()
        .unwrap()
        .into_iter()
        .find(|w| w.branch == "topic")
        .unwrap();
    assert!(repository.remove_workspace(&selected).is_err());
    assert_eq!(
        std::fs::read_to_string(path.join("file")).unwrap(),
        "hidden edits\n"
    );
}

#[test]
fn unfinished_operation_is_not_discarded_with_a_clean_worktree() {
    let repo = TestRepo::new();
    repo.write("file", "base\n");
    repo.commit_all("base");
    let path = repo.directory.0.join("review");
    let repository = repo.open();
    repository.create_workspace(&path, "topic").unwrap();
    let git_dir =
        std::path::PathBuf::from(support::git(&path, &["rev-parse", "--absolute-git-dir"]).trim());
    let head = support::git(&path, &["rev-parse", "HEAD"]);
    std::fs::write(git_dir.join("CHERRY_PICK_HEAD"), &head).unwrap();
    let selected = repository
        .workspaces()
        .unwrap()
        .into_iter()
        .find(|w| w.branch == "topic")
        .unwrap();
    assert!(repository.remove_workspace(&selected).is_err());
    assert_eq!(
        std::fs::read_to_string(git_dir.join("CHERRY_PICK_HEAD")).unwrap(),
        head
    );
    assert!(path.join("file").exists());
}

#[test]
fn worktrees_use_existing_branches_and_explicit_bases_without_changing_current_files() {
    let repo = TestRepo::new();
    repo.write("file", "base\n");
    repo.commit_all("base");
    let base = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&["branch", "existing"]);
    repo.write("file", "later\n");
    repo.commit_all("later");
    repo.write("file", "unstaged original\n");
    let before_index = repo.git(&["ls-files", "--stage"]);
    let repository = repo.open();
    let existing = repo.directory.0.join("existing ü");
    repository
        .create_worktree_existing(&existing, "existing")
        .unwrap();
    let from = repo.directory.0.join("from base");
    repository
        .create_worktree_from(&from, "from-base", base.trim())
        .unwrap();
    assert_eq!(support::git(&existing, &["rev-parse", "HEAD"]), base);
    assert_eq!(support::git(&from, &["rev-parse", "HEAD"]), base);
    assert_eq!(repo.git(&["ls-files", "--stage"]), before_index);
    assert_eq!(
        std::fs::read_to_string(repo.path.join("file")).unwrap(),
        "unstaged original\n"
    );
    let occupied = repo.directory.0.join("occupied");
    assert!(
        repository
            .create_worktree_existing(&occupied, "existing")
            .is_err()
    );
    assert!(!occupied.exists());
    assert!(
        repository
            .create_worktree_existing(&occupied, "does-not-exist")
            .is_err()
    );
    assert!(
        repository
            .create_worktree_from(&occupied, "invalid-base", "--help")
            .is_err()
    );
    assert!(
        repo.git(&["branch", "--list", "invalid-base"])
            .trim()
            .is_empty()
    );
    let mut app = App::new(repository).unwrap();
    app.set_view(ViewMode::Branches);
    app.select(
        app.state()
            .branches
            .iter()
            .position(|b| b.name == "existing")
            .unwrap(),
    );
    assert!(app.detail_text().contains("Checked out in:"));
    assert!(app.detail_text().contains("existing ü"));
}

#[test]
fn lock_unlock_and_move_preserve_dirty_files_and_index_and_reject_stale_requests() {
    let repo = TestRepo::new();
    repo.write("file", "base\n");
    repo.commit_all("base");
    let path = repo.directory.0.join("review");
    let moved = repo.directory.0.join("moved ü");
    let repository = repo.open();
    repository.create_workspace(&path, "topic").unwrap();
    let entry = || {
        repository
            .workspaces()
            .unwrap()
            .into_iter()
            .find(|w| w.branch == "topic")
            .unwrap()
    };
    let selected = entry();
    let request = repository.prepare_worktree_removal(&selected).unwrap();
    repository
        .set_worktree_lock(&selected, true, "retain while offline")
        .unwrap();
    assert!(entry().locked);
    assert!(repository.move_worktree(&request, &moved).is_err());
    assert!(repository.set_worktree_lock(&selected, false, "").is_err());
    repository.set_worktree_lock(&entry(), false, "").unwrap();
    std::fs::write(path.join("file"), "staged\n").unwrap();
    support::git(&path, &["add", "file"]);
    std::fs::write(path.join("file"), "unstaged\n").unwrap();
    std::fs::write(path.join("notes"), "untracked\n").unwrap();
    repository.move_worktree(&request, &moved).unwrap();
    assert!(!path.exists());
    assert_eq!(support::git(&moved, &["show", ":file"]), "staged\n");
    assert_eq!(
        std::fs::read_to_string(moved.join("file")).unwrap(),
        "unstaged\n"
    );
    assert_eq!(
        std::fs::read_to_string(moved.join("notes")).unwrap(),
        "untracked\n"
    );
    assert_eq!(
        entry().path.canonicalize().unwrap(),
        moved.canonicalize().unwrap()
    );
    assert!(repository.move_worktree(&request, &path).is_err());
}
