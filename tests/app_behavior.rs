mod support;

use std::path::Path;

use git_wirdo::app::{Action, App};
use git_wirdo::model::ViewMode;
use support::{TestRepo, rebase_conflict};

#[test]
fn failed_git_actions_keep_the_app_running_and_show_the_error() {
    let fixture = TestRepo::new();
    let mut app = App::new(fixture.open()).unwrap();
    app.handle(Action::Commit);
    assert!(app.running);
    assert!(app.message_is_error);
    assert!(app.detail_text.contains("Git failed"));
    app.handle(Action::Refresh);
    assert!(app.running);
    assert!(!app.message_is_error);
    app.handle(Action::Push);
    assert!(app.running && app.message_is_error);
    app.handle(Action::Quit);
    assert!(!app.running);
}

#[test]
fn staging_keys_do_not_mutate_hidden_file_selections() {
    let fixture = TestRepo::new();
    fixture.write("staged", "one\n");
    fixture.write("unstaged", "two\n");
    fixture.git(&["add", "staged"]);
    let mut app = App::new(fixture.open()).unwrap();
    let before = fixture.git(&["ls-files", "--stage"]);
    for view in [ViewMode::History, ViewMode::Branches, ViewMode::Conflicts] {
        app.view = view;
        for index in 0..2 {
            app.file_selection = index;
            app.handle(Action::Stage);
            app.handle(Action::Unstage);
            assert_eq!(fixture.git(&["ls-files", "--stage"]), before);
        }
    }
}

#[test]
fn navigation_does_not_skip_a_rebase_but_explicit_skip_does() {
    let fixture = rebase_conflict();
    let mut app = App::new(fixture.open()).unwrap();
    app.view = ViewMode::Conflicts;
    let before = fixture.git(&["rev-parse", "HEAD"]);
    app.handle(Action::PreviousItem);
    assert!(app.state.merge_state.rebase_in_progress);
    assert_eq!(app.state.merge_state.conflicts, [Path::new("first.txt")]);
    assert_eq!(fixture.git(&["rev-parse", "HEAD"]), before);
    assert!(!app.message_is_error);
    app.handle(Action::SkipRebase);
    assert!(app.state.merge_state.rebase_in_progress);
    assert_eq!(app.state.merge_state.conflicts, [Path::new("second.txt")]);
    app.handle(Action::SkipRebase);
    assert!(!app.state.merge_state.rebase_in_progress);
    assert!(app.running && !app.message_is_error);
}

#[test]
fn failed_continue_refreshes_state_after_rebase_advances_to_another_conflict() {
    let fixture = rebase_conflict();
    fixture.git(&["config", "core.editor", "this-editor-must-never-start"]);
    let mut app = App::new(fixture.open()).unwrap();
    app.view = ViewMode::Conflicts;
    app.handle(Action::TakeTheirs);
    assert!(app.state.merge_state.conflicts.is_empty());
    app.handle(Action::Continue);
    assert!(app.running && app.message_is_error);
    assert!(app.state.merge_state.rebase_in_progress);
    assert_eq!(app.state.merge_state.conflicts, [Path::new("second.txt")]);
    app.handle(Action::TakeTheirs);
    app.handle(Action::Continue);
    assert!(!app.message_is_error, "{}", app.message);
    assert!(!app.state.merge_state.rebase_in_progress);
    assert_eq!(app.state.branch, "topic");
    assert!(app.state.files.is_empty());
}

#[test]
fn selections_are_clamped_when_lists_shrink_and_empty_lists_are_navigable() {
    let fixture = TestRepo::new();
    fixture.write("first", "one\n");
    fixture.write("second", "two\n");
    let mut app = App::new(fixture.open()).unwrap();
    app.handle(Action::NextItem);
    assert_eq!(app.file_selection, 1);
    fixture.commit_all("all files");
    app.refresh().unwrap();
    assert_eq!(app.file_selection, 0);
    for _ in 0..4 {
        app.handle(Action::PreviousItem);
        app.handle(Action::NextItem);
        app.handle(Action::NextView);
    }
    assert!(app.running);
    assert_eq!(app.file_selection, 0);
}
