mod support;
use support::TestRepo;

#[test]
fn restore_index_keeps_staged_edits_and_restore_head_discards_both() {
    let repo = TestRepo::new();
    repo.write("file", "base");
    repo.commit_all("base");
    repo.write("file", "staged");
    repo.git(&["add", "file"]);
    repo.write("file", "unstaged");
    let repository = repo.open();
    let file = repository.load_state().unwrap().files[0].clone();
    let request = repository.prepare_restore(&file, false).unwrap();
    repository.restore_file(&request).unwrap();
    assert_eq!(
        std::fs::read_to_string(repo.path.join("file")).unwrap(),
        "staged"
    );
    assert_eq!(repo.git(&["show", ":file"]), "staged");
    let file = repository.load_state().unwrap().files[0].clone();
    let request = repository.prepare_restore(&file, true).unwrap();
    repository.restore_file(&request).unwrap();
    assert!(repository.load_state().unwrap().files.is_empty());
}

#[test]
fn changed_file_or_index_invalidates_discard_confirmation() {
    let repo = TestRepo::new();
    repo.write("file", "base");
    repo.commit_all("base");
    repo.write("file", "dirty");
    let repository = repo.open();
    let file = repository.load_state().unwrap().files[0].clone();
    let request = repository.prepare_restore(&file, true).unwrap();
    repo.write("file", "new edits");
    assert!(repository.restore_file(&request).is_err());
    assert_eq!(
        std::fs::read_to_string(repo.path.join("file")).unwrap(),
        "new edits"
    );
    let request = repository.prepare_restore(&file, true).unwrap();
    repo.git(&["add", "file"]);
    assert!(repository.restore_file(&request).is_err());
    assert_eq!(repo.git(&["show", ":file"]), "new edits");
}

#[test]
fn rename_restore_recovers_original_path_and_untracked_discard_is_selective() {
    let repo = TestRepo::new();
    repo.write("old", "base");
    repo.commit_all("base");
    repo.git(&["mv", "old", "new"]);
    let repository = repo.open();
    let file = repository.load_state().unwrap().files[0].clone();
    repository
        .restore_file(&repository.prepare_restore(&file, true).unwrap())
        .unwrap();
    assert!(repo.path.join("old").exists() && !repo.path.join("new").exists());
    repo.write("[a]", "discard");
    repo.write("a", "retain");
    let file = repository
        .load_state()
        .unwrap()
        .files
        .into_iter()
        .find(|f| f.path.to_str() == Some("[a]"))
        .unwrap();
    repository
        .restore_file(&repository.prepare_restore(&file, true).unwrap())
        .unwrap();
    assert!(!repo.path.join("[a]").exists() && repo.path.join("a").exists());
}

#[cfg(unix)]
#[test]
fn untracked_symlink_discard_does_not_touch_its_target() {
    let repo = TestRepo::new();
    let outside = repo.directory.0.join("outside");
    std::fs::write(&outside, "retain").unwrap();
    std::os::unix::fs::symlink(&outside, repo.path.join("link")).unwrap();
    let repository = repo.open();
    let file = repository.load_state().unwrap().files[0].clone();
    repository
        .restore_file(&repository.prepare_restore(&file, true).unwrap())
        .unwrap();
    assert_eq!(std::fs::read_to_string(outside).unwrap(), "retain");
}

#[test]
fn discard_prompt_can_be_cancelled_and_never_acts_in_upstream_view() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use git_wirdo::app::{Action, App};
    let repo = TestRepo::new();
    repo.write("file", "base");
    repo.commit_all("base");
    repo.write("file", "dirty");
    let mut app = App::new(repo.open()).unwrap();
    app.handle(Action::Remove);
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(
        std::fs::read_to_string(repo.path.join("file")).unwrap(),
        "dirty"
    );
    app.upstream_comparison = true;
    app.handle(Action::Remove);
    assert!(app.prompt.is_none());
    app.upstream_comparison = false;
    app.handle(Action::RestoreFromIndex);
    for c in "discard".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        std::fs::read_to_string(repo.path.join("file")).unwrap(),
        "base"
    );
}

#[test]
fn previously_untracked_file_staged_before_prompt_requires_refresh() {
    let repo = TestRepo::new();
    repo.write("file", "new");
    let repository = repo.open();
    let stale = repository.load_state().unwrap().files[0].clone();
    repo.git(&["add", "file"]);
    assert!(repository.prepare_restore(&stale, true).is_err());
    assert_eq!(
        std::fs::read_to_string(repo.path.join("file")).unwrap(),
        "new"
    );
}
