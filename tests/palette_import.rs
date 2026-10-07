mod support;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use git_wirdo::{
    app::{Action, App, PromptKind},
    commands,
    model::ViewMode,
};
use std::{path::PathBuf, process::Command};
use support::TestRepo;
fn key(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}
fn field(app: &mut App, text: &str) {
    app.handle_paste(text);
    key(app, KeyCode::Enter);
}
fn conflict_fixture() -> (TestRepo, PathBuf, String) {
    let source = TestRepo::new();
    source.write("file [a] ü.txt", "base\n");
    source.commit_all("base");
    let target = TestRepo::new();
    target.write("file [a] ü.txt", "base\n");
    target.commit_all("base");
    source.write("file [a] ü.txt", "patch side\n");
    source.commit_all("incoming commit\n\nmail body ü");
    let path = target.directory.0.join("incoming ü.patch");
    source.open().export_commit("HEAD", &path).unwrap();
    target.write("file [a] ü.txt", "local side\n");
    target.commit_all("local divergence");
    let head = target.git(&["rev-parse", "HEAD"]);
    (target, path, head)
}
#[test]
fn palette_filters_context_and_opens_existing_confirmed_forms_without_shortcut_execution() {
    let repo = TestRepo::new();
    let mut app = App::new(repo.open()).unwrap();
    app.handle(Action::CommandPalette);
    app.handle_paste("COMMIT staged");
    assert_eq!(commands::filtered(app.view, "COMMIT staged").len(), 1);
    key(&mut app, KeyCode::Enter);
    assert!(matches!(
        app.prompt.as_ref().unwrap().kind,
        PromptKind::Commit(false)
    ));
    key(&mut app, KeyCode::Esc);
    app.handle(Action::CommandPalette);
    app.handle_paste("commit");
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Enter);
    assert!(matches!(
        app.prompt.as_ref().unwrap().kind,
        PromptKind::Commit(true)
    ));
    key(&mut app, KeyCode::Esc);
    app.handle(Action::CommandPalette);
    field(&mut app, "view commit history");
    assert_eq!(app.view, ViewMode::History);
    assert!(
        commands::filtered(app.view, "export")
            .iter()
            .any(|e| e.action == Action::ExportCommit)
    );
    assert!(commands::filtered(ViewMode::Files, "export").is_empty());
    app.handle(Action::CommandPalette);
    field(&mut app, "qPcs $(echo untouched)");
    assert!(app.running && app.message_is_error);
    assert!(repo.git(&["status", "--porcelain"]).is_empty());
    assert!(
        commands::filtered(ViewMode::PullRequests, "upstream")
            .iter()
            .all(|e| e.action != Action::SetUpstream)
    );
    assert_eq!(
        commands::filtered(ViewMode::Tags, "publish")[0].title,
        "Publish selected tag"
    );
}
#[test]
fn failed_import_is_mail_operation_not_rebase_and_can_abort_or_resolve_then_continue() {
    for abort in [true, false] {
        let (repo, path, before) = conflict_fixture();
        assert!(repo.open().import_mail_patch(&path).is_err());
        let state = repo.open().load_state().unwrap();
        assert!(state.merge_state.am_in_progress && !state.merge_state.rebase_in_progress);
        assert_eq!(state.merge_state.conflicts.len(), 1);
        assert!(
            repo.open()
                .current_mail_patch()
                .unwrap()
                .contains("incoming commit")
        );
        assert!(repo.open().skip_rebase().is_err());
        assert!(repo.open().continue_operation().is_err());
        if abort {
            repo.open().abort_operation().unwrap();
            assert_eq!(repo.git(&["rev-parse", "HEAD"]), before);
            assert_eq!(
                std::fs::read_to_string(repo.path.join("file [a] ü.txt")).unwrap(),
                "local side\n"
            );
        } else {
            repo.write("file [a] ü.txt", "resolved side\n");
            repo.open()
                .mark_resolved(std::path::Path::new("file [a] ü.txt"))
                .unwrap();
            repo.open().continue_operation().unwrap();
            assert!(
                repo.git(&["log", "-1", "--format=%B"])
                    .contains("mail body ü")
            );
            assert_eq!(
                std::fs::read_to_string(repo.path.join("file [a] ü.txt")).unwrap(),
                "resolved side\n"
            );
        }
        assert!(!repo.open().load_state().unwrap().merge_state.in_progress());
    }
}
#[test]
fn cli_import_preserves_authorship_and_dirty_refusal_does_not_touch_files_or_refs() {
    let source = TestRepo::new();
    source.git(&["config", "user.name", "Original Mail Author"]);
    source.git(&["config", "user.email", "original-mail@example.invalid"]);
    source.write("file", "mail content");
    source.commit_all("root mail commit");
    let path = source.directory.0.join("mail.patch");
    source.open().export_commit("HEAD", &path).unwrap();
    let target = TestRepo::new();
    let output = Command::new(env!("CARGO_BIN_EXE_git-wirdo"))
        .arg("--repo")
        .arg(&target.path)
        .arg("--import-patch")
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(target.path.join("file")).unwrap(),
        "mail content"
    );
    assert_eq!(
        target.git(&["log", "-1", "--format=%an <%ae>"]),
        source.git(&["log", "-1", "--format=%an <%ae>"])
    );
    target.write("file", "pending");
    let head = target.git(&["rev-parse", "HEAD"]);
    assert!(target.open().import_mail_patch(&path).is_err());
    assert_eq!(target.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(
        std::fs::read_to_string(target.path.join("file")).unwrap(),
        "pending"
    );
    let mut app = App::new(target.open()).unwrap();
    app.handle(Action::ImportPatch);
    field(&mut app, path.to_str().unwrap());
    field(&mut app, "no");
    assert!(app.message_is_error);
    assert_eq!(target.git(&["rev-parse", "HEAD"]), head);
}

#[test]
fn apply_backend_rebase_is_not_misclassified_as_mail_import() {
    let repo = TestRepo::new();
    repo.write("file", "base\n");
    repo.commit_all("base");
    repo.git(&["switch", "-c", "topic"]);
    repo.write("file", "topic\n");
    repo.commit_all("topic");
    let head = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&["switch", "main"]);
    repo.write("file", "main\n");
    repo.commit_all("main");
    repo.git(&["switch", "topic"]);
    let output = Command::new("git")
        .arg("-C")
        .arg(&repo.path)
        .args(["rebase", "--apply", "main"])
        .env("GIT_EDITOR", "true")
        .env("GIT_SEQUENCE_EDITOR", "true")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let state = repo.open().load_state().unwrap().merge_state;
    assert!(state.rebase_in_progress && !state.am_in_progress);
    repo.open().abort_operation().unwrap();
    assert_eq!(repo.git(&["rev-parse", "HEAD"]), head);
}

#[test]
fn aborting_a_partly_applied_mail_series_restores_the_pre_import_head_and_files() {
    let source = TestRepo::new();
    source.write("file", "base\n");
    source.commit_all("base");
    source.write("first", "first patch\n");
    source.commit_all("first accepted patch");
    source.write("file", "incoming\n");
    source.commit_all("second conflicting patch");
    let target = TestRepo::new();
    target.write("file", "base\n");
    target.commit_all("base");
    target.write("file", "local\n");
    target.commit_all("local");
    let before = target.git(&["rev-parse", "HEAD"]);
    let patch = target.directory.0.join("series.mbox");
    std::fs::write(
        &patch,
        source.git(&["format-patch", "--stdout", "-2", "HEAD"]),
    )
    .unwrap();
    assert!(target.open().import_mail_patch(&patch).is_err());
    assert!(target.path.join("first").exists());
    assert_ne!(target.git(&["rev-parse", "HEAD"]), before);
    assert!(
        target
            .open()
            .load_state()
            .unwrap()
            .merge_state
            .am_in_progress
    );
    target.open().abort_operation().unwrap();
    assert_eq!(target.git(&["rev-parse", "HEAD"]), before);
    assert!(!target.path.join("first").exists());
    assert_eq!(
        std::fs::read_to_string(target.path.join("file")).unwrap(),
        "local\n"
    );
}

#[test]
fn palette_hunk_navigation_does_not_stage_a_different_file_from_the_tracked_browser() {
    let repo = TestRepo::new();
    repo.write("aaa-clean", "clean\n");
    repo.write("zzz-dirty", "base\n");
    repo.commit_all("base");
    repo.write("zzz-dirty", "pending\n");
    let mut app = App::new(repo.open()).unwrap();
    app.handle(Action::BrowseTracked);
    assert_eq!(
        app.displayed_files()[app.file_selection].path,
        std::path::Path::new("aaa-clean")
    );
    app.handle(Action::CommandPalette);
    field(&mut app, "view hunks");
    assert_eq!(app.view, ViewMode::Hunks);
    assert!(app.hunks.is_empty());
    app.handle(Action::Stage);
    assert!(repo.git(&["diff", "--cached"]).is_empty());
}
