mod support;
use git_wirdo::app::{Action, App};
use git_wirdo::model::ViewMode;
use support::{TestRepo, git};

fn bare(repo: &TestRepo) -> String {
    let path = repo.directory.0.join("remote with spaces.git");
    std::fs::create_dir(&path).unwrap();
    git(&path, &["init", "--bare", "--initial-branch=main"]);
    path.to_str().unwrap().to_owned()
}

#[test]
fn remote_configuration_roundtrip_and_publish_sets_tracking() {
    let repo = TestRepo::new();
    repo.write("file", "base");
    repo.commit_all("base");
    let url = bare(&repo);
    let repository = repo.open();
    repository.add_remote("origin", &url).unwrap();
    assert_eq!(repository.remotes().unwrap()[0].fetch_urls.trim(), url);
    assert!(repository.add_remote("origin", &url).is_err());
    repository.publish_branch("origin").unwrap();
    let tracking = repository.tracking().unwrap();
    assert_eq!(tracking.upstream.as_deref(), Some("origin/main"));
    assert_eq!((tracking.ahead, tracking.behind), (0, 0));
    repository
        .edit_push_url("origin", "https://example.invalid/push.git")
        .unwrap();
    let remote = repository.remotes().unwrap().remove(0);
    assert!(remote.push_urls.contains("push.git"));
    assert_eq!(remote.fetch_urls.trim(), url);
    repository.set_upstream("").unwrap();
    assert!(repository.tracking().unwrap().upstream.is_none());
    let before = repo.git(&["rev-parse", "HEAD"]);
    repository
        .edit_remote("origin", "https://example.invalid/repo.git")
        .unwrap();
    assert!(
        repository.remotes().unwrap()[0]
            .fetch_urls
            .contains("example.invalid")
    );
    repository.remove_remote("origin").unwrap();
    assert!(repository.remotes().unwrap().is_empty());
    assert_eq!(repo.git(&["rev-parse", "HEAD"]), before);
}

#[test]
fn checkout_remote_and_pull_strategies_resolve_divergence() {
    let writer = TestRepo::new();
    writer.write("base", "base");
    writer.commit_all("base");
    let url = bare(&writer);
    writer.open().add_remote("origin", &url).unwrap();
    writer.open().publish_branch("origin").unwrap();
    let reader = TestRepo::new();
    let repository = reader.open();
    repository.add_remote("origin", &url).unwrap();
    repository.fetch().unwrap();
    assert!(
        repository
            .remote_branches()
            .unwrap()
            .contains(&"origin/main".into())
    );
    reader.git(&["remote", "set-head", "origin", "main"]);
    assert!(
        !repository
            .remote_branches()
            .unwrap()
            .contains(&"origin/HEAD".into())
    );
    repository.checkout_remote("origin/main", "local").unwrap();
    reader.write("local", "local");
    reader.commit_all("local");
    writer.write("remote", "remote");
    writer.commit_all("remote");
    writer.open().push().unwrap();
    repository.fetch().unwrap();
    let tracking = repository.tracking().unwrap();
    assert_eq!((tracking.ahead, tracking.behind), (1, 1));
    assert!(repository.pull_strategy("ff-only").is_err());
    reader.git(&["config", "pull.ff", "only"]);
    repository.pull_strategy("merge").unwrap();
    assert!(reader.path.join("local").exists() && reader.path.join("remote").exists());
    repository.publish_branch("origin").unwrap();
    repository.set_upstream("origin/local").unwrap();
    assert_eq!(
        repository.tracking().unwrap().upstream.as_deref(),
        Some("origin/local")
    );
    assert!(repository.pull_strategy("unknown").is_err());
}

#[test]
fn rebase_pull_replays_local_commit_and_dirty_work_is_preserved() {
    let writer = TestRepo::new();
    writer.write("base", "base");
    writer.commit_all("base");
    let url = bare(&writer);
    writer.open().add_remote("origin", &url).unwrap();
    writer.open().publish_branch("origin").unwrap();
    let reader = TestRepo::new();
    let repository = reader.open();
    repository.add_remote("origin", &url).unwrap();
    repository.fetch().unwrap();
    repository.checkout_remote("origin/main", "local").unwrap();
    reader.write("local", "local");
    reader.commit_all("local");
    writer.write("remote", "remote");
    writer.commit_all("remote");
    writer.open().push().unwrap();
    reader.write("dirty", "keep");
    assert!(repository.pull_strategy("rebase").is_err());
    assert_eq!(
        std::fs::read_to_string(reader.path.join("dirty")).unwrap(),
        "keep"
    );
    std::fs::remove_file(reader.path.join("dirty")).unwrap();
    reader.git(&["config", "pull.ff", "only"]);
    repository.pull_strategy("rebase").unwrap();
    assert_eq!(
        reader.git(&["rev-parse", "HEAD^"]),
        writer.git(&["rev-parse", "HEAD"])
    );
}

#[test]
fn remote_removal_prompt_requires_confirmation() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let repo = TestRepo::new();
    repo.open()
        .add_remote("origin", "https://example.invalid/repo.git")
        .unwrap();
    let mut app = App::new(repo.open()).unwrap();
    app.view = ViewMode::Remotes;
    app.refresh().unwrap();
    app.handle(Action::Remove);
    for c in "yes".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.message_is_error && app.remotes.len() == 1);
}

#[test]
fn missing_upstream_ref_is_visible_instead_of_silent_zero_counts() {
    let repo = TestRepo::new();
    repo.write("base", "base");
    repo.commit_all("base");
    let url = bare(&repo);
    let repository = repo.open();
    repository.add_remote("origin", &url).unwrap();
    repository.publish_branch("origin").unwrap();
    repo.git(&["update-ref", "-d", "refs/remotes/origin/main"]);
    assert!(repository.tracking().is_err());
    let app = App::new(repository).unwrap();
    assert!(app.tracking.error.is_some());
}

#[test]
fn pull_merge_conflict_refreshes_state_and_can_be_aborted() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let writer = TestRepo::new();
    writer.write("file", "base\n");
    writer.commit_all("base");
    let url = bare(&writer);
    writer.open().add_remote("origin", &url).unwrap();
    writer.open().publish_branch("origin").unwrap();
    let reader = TestRepo::new();
    let repository = reader.open();
    repository.add_remote("origin", &url).unwrap();
    repository.fetch().unwrap();
    repository.checkout_remote("origin/main", "local").unwrap();
    reader.write("file", "local\n");
    reader.commit_all("local");
    writer.write("file", "remote\n");
    writer.commit_all("remote");
    writer.open().push().unwrap();
    let before = reader.git(&["rev-parse", "HEAD"]);
    let mut app = App::new(repository).unwrap();
    app.handle(Action::PullStrategy);
    for c in "merge".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        app.message_is_error
            && app.state.merge_state.merge_in_progress
            && app.state.merge_state.conflicts.len() == 1
    );
    app.view = ViewMode::Conflicts;
    app.handle(Action::Abort);
    assert!(!app.message_is_error);
    assert_eq!(reader.git(&["rev-parse", "HEAD"]), before);
    assert_eq!(
        std::fs::read_to_string(reader.path.join("file")).unwrap(),
        "local\n"
    );
}
