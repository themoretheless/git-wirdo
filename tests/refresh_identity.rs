mod support;
use git_wirdo::{
    app::{Action, App},
    model::ViewMode,
};
use support::TestRepo;
#[test]
fn external_insertion_keeps_selected_file_and_unchanged_viewport() {
    let repo = TestRepo::new();
    repo.write("b", &"target\n".repeat(60));
    repo.write("c", "other\n");
    let mut app = App::new(repo.open()).unwrap();
    app.handle(Action::PageDown);
    app.handle(Action::ScrollRight);
    let viewport = (app.detail_scroll(), app.detail_column());
    repo.write("a", "inserted\n");
    app.refresh().unwrap();
    assert_eq!(
        app.displayed_files()[app.file_selection()].path.to_str(),
        Some("b")
    );
    assert_eq!((app.detail_scroll(), app.detail_column()), viewport);
    app.handle(Action::Stage);
    assert_eq!(repo.git(&["diff", "--cached", "--name-only"]).trim(), "b");
    assert_eq!(
        std::fs::read_to_string(repo.path.join("a")).unwrap(),
        "inserted\n"
    );
}
#[test]
fn history_refresh_keeps_commit_after_external_commit() {
    let repo = TestRepo::new();
    repo.write("file", "one\n");
    repo.commit_all("first");
    let mut app = App::new(repo.open()).unwrap();
    app.set_view(ViewMode::History);
    let sha = app.state().commits[0].sha.clone();
    repo.write("file", "two\n");
    repo.commit_all("second");
    app.refresh().unwrap();
    assert_eq!(app.state().commits[app.history_selection()].sha, sha);
}
#[test]
fn failed_refresh_preserves_the_entire_visible_snapshot() {
    let repo = TestRepo::new();
    repo.write("file", "one\n");
    repo.commit_all("base");
    repo.git(&["branch", "--set-upstream-to=main"]);
    repo.git(&["switch", "-c", "topic"]);
    repo.git(&["branch", "--set-upstream-to=main"]);
    repo.write("file", "pending\n");
    let mut app = App::new(repo.open()).unwrap();
    app.handle(Action::ToggleComparison);
    let files = app.state().files.clone();
    let detail = app.detail_text().to_owned();
    repo.git(&["branch", "-D", "main"]);
    repo.write("new", "outside\n");
    let index = repo.git(&["ls-files", "--stage"]);
    assert!(app.refresh().is_err());
    assert_eq!(app.state().files, files);
    assert_eq!(app.detail_text(), detail);
    assert_eq!(repo.git(&["ls-files", "--stage"]), index);
}

#[test]
fn changed_or_removed_detail_resets_viewport_and_clamps_selection() {
    let repo = TestRepo::new();
    repo.write("a", &"old\n".repeat(60));
    repo.write("b", "other\n");
    let mut app = App::new(repo.open()).unwrap();
    app.handle(Action::PageDown);
    repo.write("a", "short\n");
    app.refresh().unwrap();
    assert_eq!(app.detail_scroll(), 0);
    app.handle(Action::NextItem);
    std::fs::remove_file(repo.path.join("b")).unwrap();
    app.refresh().unwrap();
    assert_eq!(app.file_selection(), 0);
    assert!(app.detail_text().contains("short"));
}

#[test]
fn horizontal_bounds_use_terminal_cells_for_wide_and_combining_characters() {
    let repo = TestRepo::new();
    repo.write("file", &format!("{}TAIL", "界".repeat(40)));
    let mut app = App::new(repo.open()).unwrap();
    for _ in 0..20 {
        app.handle(Action::ScrollRight);
    }
    assert_eq!(app.detail_column(), 84);
    repo.write("file", &"e\u{301}".repeat(50));
    app.refresh().unwrap();
    for _ in 0..20 {
        app.handle(Action::ScrollRight);
    }
    assert_eq!(app.detail_column(), 50);
}

#[test]
fn branch_and_tag_insertions_keep_existing_selection() {
    let repo = TestRepo::new();
    repo.write("file", "base\n");
    repo.commit_all("base");
    repo.git(&["tag", "z"]);
    let mut app = App::new(repo.open()).unwrap();
    app.set_view(ViewMode::Branches);
    repo.git(&["branch", "a"]);
    app.refresh().unwrap();
    assert_eq!(app.state().branches[app.branch_selection()].name, "main");
    app.set_view(ViewMode::Tags);
    app.refresh().unwrap();
    repo.git(&["tag", "a"]);
    app.refresh().unwrap();
    assert_eq!(app.tags().selected().unwrap().name, "z");
}

#[cfg(target_os = "linux")]
#[test]
fn refresh_identity_retains_non_utf8_filename_bytes() {
    use std::os::unix::ffi::OsStringExt;
    let repo = TestRepo::new();
    let name = std::ffi::OsString::from_vec(b"b\xff".to_vec());
    repo.write(&name, "target\n");
    let mut app = App::new(repo.open()).unwrap();
    repo.write("a", "inserted\n");
    app.refresh().unwrap();
    assert_eq!(
        app.displayed_files()[app.file_selection()].path,
        std::path::PathBuf::from(name)
    );
}
