mod support;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use git_wirdo::app::{Action, App};
use git_wirdo::git::ResetMode;
use git_wirdo::model::ViewMode;
use support::TestRepo;

fn fixture() -> (TestRepo, String, String) {
    let r = TestRepo::new();
    r.write("file", "base\n");
    r.commit_all("base");
    let base = r.git(&["rev-parse", "HEAD"]).trim().to_owned();
    r.write("file", "second\n");
    r.commit_all("second");
    let head = r.git(&["rev-parse", "HEAD"]).trim().to_owned();
    (r, base, head)
}
fn submit(app: &mut App, text: &str) {
    for c in text.chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
}

#[test]
fn each_reset_mode_has_native_index_and_worktree_semantics_and_a_durable_backup() {
    for mode in [ResetMode::Soft, ResetMode::Mixed, ResetMode::Hard] {
        let (r, base, head) = fixture();
        r.write("file", "staged\n");
        r.git(&["add", "file"]);
        r.write("file", "working\n");
        r.write("untracked", "keep");
        let repo = r.open();
        let request = repo.prepare_reset(&base).unwrap();
        let backup = repo.reset_commit(&request, mode).unwrap();
        assert_eq!(r.git(&["rev-parse", &backup]).trim(), head);
        assert_eq!(r.git(&["rev-parse", "HEAD"]).trim(), base);
        let index = r.git(&["show", ":file"]);
        assert_eq!(
            index,
            if mode == ResetMode::Soft {
                "staged\n"
            } else {
                "base\n"
            }
        );
        let working = std::fs::read_to_string(r.path.join("file")).unwrap();
        assert_eq!(
            working,
            if mode == ResetMode::Hard {
                "base\n"
            } else {
                "working\n"
            }
        );
        assert_eq!(
            std::fs::read_to_string(r.path.join("untracked")).unwrap(),
            "keep"
        );
        assert!(repo.reflog(100).unwrap().iter().any(|e| e.sha == head));
        // The backup is a real reference: the commit survives reflog expiration and pruning.
        r.git(&["reflog", "expire", "--expire=now", "--all"]);
        r.git(&["gc", "--prune=now"]);
        assert_eq!(r.git(&["rev-parse", &backup]).trim(), head);
        assert!(
            r.git(&["show", &format!("{backup}:file")])
                .contains("second")
        );
    }
}

#[test]
fn reset_refuses_changed_content_index_head_or_branch_since_preview() {
    for change in 0..4 {
        let (r, base, head) = fixture();
        r.write("file", "working\n");
        let repo = r.open();
        let request = repo.prepare_reset(&base).unwrap();
        match change {
            0 => r.write("file", "newer working\n"),
            1 => {
                r.git(&["add", "file"]);
            }
            2 => {
                r.git(&["commit", "--allow-empty", "-m", "external"]);
            }
            _ => {
                r.git(&["switch", "-c", "external"]);
            }
        }
        let expected = r.git(&["rev-parse", "HEAD"]);
        assert!(repo.reset_commit(&request, ResetMode::Hard).is_err());
        assert_eq!(r.git(&["rev-parse", "HEAD"]), expected);
        if change != 2 {
            assert_eq!(expected.trim(), head);
        }
        assert!(
            r.git(&["for-each-ref", "refs/git-wirdo/recovery"])
                .is_empty()
        );
    }
}

#[test]
fn hard_reset_protects_untracked_and_ignored_obstructions_in_both_directions() {
    for (path, directory, ignored) in [
        ("collision", false, false),
        ("collision", false, true),
        ("collision", true, true),
        ("parent/child", false, true),
    ] {
        let r = TestRepo::new();
        r.write("base", "base");
        r.commit_all("base");
        let base = r.git(&["rev-parse", "HEAD"]).trim().to_owned();
        r.write(path, "target");
        r.commit_all("target");
        let target = r.git(&["rev-parse", "HEAD"]).trim().to_owned();
        r.git(&["reset", "--hard", &base]);
        let collision = if directory {
            "collision/precious"
        } else if path == "parent/child" {
            "parent"
        } else {
            path
        };
        if ignored {
            r.git(&[
                "config",
                "core.excludesFile",
                r.path.join(".git/ignore-test").to_str().unwrap(),
            ]);
            std::fs::write(r.path.join(".git/ignore-test"), "collision\nparent\n").unwrap();
        }
        r.write(collision, "precious");
        let repo = r.open();
        let request = repo.prepare_reset(&target).unwrap();
        assert!(repo.reset_commit(&request, ResetMode::Hard).is_err());
        assert_eq!(
            std::fs::read_to_string(r.path.join(collision)).unwrap(),
            "precious"
        );
        assert_eq!(r.git(&["rev-parse", "HEAD"]).trim(), base);
    }
}

#[test]
fn reflog_recovers_an_unreachable_commit_into_a_new_branch_without_switching() {
    let (r, base, lost) = fixture();
    r.git(&["reset", "--hard", &base]);
    r.write("file", "dirty\n");
    let repo = r.open();
    let entries = repo.reflog(100).unwrap();
    let entry = entries.iter().find(|e| e.sha == lost).unwrap();
    assert!(entry.selector.starts_with("HEAD@{") && entry.subject.contains("second"));
    repo.recover_commit(&entry.sha, "recovery/lost").unwrap();
    assert_eq!(r.git(&["rev-parse", "recovery/lost"]).trim(), lost);
    assert_eq!(r.git(&["branch", "--show-current"]).trim(), "main");
    assert_eq!(
        std::fs::read_to_string(r.path.join("file")).unwrap(),
        "dirty\n"
    );
    assert!(repo.recover_commit(&base, "recovery/lost").is_err());
    assert_eq!(r.git(&["rev-parse", "recovery/lost"]).trim(), lost);
    assert!(repo.recover_commit("--help", "another").is_err());
    assert!(repo.recover_commit(&lost, "--help").is_err());
}

#[test]
fn linked_worktree_and_detached_head_reflog_use_their_own_head() {
    let (r, base, head) = fixture();
    let path = r.directory.0.join("linked");
    r.open().create_workspace(&path, "linked").unwrap();
    let linked = git_wirdo::git::Repository::open(&path).unwrap();
    let req = linked.prepare_reset(&base).unwrap();
    linked.reset_commit(&req, ResetMode::Hard).unwrap();
    assert_eq!(r.git(&["rev-parse", "HEAD"]).trim(), head);
    assert!(linked.reflog(100).unwrap().iter().any(|e| e.sha == head));
    r.git(&["switch", "--detach", &head]);
    let req = r.open().prepare_reset(&base).unwrap();
    r.open().reset_commit(&req, ResetMode::Soft).unwrap();
    assert!(r.git(&["branch", "--show-current"]).trim().is_empty());
}

#[test]
fn reset_conflict_and_unborn_states_are_refused() {
    let r = support::merge_conflict();
    let sha = r.git(&["rev-parse", "HEAD"]).trim().to_owned();
    assert!(r.open().prepare_reset(&sha).is_err());
    let r = TestRepo::new();
    assert!(r.open().reflog(100).unwrap().is_empty());
    assert!(r.open().prepare_reset(&"a".repeat(40)).is_err());
}

#[test]
fn tui_requires_mode_and_full_sha_then_recovers_without_destroying_current_work() {
    let (r, base, head) = fixture();
    let mut app = App::new(r.open()).unwrap();
    app.view = ViewMode::History;
    app.history_selection = app
        .state
        .commits
        .iter()
        .position(|c| c.sha == base)
        .unwrap();
    app.handle(Action::ResetCommit);
    submit(&mut app, "hard");
    submit(&mut app, "wrong");
    assert!(app.message_is_error);
    assert_eq!(r.git(&["rev-parse", "HEAD"]).trim(), head);
    app.handle(Action::ResetCommit);
    submit(&mut app, "hard");
    submit(&mut app, &base);
    assert!(!app.message_is_error, "{}", app.message);
    assert!(app.message.contains("refs/git-wirdo/recovery"));
    app.view = ViewMode::Reflog;
    app.refresh().unwrap();
    app.reflog_selection = app.reflog.iter().position(|e| e.sha == head).unwrap();
    app.handle(Action::New);
    submit(&mut app, "saved");
    assert!(!app.message_is_error, "{}", app.message);
    assert_eq!(r.git(&["rev-parse", "saved"]).trim(), head);
    assert_eq!(r.git(&["rev-parse", "HEAD"]).trim(), base);
}

#[test]
fn reset_confirmation_tracks_content_hidden_by_assume_unchanged() {
    let (r, base, _) = fixture();
    r.git(&["update-index", "--assume-unchanged", "file"]);
    r.write("file", "hidden\n");
    assert!(r.open().load_state().unwrap().files.is_empty());
    let repo = r.open();
    let req = repo.prepare_reset(&base).unwrap();
    r.write("file", "new hidden\n");
    assert!(repo.reset_commit(&req, ResetMode::Hard).is_err());
    assert_eq!(
        std::fs::read_to_string(r.path.join("file")).unwrap(),
        "new hidden\n"
    );
}

#[test]
fn persistent_backups_are_listed_and_recoverable_after_head_reflog_expiration() {
    let (r, base, lost) = fixture();
    let repo = r.open();
    let req = repo.prepare_reset(&base).unwrap();
    let backup = repo.reset_commit(&req, ResetMode::Hard).unwrap();
    r.git(&["reflog", "expire", "--expire=now", "--all"]);
    let entries = repo.reflog(100).unwrap();
    let entry = entries.iter().find(|e| e.selector == backup).unwrap();
    assert_eq!(entry.sha, lost);
    repo.recover_commit(&entry.sha, "after-expiration").unwrap();
    assert_eq!(r.git(&["rev-parse", "after-expiration"]).trim(), lost);
}
