//! GitHub flows driven through the TUI with a scripted `gh`.
use crate::app::{Action, App};
use crate::github::{Mock, PullRequest};
use crate::keys::{KeyCode, KeyEvent, KeyModifiers};
use crate::model::ViewMode;
use crate::test_support::TestRepo;

fn press(app: &mut crate::app::App, text: &str) {
    for c in text.chars() {
        app.handle_key(crate::keys::KeyEvent::new(
            crate::keys::KeyCode::Char(c),
            crate::keys::KeyModifiers::NONE,
        ));
    }
    app.handle_key(crate::keys::KeyEvent::new(
        crate::keys::KeyCode::Enter,
        crate::keys::KeyModifiers::NONE,
    ));
}
fn selected_app() -> (crate::test_support::TestRepo, crate::app::App) {
    let repo = crate::test_support::TestRepo::new();
    let mut app = crate::app::App::new(repo.open()).unwrap();
    app.view = crate::model::ViewMode::PullRequests;
    app.pull_requests.items.push(PullRequest {
        number: 7,
        title: "Title".into(),
        url: "url".into(),
        head_ref_name: "topic".into(),
        base_ref_name: "main".into(),
        head_ref_oid: "a".repeat(40),
        is_draft: true,
    });
    (repo, app)
}
#[test]
fn tui_edit_prefills_current_fields_and_submits_multiline_body_without_shell_interpretation() {
    let (_repo, mut app) = selected_app();
    let metadata =
        serde_json::json!({"number":7,"title":"Title","body":"Old\nbody","baseRefName":"main"})
            .to_string();
    let mock = Mock::new([metadata.clone(), metadata, "edited".into(), "[]".into()]);
    app.handle(crate::app::Action::EditRemote);
    assert_eq!(app.prompt.as_ref().unwrap().text, "Title");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.prompt.as_ref().unwrap().text, "Old\nbody");
    app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    app.handle_paste("new literal $(not a command)\nsecond line");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
    app.handle_paste("third line");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.prompt.as_ref().unwrap().text, "main");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        !app.message_is_error && app.prompt.is_none(),
        "{}",
        app.message
    );
    assert!(
        mock.calls()[2].contains(&"new literal $(not a command)\nsecond line\nthird line".into())
    );
}
#[test]
fn tui_merge_requires_number_and_method_and_preserves_head_pin() {
    let (_repo, mut app) = selected_app();
    let mock = Mock::new(["merged".into(), "[]".into()]);
    app.handle(crate::app::Action::MergePr);
    press(&mut app, "7");
    assert!(mock.calls().is_empty());
    assert!(app.prompt.as_ref().unwrap().labels[1].contains("Merge method"));
    press(&mut app, "rebase");
    assert!(!app.message_is_error, "{}", app.message);
    assert_eq!(&mock.calls()[0][3], "--rebase");
    assert!(mock.calls()[0].contains(&"a".repeat(40)));
}
#[test]
fn invalid_filter_submission_keeps_previous_list_and_never_calls_provider() {
    let (_repo, mut app) = selected_app();
    let mock = Mock::new([]);
    app.handle(crate::app::Action::SetUpstream);
    app.handle_key(crate::keys::KeyEvent::new(
        crate::keys::KeyCode::Char('u'),
        crate::keys::KeyModifiers::CONTROL,
    ));
    press(&mut app, "invalid");
    press(&mut app, "query");
    assert!(
        app.message_is_error
            && app.pr_filter.state == "open"
            && app.pull_requests.items[0].number == 7
    );
    assert!(mock.calls().is_empty());
}

#[test]
fn tui_repository_creation_requires_typed_confirmation() {
    let (_repo, mut app) = selected_app();
    app.view = crate::model::ViewMode::GitHubRepositories;
    // A cancelled form never reaches GitHub; a confirmed one creates, then reloads the list.
    let mock = Mock::new(["created".into(), "[]".into()]);
    app.handle(crate::app::Action::New);
    press(&mut app, "me/tool");
    press(&mut app, "private");
    press(&mut app, "origin");
    press(&mut app, "later");
    assert!(app.message_is_error, "{}", app.message);
    assert!(mock.calls().is_empty());
    app.handle(crate::app::Action::New);
    for field in ["me/tool", "private", "", "create"] {
        press(&mut app, field);
    }
    assert!(!app.message_is_error, "{}", app.message);
    assert_eq!(mock.calls()[0], ["repo", "create", "me/tool", "--private"]);
    assert_eq!(mock.calls()[1][..2], ["repo", "list"]);
}

#[test]
fn pr_checkout_refuses_dirty_work_and_merge_requires_selected_number() {
    let repo = TestRepo::new();
    repo.write("file", "unsaved");
    let mut app = App::new(repo.open()).unwrap();
    app.set_view(ViewMode::PullRequests);
    app.pull_requests.items.push(PullRequest {
        number: 999999,
        title: "Fixture".into(),
        url: "https://github.com/o/r/pull/999999".into(),
        head_ref_name: "topic".into(),
        base_ref_name: "main".into(),
        head_ref_oid: "abc".into(),
        is_draft: false,
    });
    app.handle(Action::SwitchBranch);
    assert!(app.message().contains("Commit or stash"));
    assert_eq!(
        std::fs::read_to_string(repo.path.join("file")).unwrap(),
        "unsaved"
    );
    app.handle(Action::MergePr);
    press(&mut app, "1");
    press(&mut app, "squash");
    assert!(app.message().contains("Merge cancelled"));
    app.handle(Action::CommentPr);
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.prompt().is_none());
}
