mod support;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use git_wirdo::app::{Action, App};
use support::TestRepo;

#[test]
fn comparison_uses_merge_base_and_never_stages_review_paths() {
    let repo = TestRepo::new();
    repo.write("[file].txt", "base\n");
    repo.commit_all("base");
    repo.git(&["switch", "-c", "topic"]);
    repo.git(&["branch", "--set-upstream-to=main"]);
    repo.write("[file].txt", "topic\n");
    repo.commit_all("topic");
    repo.git(&["switch", "main"]);
    repo.write("upstream-only", "upstream\n");
    repo.commit_all("upstream advances");
    repo.git(&["switch", "topic"]);
    repo.write("[file].txt", "working\n");
    let mut app = App::new(repo.open()).unwrap();
    app.handle(Action::ToggleComparison);
    assert!(app.upstream_comparison && !app.message_is_error);
    assert_eq!(app.displayed_files().len(), 1);
    assert!(app.detail_text.contains("-base") && app.detail_text.contains("+working"));
    let index = repo.git(&["ls-files", "--stage"]);
    app.handle(Action::Stage);
    app.handle(Action::Unstage);
    assert_eq!(repo.git(&["ls-files", "--stage"]), index);
    app.handle(Action::ToggleComparison);
    assert!(!app.upstream_comparison);
    app.handle(Action::Stage);
    assert_ne!(repo.git(&["ls-files", "--stage"]), index);
}

#[test]
fn missing_upstream_is_recoverable_and_keeps_working_view() {
    let repo = TestRepo::new();
    let mut app = App::new(repo.open()).unwrap();
    app.handle(Action::ToggleComparison);
    assert!(!app.upstream_comparison && app.running && app.message_is_error);
    assert!(app.message.contains("upstream"));
    app.handle(Action::Refresh);
    assert!(!app.message_is_error);
}

#[test]
fn seen_survives_refresh_but_clears_when_contents_change() {
    let repo = TestRepo::new();
    repo.write("file", "first\n");
    let mut app = App::new(repo.open()).unwrap();
    app.handle(Action::ToggleSeen);
    app.refresh().unwrap();
    assert!(app.file_seen(&app.displayed_files()[0]));
    repo.write("file", "second\n");
    app.refresh().unwrap();
    assert!(!app.file_seen(&app.displayed_files()[0]));
}

#[test]
fn search_consumes_git_shortcuts_and_wraps_between_matches() {
    let repo = TestRepo::new();
    repo.write(
        "file",
        &(0..40).map(|i| format!("line {i}\n")).collect::<String>(),
    );
    let mut app = App::new(repo.open()).unwrap();
    app.handle(Action::PageDown);
    assert_eq!(app.detail_scroll, 10);
    app.handle(Action::Search);
    for c in "line 3".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    assert!(app.running && repo.git(&["ls-files"]).is_empty());
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.detail_scroll, 4);
    app.handle(Action::NextMatch);
    assert_eq!(app.detail_scroll, 31);
    for _ in 0..10 {
        app.handle(Action::NextMatch);
    }
    assert_eq!(app.detail_scroll, 4);
    app.handle(Action::NextItem);
    assert_eq!(app.detail_scroll, 0);
}
