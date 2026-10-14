//! White-box tests that place GitHub data in `App` directly, since it normally
//! arrives from the GitHub CLI.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use super::{Action, App};
use crate::model::ViewMode;
use crate::test_support::TestRepo;
use crate::ui::Ui;

fn screen(app: &App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    let mut ui = Ui::default();
    terminal.draw(|frame| ui.draw(frame, app)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn type_text(app: &mut App, value: &str) {
    for c in value.chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
}

#[test]
fn pr_file_view_shows_two_line_numbers_and_inline_comment_target() {
    use crate::{github::PullRequest, model::ViewMode, pr_review::PrFile};
    let r = TestRepo::new();
    let mut app = App::new(r.open()).unwrap();
    app.set_view(ViewMode::PrFiles);
    app.review_pr = Some(PullRequest {
        number: 7,
        title: "Review".into(),
        url: "url".into(),
        head_ref_name: "topic".into(),
        base_ref_name: "main".into(),
        head_ref_oid: "a".repeat(40),
        is_draft: false,
    });
    app.pr_files.items.push(PrFile {
        filename: "file.rs".into(),
        sha: "blob".into(),
        status: "modified".into(),
        previous_filename: None,
        patch: Some("@@ -4 +4 @@\n-old\n+new".into()),
    });
    app.handle(Action::NextItem);
    let rendered = screen(&app, 180, 30);
    assert!(
        rendered.contains("View: PrFiles")
            && rendered.contains("PR #7 file.rs")
            && rendered.contains("Old line | New line")
    );
    app.handle(Action::CommentPr);
    assert!(screen(&app, 180, 30).contains("Diff side: LEFT or RIGHT"));
}

#[test]
fn pr_checkout_refuses_dirty_work_and_merge_requires_selected_number() {
    let repo = TestRepo::new();
    repo.write("file", "unsaved");
    let mut app = App::new(repo.open()).unwrap();
    app.set_view(ViewMode::PullRequests);
    app.pull_requests.items.push(crate::github::PullRequest {
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
    type_text(&mut app, "1");
    type_text(&mut app, "squash");
    assert!(app.message().contains("Merge cancelled"));
    app.handle(Action::CommentPr);
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.prompt().is_none());
}
