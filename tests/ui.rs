mod support;

use git_wirdo::app::{Action, App};
use git_wirdo::ui::Ui;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use support::TestRepo;

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
fn header_and_action_message_fit_inside_their_borders() {
    let fixture = TestRepo::new();
    let mut app = App::new(fixture.open()).unwrap();
    let rendered = screen(&app, 120, 24);
    assert!(rendered.contains("View: Files"));
    assert!(rendered.contains("Conflicts: 0"));
    assert!(rendered.contains("Action: Ready"));
    app.handle(Action::Commit);
    let rendered = screen(&app, 120, 24);
    assert!(rendered.contains("Action: Git failed"));
    assert!(rendered.contains("Action failed"));
}

#[test]
fn list_scrolls_to_keep_the_selection_visible() {
    let fixture = TestRepo::new();
    for index in 0..40 {
        fixture.write(format!("file-{index:02}"), "content\n");
    }
    let mut app = App::new(fixture.open()).unwrap();
    app.file_selection = 39;
    let rendered = screen(&app, 100, 24);
    assert!(rendered.contains("> [ U] [??] file-39"));
    assert!(!rendered.contains("file-00"));
}

#[test]
fn small_terminals_have_a_safe_fallback() {
    let fixture = TestRepo::new();
    let app = App::new(fixture.open()).unwrap();
    assert!(screen(&app, 40, 10).contains("Terminal too small"));
}
