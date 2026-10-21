mod support;
use git_wirdo::keys::{KeyCode, KeyEvent, KeyModifiers};
use git_wirdo::{
    app::{Action, App},
    model::ViewMode,
};
use support::TestRepo;
fn key(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}
fn filter(app: &mut App, text: &str) {
    app.handle(Action::FilterList);
    app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    app.handle_paste(text);
    key(app, KeyCode::Enter);
}

#[test]
fn live_filters_are_independent_and_shortcuts_are_text() {
    let repo = TestRepo::new();
    repo.write("base", "base");
    repo.commit_all("base");
    repo.write("aaa", "a");
    repo.write("zzz", "z");
    repo.git(&["branch", "topic"]);
    let mut app = App::new(repo.open()).unwrap();
    app.handle(Action::FilterList);
    app.handle_paste("zzz");
    assert_eq!(app.visible_indices(), vec![1]);
    assert_eq!(app.name_filter(), "zzz");
    assert!(repo.git(&["diff", "--cached"]).is_empty());
    key(&mut app, KeyCode::Esc);
    app.set_view(ViewMode::Branches);
    filter(&mut app, "topic");
    assert_eq!(app.visible_indices().len(), 1);
    app.set_view(ViewMode::Files);
    assert_eq!(app.name_filter(), "zzz");
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.visible_indices().len(), 2);
    app.set_view(ViewMode::Branches);
    assert_eq!(app.name_filter(), "topic");
}

#[test]
fn line_cursor_range_stages_only_selected_changes_and_unstages() {
    let repo = TestRepo::new();
    repo.write("file", "one\ntwo\nthree\n");
    repo.commit_all("base");
    repo.write("file", "ONE\ntwo\nTHREE\n");
    let mut app = App::new(repo.open()).unwrap();
    app.handle(Action::OpenHunks);
    key(&mut app, KeyCode::Char(' '));
    assert!(app.line_editor_open());
    key(&mut app, KeyCode::Char('v'));
    key(&mut app, KeyCode::Down);
    assert_eq!(app.selected_line_numbers(), vec![1, 2]);
    key(&mut app, KeyCode::Char('s'));
    assert_eq!(repo.git(&["show", ":file"]), "ONE\ntwo\nthree\n");
    app.handle(Action::ToggleComparison);
    key(&mut app, KeyCode::Char(' '));
    key(&mut app, KeyCode::Char('v'));
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Char('u'));
    assert!(repo.git(&["diff", "--cached"]).is_empty());
    assert_eq!(
        std::fs::read_to_string(repo.path.join("file")).unwrap(),
        "ONE\ntwo\nTHREE\n"
    );
}

#[test]
fn stale_visual_lines_are_refused_and_cancel_changes_nothing() {
    let repo = TestRepo::new();
    repo.write("file", "base\n");
    repo.commit_all("base");
    repo.write("file", "change\n");
    let mut app = App::new(repo.open()).unwrap();
    app.handle(Action::OpenHunks);
    key(&mut app, KeyCode::Char(' '));
    key(&mut app, KeyCode::Char(' '));
    key(&mut app, KeyCode::Esc);
    assert!(repo.git(&["diff", "--cached"]).is_empty());
    key(&mut app, KeyCode::Char(' '));
    key(&mut app, KeyCode::Char(' '));
    repo.write("file", "external\n");
    key(&mut app, KeyCode::Char('s'));
    assert!(app.message_is_error());
    assert!(repo.git(&["diff", "--cached"]).is_empty());
}

#[test]
fn visual_rebase_reorders_previews_and_requires_confirmation() {
    let repo = TestRepo::new();
    repo.write("base", "base");
    repo.commit_all("base");
    repo.write("a", "a");
    repo.commit_all("first");
    repo.write("b", "b");
    repo.commit_all("second");
    let head = repo.git(&["rev-parse", "HEAD"]);
    let mut app = App::new(repo.open()).unwrap();
    app.set_view(ViewMode::History);
    app.select(2);
    app.handle(Action::InteractiveRebase);
    assert!(app.rebase_editor_open());
    key(&mut app, KeyCode::Char('J'));
    key(&mut app, KeyCode::Char('f'));
    key(&mut app, KeyCode::Char('v'));
    assert!(
        app.rebase_preview()
            .unwrap()
            .contains("2 original commits -> 1 resulting commits")
    );
    key(&mut app, KeyCode::Enter);
    assert_eq!(repo.git(&["rev-parse", "HEAD"]), head);
    key(&mut app, KeyCode::Esc);
    assert!(app.rebase_editor_open());
    key(&mut app, KeyCode::Enter);
    app.handle_paste("rebase");
    key(&mut app, KeyCode::Enter);
    assert!(!app.message_is_error(), "{}", app.message());
    assert!(!app.rebase_editor_open());
    assert_eq!(repo.git(&["log", "-1", "--format=%s"]).trim(), "second");
    assert_eq!(repo.git(&["rev-list", "--count", "HEAD"]).trim(), "2");
}

#[test]
fn selected_stash_excludes_other_staged_files_and_restores_index_and_untracked() {
    let repo = TestRepo::new();
    repo.write("selected", "base\n");
    repo.write("other", "base\n");
    repo.commit_all("base");
    repo.write("selected", "staged\n");
    repo.git(&["add", "selected"]);
    repo.write("selected", "working\n");
    repo.write("other", "other staged\n");
    repo.git(&["add", "other"]);
    repo.write("other", "other working\n");
    repo.write("literal [new] ü", "untracked\n");
    let git = repo.open();
    let files = git
        .load_state()
        .unwrap()
        .files
        .into_iter()
        .filter(|f| f.path.to_str() != Some("other"))
        .collect::<Vec<_>>();
    let request = git.prepare_stash_files(&files).unwrap();
    git.save_stash_files(&request, "selected only").unwrap();
    assert_eq!(repo.git(&["show", ":other"]), "other staged\n");
    assert_eq!(
        std::fs::read_to_string(repo.path.join("other")).unwrap(),
        "other working\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.path.join("selected")).unwrap(),
        "base\n"
    );
    assert!(!repo.path.join("literal [new] ü").exists());
    assert_eq!(
        repo.git(&["diff", "stash@{0}^1", "stash@{0}", "--name-only"])
            .trim(),
        "selected"
    );
    assert_eq!(
        repo.git(&["diff", "stash@{0}^1", "stash@{0}^2", "--name-only"])
            .trim(),
        "selected"
    );
    repo.git(&[
        "restore",
        "--source=HEAD",
        "--staged",
        "--worktree",
        "other",
    ]);
    let stash = git.stashes().unwrap().remove(0);
    git.apply_stash(&stash, false).unwrap();
    assert_eq!(repo.git(&["show", ":selected"]), "staged\n");
    assert_eq!(
        std::fs::read_to_string(repo.path.join("selected")).unwrap(),
        "working\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.path.join("literal [new] ü")).unwrap(),
        "untracked\n"
    );
}

#[test]
fn selected_stash_handles_rename_deletion_and_new_staged_file() {
    let repo = TestRepo::new();
    repo.write("before", "rename\n");
    repo.write("deleted", "delete\n");
    repo.commit_all("base");
    repo.git(&["mv", "before", "after"]);
    repo.git(&["rm", "deleted"]);
    repo.write("added", "new\n");
    repo.git(&["add", "added"]);
    let git = repo.open();
    let files = git.load_state().unwrap().files;
    let request = git.prepare_stash_files(&files).unwrap();
    git.save_stash_files(&request, "structural").unwrap();
    assert!(git.load_state().unwrap().files.is_empty());
    assert!(repo.path.join("before").exists());
    assert!(repo.path.join("deleted").exists());
    assert!(!repo.path.join("added").exists());
    git.apply_stash(&git.stashes().unwrap()[0], false).unwrap();
    assert!(repo.path.join("after").exists());
    assert!(repo.path.join("added").exists());
    assert!(!repo.path.join("deleted").exists());
}

#[test]
fn stale_stash_request_keeps_index_files_and_refs() {
    let repo = TestRepo::new();
    repo.write("file", "base");
    repo.commit_all("base");
    repo.write("file", "first");
    let git = repo.open();
    let request = git
        .prepare_stash_files(&git.load_state().unwrap().files)
        .unwrap();
    repo.write("file", "external");
    assert!(git.save_stash_files(&request, "stale").is_err());
    assert!(git.stashes().unwrap().is_empty());
    assert_eq!(
        std::fs::read_to_string(repo.path.join("file")).unwrap(),
        "external"
    );
    assert!(repo.git(&["diff", "--cached"]).is_empty());
}

#[derive(Debug)]
struct FailVerb(&'static str);
impl git_wirdo::git::GitRunner for FailVerb {
    fn output(
        &self,
        command: &mut std::process::Command,
        input: Option<&[u8]>,
    ) -> anyhow::Result<std::process::Output> {
        if command.get_args().any(|arg| arg == self.0) {
            anyhow::bail!("injected failure in {}", self.0);
        }
        git_wirdo::git::GitRunner::output(&git_wirdo::git::ProcessRunner, command, input)
    }
}

#[test]
fn stash_failure_before_and_after_store_preserves_work_and_cleans_private_index() {
    for verb in ["commit-tree", "restore"] {
        let repo = TestRepo::new();
        repo.write("file", "base\n");
        repo.commit_all("base");
        repo.write("file", "staged\n");
        repo.git(&["add", "file"]);
        repo.write("file", "working\n");
        let head = repo.git(&["rev-parse", "HEAD"]);
        let index = repo.git(&["write-tree"]);
        let git =
            git_wirdo::git::Repository::open_with(&repo.path, std::sync::Arc::new(FailVerb(verb)))
                .unwrap();
        let request = git
            .prepare_stash_files(&git.load_state().unwrap().files)
            .unwrap();
        assert!(git.save_stash_files(&request, "recoverable").is_err());
        assert_eq!(repo.git(&["rev-parse", "HEAD"]), head);
        assert_eq!(repo.git(&["write-tree"]), index);
        assert_eq!(
            std::fs::read_to_string(repo.path.join("file")).unwrap(),
            "working\n"
        );
        assert_eq!(git.stashes().unwrap().is_empty(), verb == "commit-tree");
        assert!(
            !std::fs::read_dir(repo.path.join(".git")).unwrap().any(|e| e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("git-wirdo-stash-"))
        );
    }
}

#[test]
fn untracked_only_stash_and_staged_ignored_file_restore_exact_contents() {
    let repo = TestRepo::new();
    repo.write(".gitignore", "ignored\n");
    repo.commit_all("base");
    repo.write("untracked", "keep\n");
    let git = repo.open();
    let request = git
        .prepare_stash_files(&git.load_state().unwrap().files)
        .unwrap();
    git.save_stash_files(&request, "untracked only").unwrap();
    assert!(git.load_state().unwrap().files.is_empty());
    git.apply_stash(&git.stashes().unwrap()[0], true).unwrap();
    assert_eq!(
        std::fs::read_to_string(repo.path.join("untracked")).unwrap(),
        "keep\n"
    );
    std::fs::remove_file(repo.path.join("untracked")).unwrap();
    repo.write("ignored", "staged\n");
    repo.git(&["add", "--force", "ignored"]);
    let request = git
        .prepare_stash_files(&git.load_state().unwrap().files)
        .unwrap();
    git.save_stash_files(&request, "ignored but tracked")
        .unwrap();
    assert!(!repo.path.join("ignored").exists());
    git.apply_stash(&git.stashes().unwrap()[0], false).unwrap();
    assert_eq!(repo.git(&["show", ":ignored"]), "staged\n");
}

#[test]
fn intent_to_add_is_refused_without_losing_the_index_flag() {
    let repo = TestRepo::new();
    repo.write("base", "base");
    repo.commit_all("base");
    repo.write("new", "pending");
    repo.git(&["add", "--intent-to-add", "new"]);
    let git = repo.open();
    let before = repo.git(&["status", "--porcelain"]);
    assert!(
        git.prepare_stash_files(&git.load_state().unwrap().files)
            .is_err()
    );
    assert_eq!(repo.git(&["status", "--porcelain"]), before);
    assert!(git.stashes().unwrap().is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn selected_untracked_stash_preserves_non_utf8_filename() {
    use std::os::unix::ffi::OsStringExt;
    let repo = TestRepo::new();
    repo.write("base", "base");
    repo.commit_all("base");
    let name = std::ffi::OsString::from_vec(b"raw-\xff".to_vec());
    std::fs::write(repo.path.join(&name), b"native bytes").unwrap();
    let git = repo.open();
    let request = git
        .prepare_stash_files(&git.load_state().unwrap().files)
        .unwrap();
    git.save_stash_files(&request, "native path").unwrap();
    assert!(!repo.path.join(&name).exists());
    git.apply_stash(&git.stashes().unwrap()[0], false).unwrap();
    assert_eq!(
        std::fs::read(repo.path.join(&name)).unwrap(),
        b"native bytes"
    );
}

#[test]
fn read_only_file_inspection_cannot_mark_or_open_selected_stash() {
    let repo = TestRepo::new();
    repo.write("file", "base\n");
    repo.commit_all("base");
    repo.write("file", "pending\n");
    for inspect in [
        Action::FileHistory,
        Action::FileBlame,
        Action::BrowseTracked,
    ] {
        let mut app = App::new(repo.open()).unwrap();
        app.handle(inspect);
        app.handle(Action::ToggleFileSelection);
        assert_eq!(app.selected_stash_file_count(), 0);
        app.handle(Action::StashSelected);
        assert!(app.prompt().is_none());
        assert!(repo.open().stashes().unwrap().is_empty());
        assert_eq!(
            std::fs::read_to_string(repo.path.join("file")).unwrap(),
            "pending\n"
        );
    }
}

#[test]
fn visual_reword_drop_and_edit_show_a_pause_then_continue() {
    let repo = TestRepo::new();
    repo.write("base", "base");
    repo.commit_all("base");
    for name in ["a", "b", "c"] {
        repo.write(name, name);
        repo.commit_all(name);
    }
    let mut app = App::new(repo.open()).unwrap();
    app.set_view(ViewMode::History);
    app.select(3);
    app.handle(Action::InteractiveRebase);
    key(&mut app, KeyCode::Char('r'));
    assert!(app.prompt().unwrap().text.contains('a'));
    app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    app.handle_paste("new a\n\nretained body");
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Char('j'));
    key(&mut app, KeyCode::Char('d'));
    key(&mut app, KeyCode::Char('j'));
    key(&mut app, KeyCode::Char('e'));
    key(&mut app, KeyCode::Char('v'));
    assert!(app.rebase_preview().unwrap().contains("DROP: b"));
    key(&mut app, KeyCode::Enter);
    app.handle_paste("rebase");
    key(&mut app, KeyCode::Enter);
    assert!(!app.message_is_error(), "{}", app.message());
    assert!(app.message().contains("paused"));
    assert_eq!(app.view(), ViewMode::Conflicts);
    assert!(app.state().merge_state.rebase_in_progress);
    assert!(
        repo.git(&["log", "--format=%B"])
            .contains("new a\n\nretained body")
    );
    assert!(!repo.path.join("b").exists());
    app.handle(Action::Continue);
    assert!(!app.message_is_error(), "{}", app.message());
    assert!(!app.state().merge_state.rebase_in_progress);
}
