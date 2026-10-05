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
fn recent_navigation_and_settings_errors_are_visible_without_path_controls() {
    let repo = TestRepo::new();
    let mut app = App::new(repo.open()).unwrap();
    app.handle(Action::RecentRepositories);
    app.persistence_error = Some("Corrupt settings; saving disabled".into());
    let rendered = screen(&app, 180, 28);
    assert!(rendered.contains("View: RecentRepositories"));
    assert!(rendered.contains("D forget entry"));
    assert!(rendered.contains("Corrupt settings; saving disabled"));
    assert!(rendered.contains("Saved view: Files"));
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
    for c in "empty commit".chars() {
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(c),
            crossterm::event::KeyModifiers::NONE,
        ));
    }
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    ));
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

#[test]
fn diff_paging_and_seen_progress_are_visible() {
    let fixture = TestRepo::new();
    fixture.write(
        "file",
        &(0..40)
            .map(|i| format!("unique-line-{i:02}\n"))
            .collect::<String>(),
    );
    let mut app = App::new(fixture.open()).unwrap();
    app.handle(Action::ToggleSeen);
    let rendered = screen(&app, 120, 24);
    assert!(rendered.contains("Seen 1/1"));
    assert!(rendered.contains("[seen]"));
    app.handle(Action::PageDown);
    let rendered = screen(&app, 120, 24);
    assert!(rendered.contains("unique-line-09"));
    assert!(!rendered.contains("unique-line-00"));
}

#[test]
fn workspaces_and_pr_prompts_render_inside_existing_tui() {
    let fixture = TestRepo::new();
    let mut app = App::new(fixture.open()).unwrap();
    app.view = git_wirdo::model::ViewMode::Workspaces;
    app.refresh().unwrap();
    let rendered = screen(&app, 120, 24);
    assert!(rendered.contains("View: Workspaces") && rendered.contains("N create"));
    app.handle(Action::New);
    assert!(screen(&app, 120, 24).contains("Workspace path"));
    app.prompt = None;
    app.view = git_wirdo::model::ViewMode::PullRequests;
    app.handle(Action::New);
    assert!(screen(&app, 120, 24).contains("Draft PR title"));
}

#[test]
fn stash_view_and_confirmation_render() {
    let fixture = TestRepo::new();
    fixture.write("file", "base");
    fixture.commit_all("base");
    fixture.write("file", "saved change");
    fixture.open().save_stash("UI stash").unwrap();
    let mut app = App::new(fixture.open()).unwrap();
    app.view = git_wirdo::model::ViewMode::Stashes;
    app.refresh().unwrap();
    let rendered = screen(&app, 120, 24);
    assert!(rendered.contains("stash@{0}") && rendered.contains("UI stash"));
    app.handle(Action::PageDown);
    assert!(screen(&app, 120, 24).contains("saved change"));
    app.handle(Action::Remove);
    assert!(screen(&app, 120, 24).contains("Type drop"));
}

#[test]
fn remote_views_show_urls_and_tracking_errors() {
    let fixture = TestRepo::new();
    fixture
        .open()
        .add_remote("origin", "https://example.invalid/repo.git")
        .unwrap();
    let mut app = App::new(fixture.open()).unwrap();
    app.view = git_wirdo::model::ViewMode::Remotes;
    app.refresh().unwrap();
    let rendered = screen(&app, 140, 28);
    assert!(rendered.contains("Fetch URLs") && rendered.contains("example.invalid"));
    app.tracking.error = Some("upstream missing".into());
    assert!(screen(&app, 140, 28).contains("Tracking unavailable: upstream missing"));
    app.handle(Action::New);
    assert!(screen(&app, 140, 28).contains("Remote name"));
}

#[test]
fn tags_and_create_prompt_are_visible() {
    let fixture = TestRepo::new();
    fixture.write("file", "base");
    fixture.commit_all("base");
    fixture.open().create_tag("v1", "HEAD", "").unwrap();
    let mut app = App::new(fixture.open()).unwrap();
    app.view = git_wirdo::model::ViewMode::Tags;
    app.refresh().unwrap();
    assert!(screen(&app, 140, 28).contains("v1 [commit]"));
    app.handle(Action::New);
    assert!(screen(&app, 140, 28).contains("Tag name"));
}

#[test]
fn hunk_view_and_discard_scope_are_visible() {
    let fixture = TestRepo::new();
    fixture.write("file", "base\n");
    fixture.commit_all("base");
    fixture.write("file", "changed\n");
    let mut app = App::new(fixture.open()).unwrap();
    app.handle(Action::OpenHunks);
    let rendered = screen(&app, 140, 28);
    assert!(rendered.contains("Unstaged hunk 1") && rendered.contains("+changed"));
    app.view = git_wirdo::model::ViewMode::Files;
    app.handle(Action::RestoreFromIndex);
    let rendered = screen(&app, 180, 28);
    assert!(rendered.contains("Type discard") && rendered.contains("staged changes retained"));
}

#[test]
fn reset_consequences_and_reflog_recovery_are_visible() {
    use git_wirdo::model::ViewMode;
    let fixture = TestRepo::new();
    fixture.write("file", "base");
    fixture.commit_all("base");
    let mut app = App::new(fixture.open()).unwrap();
    app.view = ViewMode::History;
    app.handle(Action::ResetCommit);
    let rendered = screen(&app, 180, 30);
    assert!(rendered.contains("Reset mode: soft, mixed, hard"));
    assert!(rendered.contains("hard discards tracked changes"));
    app.prompt = None;
    app.view = ViewMode::Reflog;
    app.refresh().unwrap();
    let rendered = screen(&app, 120, 24);
    assert!(rendered.contains("View: Reflog") && rendered.contains("HEAD@{"));
    app.handle(Action::New);
    assert!(screen(&app, 120, 24).contains("New recovery branch name"));
}

#[test]
fn pr_file_view_shows_two_line_numbers_and_inline_comment_target() {
    use git_wirdo::{github::PullRequest, model::ViewMode, pr_review::PrFile};
    let r = TestRepo::new();
    let mut app = App::new(r.open()).unwrap();
    app.view = ViewMode::PrFiles;
    app.review_pr = Some(PullRequest {
        number: 7,
        title: "Review".into(),
        url: "url".into(),
        head_ref_name: "topic".into(),
        base_ref_name: "main".into(),
        head_ref_oid: "a".repeat(40),
        is_draft: false,
    });
    app.pr_files.push(PrFile {
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
fn multiline_paste_is_input_text_and_large_paste_is_rejected_without_truncation() {
    let r = TestRepo::new();
    let mut app = App::new(r.open()).unwrap();
    app.handle_paste("qPcs");
    assert!(app.running && app.prompt.is_none());
    app.handle(Action::Commit);
    app.handle_paste("literal qPcs\nmessage");
    assert_eq!(app.prompt.as_ref().unwrap().text, "literal qPcs\nmessage");
    let before = app.prompt.as_ref().unwrap().text.clone();
    app.handle_paste(&"x".repeat(64 * 1024));
    assert!(app.message_is_error);
    assert_eq!(app.prompt.as_ref().unwrap().text, before);
    assert!(screen(&app, 180, 30).contains("literal qPcs↵message"));
}
