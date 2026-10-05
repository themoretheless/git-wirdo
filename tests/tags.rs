mod support;
use git_wirdo::app::{Action, App};
use git_wirdo::model::ViewMode;
use support::{TestRepo, git};

#[test]
fn lightweight_and_annotated_tags_are_inspectable_without_touching_work() {
    let repo = TestRepo::new();
    repo.git(&["config", "tag.gpgSign", "false"]);
    repo.write("file", "base");
    repo.commit_all("base");
    repo.write("dirty", "keep");
    let repository = repo.open();
    repository.create_tag("v1", "", "").unwrap();
    repository
        .create_tag("v2", "HEAD", "Release notes")
        .unwrap();
    let tags = repository.tags().unwrap();
    assert_eq!(tags.len(), 2);
    assert_eq!(tags.iter().find(|t| t.name == "v1").unwrap().kind, "commit");
    let annotated = tags.iter().find(|t| t.name == "v2").unwrap();
    assert_eq!(annotated.kind, "tag");
    assert!(
        repository
            .tag_detail(annotated)
            .unwrap()
            .contains("Release notes")
    );
    assert!(repository.create_tag("v1", "HEAD", "replacement").is_err());
    assert!(repository.create_tag("bad name", "HEAD", "").is_err());
    assert!(
        repository
            .create_tag("invalid", "missing-commit", "")
            .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(repo.path.join("dirty")).unwrap(),
        "keep"
    );
}

#[test]
fn local_tag_delete_uses_expected_object_and_refuses_replaced_tag() {
    let repo = TestRepo::new();
    repo.write("file", "base");
    repo.commit_all("base");
    let repository = repo.open();
    repository.create_tag("v1", "HEAD", "").unwrap();
    let selected = repository.tags().unwrap()[0].clone();
    repo.write("file", "new");
    repo.commit_all("new");
    repo.git(&["tag", "-f", "v1"]);
    assert!(repository.delete_tag(&selected).is_err());
    assert_eq!(
        repo.git(&["rev-parse", "v1"]),
        repo.git(&["rev-parse", "HEAD"])
    );
    repository
        .delete_tag(&repository.tags().unwrap()[0])
        .unwrap();
    assert!(repository.tags().unwrap().is_empty());
}

#[test]
fn publication_is_selective_and_remote_delete_rejects_changed_tag() {
    let repo = TestRepo::new();
    repo.git(&["config", "tag.gpgSign", "false"]);
    repo.write("file", "base");
    repo.commit_all("base");
    let path = repo.directory.0.join("remote.git");
    std::fs::create_dir(&path).unwrap();
    git(&path, &["init", "--bare"]);
    let repository = repo.open();
    repository
        .add_remote("origin", path.to_str().unwrap())
        .unwrap();
    repository.create_tag("v1", "HEAD", "notes").unwrap();
    repository.create_tag("unpublished", "HEAD", "").unwrap();
    let selected = repository
        .tags()
        .unwrap()
        .into_iter()
        .find(|t| t.name == "v1")
        .unwrap();
    repository.publish_tag(&selected, "origin").unwrap();
    assert_eq!(git(&path, &["rev-parse", "v1"]).trim(), selected.sha);
    assert!(!git(&path, &["tag", "--list"]).contains("unpublished"));
    repo.write("file", "new");
    repo.commit_all("new");
    repository.publish_branch("origin").unwrap();
    let sha = repo.git(&["rev-parse", "HEAD"]);
    git(&path, &["update-ref", "refs/tags/v1", sha.trim()]);
    assert!(repository.delete_remote_tag(&selected, "origin").is_err());
    assert_eq!(git(&path, &["rev-parse", "v1"]), sha);
    git(&path, &["update-ref", "refs/tags/v1", &selected.sha]);
    repository.delete_remote_tag(&selected, "origin").unwrap();
    assert!(git(&path, &["tag", "--list", "v1"]).trim().is_empty());
    assert!(repository.tags().unwrap().iter().any(|t| t.name == "v1"));
}

#[test]
fn tag_dialog_requires_confirmed_local_deletion() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let repo = TestRepo::new();
    repo.write("file", "base");
    repo.commit_all("base");
    let mut app = App::new(repo.open()).unwrap();
    app.view = ViewMode::Tags;
    app.refresh().unwrap();
    app.handle(Action::New);
    for value in ["ui-tag", "", ""] {
        for c in value.chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    }
    assert_eq!(app.tags[0].name, "ui-tag");
    app.handle(Action::Remove);
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.repository.tags().unwrap().len(), 1);
    app.handle(Action::Remove);
    for c in "delete".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.tags.is_empty());
}
