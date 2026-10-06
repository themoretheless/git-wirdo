mod support;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use git_wirdo::app::{Action, App};
use git_wirdo::model::ViewMode;
use support::TestRepo;

fn type_text(app: &mut App, value: &str) {
    for c in value.chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
}

#[test]
fn create_switch_and_remove_workspaces_preserves_dirty_files_and_branches() {
    let repo = TestRepo::new();
    repo.write("file", "base\n");
    repo.commit_all("base");
    let path = repo.directory.0.join("workspace with spaces");
    repo.open().create_workspace(&path, "topic").unwrap();
    let entries = repo.open().workspaces().unwrap();
    let w = entries.iter().find(|w| w.branch == "topic").unwrap();
    assert_eq!(w.path.canonicalize().unwrap(), path.canonicalize().unwrap());
    std::fs::write(path.join("file"), "keep me\n").unwrap();
    assert!(repo.open().remove_workspace(w).is_err());
    assert_eq!(
        std::fs::read_to_string(path.join("file")).unwrap(),
        "keep me\n"
    );
    repo.git(&["worktree", "lock", path.to_str().unwrap()]);
    assert!(
        repo.open()
            .workspaces()
            .unwrap()
            .iter()
            .find(|w| w.branch == "topic")
            .unwrap()
            .locked
    );
    repo.git(&["worktree", "unlock", path.to_str().unwrap()]);
    let mut app = App::new(repo.open()).unwrap();
    app.view = ViewMode::Workspaces;
    app.refresh().unwrap();
    app.workspace_selection = app
        .workspaces
        .iter()
        .position(|w| w.branch == "topic")
        .unwrap();
    app.handle(Action::SwitchBranch);
    assert_eq!(
        app.repository.root().canonicalize().unwrap(),
        path.canonicalize().unwrap()
    );
    assert!(app.detail_text.contains("keep me"));
    app.handle(Action::OpenRepository);
    type_text(&mut app, repo.path.to_str().unwrap());
    assert_eq!(
        app.repository.root().canonicalize().unwrap(),
        repo.path.canonicalize().unwrap()
    );
    let workspace_repo = git_wirdo::git::Repository::open(&path).unwrap();
    assert!(workspace_repo.ensure_clean().is_err());
    support::git(&path, &["add", "--all"]);
    support::git(&path, &["commit", "-m", "retain work"]);
    repo.open().remove_workspace(w).unwrap();
    assert!(!path.exists());
    assert!(repo.git(&["show", "topic:file"]).contains("keep me"));
}

#[test]
fn prompts_consume_shortcuts_and_require_explicit_remove_confirmation() {
    let repo = TestRepo::new();
    repo.write("file", "base");
    repo.commit_all("base");
    let path = repo.directory.0.join("review");
    let mut app = App::new(repo.open()).unwrap();
    app.view = ViewMode::Workspaces;
    app.refresh().unwrap();
    app.handle(Action::New);
    type_text(&mut app, path.to_str().unwrap());
    type_text(&mut app, "qsc-topic");
    assert!(app.running && path.exists());
    app.workspace_selection = app
        .workspaces
        .iter()
        .position(|w| w.branch == "qsc-topic")
        .unwrap();
    app.handle(Action::Remove);
    type_text(&mut app, "yes");
    assert!(path.exists() && app.message_is_error);
    app.handle(Action::Remove);
    type_text(&mut app, "remove");
    assert!(!path.exists());
    assert!(
        repo.git(&["branch", "--list", "qsc-topic"])
            .contains("qsc-topic")
    );
}

#[test]
fn failed_open_preserves_current_repo_and_escape_cancels_prompt() {
    let repo = TestRepo::new();
    let mut app = App::new(repo.open()).unwrap();
    app.handle(Action::OpenRepository);
    type_text(&mut app, "nonexistent");
    assert_eq!(
        app.repository.root().canonicalize().unwrap(),
        repo.path.canonicalize().unwrap()
    );
    assert!(app.running && app.message_is_error);
    app.handle(Action::OpenRepository);
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.prompt.is_none());
}

#[test]
fn pr_checkout_refuses_dirty_work_and_merge_requires_selected_number() {
    let repo = TestRepo::new();
    repo.write("file", "unsaved");
    let mut app = App::new(repo.open()).unwrap();
    app.view = ViewMode::PullRequests;
    app.pull_requests.push(git_wirdo::github::PullRequest {
        number: 999999,
        title: "Fixture".into(),
        url: "https://github.com/o/r/pull/999999".into(),
        head_ref_name: "topic".into(),
        base_ref_name: "main".into(),
        head_ref_oid: "abc".into(),
        is_draft: false,
    });
    app.handle(Action::SwitchBranch);
    assert!(app.message.contains("Commit or stash"));
    assert_eq!(
        std::fs::read_to_string(repo.path.join("file")).unwrap(),
        "unsaved"
    );
    app.handle(Action::MergePr);
    type_text(&mut app, "1");
    type_text(&mut app, "squash");
    assert!(app.message.contains("Merge cancelled"));
    app.handle(Action::CommentPr);
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.prompt.is_none());
}
