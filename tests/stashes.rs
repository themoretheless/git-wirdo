mod support;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use git_wirdo::app::{Action, App};
use git_wirdo::model::ViewMode;
use support::TestRepo;

fn submit(app: &mut App, text: &str) {
    for c in text.chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
}

#[test]
fn stash_roundtrip_restores_index_working_tree_and_untracked_files() {
    let repo = TestRepo::new();
    repo.write("file", "base\n");
    repo.commit_all("base");
    repo.write("file", "staged\n");
    repo.git(&["add", "file"]);
    repo.write("file", "unstaged\n");
    repo.write("[new] file", "untracked\n");
    let repository = repo.open();
    repository.save_stash("qsc Unicode ё").unwrap();
    let entries = repository.stashes().unwrap();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].subject.contains("Unicode ё"));
    let detail = repository.stash_detail(&entries[0]).unwrap();
    assert!(detail.contains("untracked") && detail.contains("unstaged"));
    assert!(
        repository.load_state().unwrap().files.is_empty(),
        "{}",
        repo.git(&["status", "--porcelain"])
    );
    repository.apply_stash(&entries[0], false).unwrap();
    assert_eq!(repo.git(&["show", ":file"]), "staged\n");
    assert_eq!(
        std::fs::read_to_string(repo.path.join("file")).unwrap(),
        "unstaged\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.path.join("[new] file")).unwrap(),
        "untracked\n"
    );
    assert_eq!(repository.stashes().unwrap().len(), 1);
    assert!(repository.apply_stash(&entries[0], true).is_err());
}

#[test]
fn stale_stash_selector_never_removes_a_different_entry() {
    let repo = TestRepo::new();
    repo.write("file", "base");
    repo.commit_all("base");
    let repository = repo.open();
    repo.write("file", "first");
    repository.save_stash("first").unwrap();
    let old = repository.stashes().unwrap()[0].clone();
    repo.write("file", "second");
    repository.save_stash("second").unwrap();
    assert!(repository.drop_stash(&old).is_err());
    assert!(repository.apply_stash(&old, true).is_err());
    assert_eq!(repository.stashes().unwrap().len(), 2);
    assert_eq!(
        std::fs::read_to_string(repo.path.join("file")).unwrap(),
        "base"
    );
}

#[test]
fn successful_pop_removes_only_selected_entry_and_conflicting_pop_retains_it() {
    let repo = TestRepo::new();
    repo.write("file", "base\n");
    repo.commit_all("base");
    let repository = repo.open();
    repo.write("file", "saved\n");
    repository.save_stash("saved").unwrap();
    let stash = repository.stashes().unwrap()[0].clone();
    repo.write("file", "new base\n");
    repo.commit_all("changed base");
    let mut app = App::new(repository.clone()).unwrap();
    app.view = ViewMode::Stashes;
    app.refresh().unwrap();
    app.handle(Action::PopStash);
    submit(&mut app, "pop");
    assert!(app.message_is_error && app.running);
    assert_eq!(app.stashes[0].sha, stash.sha);
    assert!(!app.state.files.is_empty());
    // Reset only the isolated test repository after verifying conflict preservation.
    repo.git(&["reset", "--hard", "HEAD^"]);
    repository.apply_stash(&stash, true).unwrap();
    assert!(repository.stashes().unwrap().is_empty());
    assert_eq!(
        std::fs::read_to_string(repo.path.join("file")).unwrap(),
        "saved\n"
    );
}

#[test]
fn stash_prompts_preserve_changes_until_submission_and_confirm_drop() {
    let repo = TestRepo::new();
    repo.write("file", "base");
    repo.commit_all("base");
    repo.write("untracked", "save me");
    let mut app = App::new(repo.open()).unwrap();
    app.view = ViewMode::Stashes;
    app.refresh().unwrap();
    app.handle(Action::SaveStash);
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(repo.path.join("untracked").exists());
    app.handle(Action::SaveStash);
    submit(&mut app, "named stash");
    assert!(
        app.stashes.len() == 1 && !repo.path.join("untracked").exists(),
        "{}; {}",
        app.message,
        repo.git(&["status", "--porcelain"])
    );
    app.handle(Action::Remove);
    submit(&mut app, "yes");
    assert_eq!(app.repository.stashes().unwrap().len(), 1);
    app.handle(Action::Remove);
    submit(&mut app, "drop");
    assert!(app.stashes.is_empty());
}
