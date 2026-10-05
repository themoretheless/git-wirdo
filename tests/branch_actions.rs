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
fn editable_commit_amend_and_branch_names_preserve_unstaged_changes() {
    let repo = TestRepo::new();
    repo.write("staged", "one");
    repo.git(&["add", "staged"]);
    repo.write("untracked", "keep");
    let mut app = App::new(repo.open()).unwrap();
    app.handle(Action::Commit);
    submit(&mut app, "custom qsc message");
    assert!(!app.message_is_error && app.running);
    assert_eq!(
        repo.git(&["log", "-1", "--format=%s"]).trim(),
        "custom qsc message"
    );
    assert!(
        !repo
            .git(&["ls-tree", "--name-only", "HEAD"])
            .contains("untracked")
    );
    let before = repo.git(&["rev-parse", "HEAD"]);
    app.handle(Action::AmendCommit);
    submit(&mut app, "replacement");
    assert_ne!(repo.git(&["rev-parse", "HEAD"]), before);
    app.handle(Action::Branch);
    submit(&mut app, "topic/custom");
    assert_eq!(app.state.branch, "topic/custom");
    assert_eq!(
        std::fs::read_to_string(repo.path.join("untracked")).unwrap(),
        "keep"
    );
    repo.open()
        .rename_branch("topic/custom", "topic/renamed")
        .unwrap();
    assert!(repo.open().rename_branch("topic/renamed", "main").is_err());
}

#[test]
fn deletion_refuses_unique_commits_even_if_merged_to_upstream() {
    let repo = TestRepo::new();
    repo.write("base", "base");
    repo.commit_all("base");
    repo.git(&["switch", "-c", "unique"]);
    repo.write("unique", "save");
    repo.commit_all("unique");
    repo.git(&["branch", "upstream"]);
    repo.git(&["branch", "--set-upstream-to=upstream"]);
    repo.git(&["switch", "main"]);
    assert!(repo.open().delete_branch("unique").is_err());
    assert!(repo.git(&["show", "unique:unique"]).contains("save"));
    repo.open().integrate_branch("unique", false).unwrap();
    repo.open().delete_branch("unique").unwrap();
    assert!(repo.open().delete_branch("main").is_err());
    repo.git(&["branch", "release/keep"]);
    assert!(repo.open().delete_branch("release/keep").is_err());
}

#[test]
fn starting_merge_refreshes_conflicts_and_abort_restores_branch() {
    let repo = TestRepo::new();
    repo.write("file", "base\n");
    repo.commit_all("base");
    repo.git(&["switch", "-c", "topic"]);
    repo.write("file", "topic\n");
    repo.commit_all("topic");
    repo.git(&["switch", "main"]);
    repo.write("file", "main\n");
    repo.commit_all("main");
    let before = repo.git(&["rev-parse", "HEAD"]);
    let mut app = App::new(repo.open()).unwrap();
    app.view = ViewMode::Branches;
    app.branch_selection = app
        .state
        .branches
        .iter()
        .position(|b| b.name == "topic")
        .unwrap();
    app.handle(Action::MergeBranch);
    submit(&mut app, "merge");
    assert!(app.message_is_error && app.state.merge_state.merge_in_progress);
    assert_eq!(app.state.merge_state.conflicts.len(), 1);
    app.view = ViewMode::Conflicts;
    app.handle(Action::Abort);
    assert_eq!(repo.git(&["rev-parse", "HEAD"]), before);
    assert_eq!(
        std::fs::read_to_string(repo.path.join("file")).unwrap(),
        "main\n"
    );
}

#[test]
fn starting_rebase_replays_onto_selected_branch() {
    let repo = TestRepo::new();
    repo.write("base", "base");
    repo.commit_all("base");
    repo.git(&["switch", "-c", "topic"]);
    repo.write("topic", "topic");
    repo.commit_all("topic");
    repo.git(&["switch", "main"]);
    repo.write("main", "main");
    repo.commit_all("main");
    let base = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&["switch", "topic"]);
    repo.open().integrate_branch("main", true).unwrap();
    assert_eq!(repo.git(&["rev-parse", "HEAD^"]), base);
    assert!(
        repo.git(&["ls-tree", "--name-only", "HEAD"])
            .contains("topic")
    );
}
