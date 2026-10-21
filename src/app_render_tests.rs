//! White-box tests that place GitHub data in `App` directly, since it normally
//! arrives from the GitHub CLI.
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::app::{Action, App};
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

#[test]
fn pr_file_view_shows_two_line_numbers_and_inline_comment_target() {
    use crate::{github::PullRequest, model::ViewMode, pr_review::PrFile};
    let r = TestRepo::new();
    let mut app = App::new(r.open()).unwrap();
    app.set_view(ViewMode::PrFiles);
    app.set_review_pr(PullRequest {
        number: 7,
        title: "Review".into(),
        url: "url".into(),
        head_ref_name: "topic".into(),
        base_ref_name: "main".into(),
        head_ref_oid: "a".repeat(40),
        is_draft: false,
    });
    app.set_pr_files(vec![PrFile {
        filename: "file.rs".into(),
        sha: "blob".into(),
        status: "modified".into(),
        previous_filename: None,
        patch: Some("@@ -4 +4 @@\n-old\n+new".into()),
    }]);
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
