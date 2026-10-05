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
fn history_loads_older_and_other_branch_commits_without_losing_selection() {
    let repo = TestRepo::new();
    for n in 0..25 {
        repo.write("file", &n.to_string());
        repo.commit_all(&format!("commit {n}"));
    }
    repo.git(&["switch", "-c", "topic"]);
    repo.write("topic", "topic");
    repo.commit_all("topic");
    repo.git(&["switch", "main"]);
    let mut app = App::new(repo.open()).unwrap();
    app.view = ViewMode::History;
    assert_eq!(app.state.commits.len(), 20);
    app.handle(Action::LoadHistory);
    assert_eq!(app.state.commits.len(), 26);
    assert!(app.state.commits.iter().any(|c| c.subject == "topic"));
    app.history_selection = 25;
    app.handle(Action::Refresh);
    assert_eq!(app.history_selection, 25);
    assert_eq!(app.state.commits[25].subject, "commit 0");
}

#[test]
fn confirmed_cherry_pick_and_revert_make_commits_and_cancel_preserves_head() {
    let repo = TestRepo::new();
    repo.write("base", "base");
    repo.commit_all("base");
    repo.git(&["switch", "-c", "topic"]);
    repo.write("topic", "topic");
    repo.commit_all("topic");
    let sha = repo.git(&["rev-parse", "HEAD"]).trim().to_owned();
    repo.git(&["switch", "main"]);
    let before = repo.git(&["rev-parse", "HEAD"]);
    let mut app = App::new(repo.open()).unwrap();
    app.view = ViewMode::History;
    app.history_selection = app.state.commits.iter().position(|c| c.sha == sha).unwrap();
    app.handle(Action::CherryPick);
    submit(&mut app, "wrong");
    submit(&mut app, "");
    assert!(app.message_is_error);
    assert_eq!(repo.git(&["rev-parse", "HEAD"]), before);
    app.handle(Action::CherryPick);
    submit(&mut app, &sha);
    submit(&mut app, "");
    assert!(!app.message_is_error, "{}", app.message);
    assert_eq!(
        std::fs::read_to_string(repo.path.join("topic")).unwrap(),
        "topic"
    );
    app.history_selection = app.state.commits.iter().position(|c| c.sha == sha).unwrap();
    app.handle(Action::RevertCommit);
    submit(&mut app, &sha);
    submit(&mut app, "");
    assert!(!app.message_is_error, "{}", app.message);
    assert!(!repo.path.join("topic").exists());
    assert!(repo.git(&["log", "-1", "--format=%s"]).contains("Revert"));
}

fn conflict_repo() -> (TestRepo, String) {
    let repo = TestRepo::new();
    repo.write("file", "base\n");
    repo.commit_all("base");
    repo.git(&["switch", "-c", "topic"]);
    repo.write("file", "topic\n");
    repo.commit_all("topic");
    let sha = repo.git(&["rev-parse", "HEAD"]).trim().to_owned();
    repo.git(&["switch", "main"]);
    repo.write("file", "main\n");
    repo.commit_all("main");
    (repo, sha)
}

#[test]
fn cherry_pick_conflict_can_abort_or_resolve_and_continue() {
    let (repo, sha) = conflict_repo();
    let r = repo.open();
    let head = repo.git(&["rev-parse", "HEAD"]);
    assert!(r.apply_commit(&sha, false, None).is_err());
    let state = r.load_state().unwrap().merge_state;
    assert!(state.cherry_pick_in_progress && state.in_progress());
    assert!(state.summary().contains("Cherry-pick"));
    assert!(r.continue_operation().is_err());
    assert!(r.save_stash("unsafe").is_err());
    r.abort_operation().unwrap();
    assert_eq!(repo.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(
        std::fs::read_to_string(repo.path.join("file")).unwrap(),
        "main\n"
    );
    assert!(r.apply_commit(&sha, false, None).is_err());
    repo.write("file", "resolved\n");
    r.mark_resolved(std::path::Path::new("file")).unwrap();
    r.continue_operation().unwrap();
    assert!(!r.load_state().unwrap().merge_state.in_progress());
    assert_eq!(
        std::fs::read_to_string(repo.path.join("file")).unwrap(),
        "resolved\n"
    );
}

#[test]
fn revert_conflict_abort_and_continue_preserve_operation_type() {
    let repo = TestRepo::new();
    repo.write("file", "base\n");
    repo.commit_all("base");
    repo.write("file", "change\n");
    repo.commit_all("change");
    let sha = repo.git(&["rev-parse", "HEAD"]).trim().to_owned();
    repo.write("file", "later\n");
    repo.commit_all("later");
    let head = repo.git(&["rev-parse", "HEAD"]);
    let r = repo.open();
    assert!(r.apply_commit(&sha, true, None).is_err());
    assert!(r.load_state().unwrap().merge_state.revert_in_progress);
    r.abort_operation().unwrap();
    assert_eq!(repo.git(&["rev-parse", "HEAD"]), head);
    assert!(r.apply_commit(&sha, true, None).is_err());
    repo.write("file", "resolved\n");
    r.mark_resolved(std::path::Path::new("file")).unwrap();
    r.continue_operation().unwrap();
    assert!(!r.load_state().unwrap().merge_state.in_progress());
    assert!(repo.git(&["log", "-1", "--format=%s"]).contains("Revert"));
}

#[test]
fn merge_commits_require_valid_mainline_and_dirty_state_is_preserved() {
    let repo = TestRepo::new();
    repo.write("base", "base");
    repo.commit_all("base");
    repo.git(&["switch", "-c", "topic"]);
    repo.write("topic", "topic");
    repo.commit_all("topic");
    repo.git(&["switch", "main"]);
    repo.write("main", "main");
    repo.commit_all("main");
    repo.git(&["merge", "--no-edit", "topic"]);
    let sha = repo.git(&["rev-parse", "HEAD"]).trim().to_owned();
    let r = repo.open();
    assert!(r.apply_commit(&sha, true, None).is_err());
    assert!(r.apply_commit(&sha, true, Some(0)).is_err());
    assert!(r.apply_commit(&sha, true, Some(3)).is_err());
    repo.write("untracked", "keep");
    assert!(r.apply_commit(&sha, true, Some(1)).is_err());
    assert_eq!(
        std::fs::read_to_string(repo.path.join("untracked")).unwrap(),
        "keep"
    );
    std::fs::remove_file(repo.path.join("untracked")).unwrap();
    r.apply_commit(&sha, true, Some(1)).unwrap();
    assert!(!repo.path.join("topic").exists());
    assert!(repo.path.join("main").exists());
    assert!(r.apply_commit("--help", false, None).is_err());
}

#[test]
fn history_search_selects_matching_commits_and_wraps_without_writes() {
    let repo = TestRepo::new();
    repo.write("one", "one");
    repo.commit_all("needle first");
    repo.write("two", "two");
    repo.commit_all("unrelated");
    repo.write("three", "three");
    repo.commit_all("needle latest");
    let before = repo.git(&["rev-parse", "HEAD"]);
    let mut app = App::new(repo.open()).unwrap();
    app.view = ViewMode::History;
    app.handle(Action::Search);
    submit(&mut app, "needle");
    assert_eq!(app.history_selection, 0);
    app.handle(Action::NextMatch);
    assert_eq!(app.history_selection, 2);
    app.handle(Action::NextMatch);
    assert_eq!(app.history_selection, 0);
    assert_eq!(repo.git(&["rev-parse", "HEAD"]), before);
}
