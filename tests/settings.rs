mod support;
use git_wirdo::{
    app::{Action, App},
    model::ViewMode,
    settings::{self, NativePath, Navigation, Store},
};
use support::{TempDirectory, TestRepo};

#[test]
fn restart_restores_per_working_tree_settings_without_saving_transient_targets() {
    let repo = TestRepo::new();
    let path = repo.directory.0.join("state.json");
    let mut app = App::new(repo.open()).unwrap();
    app.view = ViewMode::History;
    app.history_limit = 120;
    app.graph_visible = true;
    app.pr_filter.state = "all".into();
    app.pr_filter.search = "author:@me label:bug".into();
    app.capture_navigation();
    settings::save(&path, &app.navigation).unwrap();
    let mut restarted = App::new(repo.open()).unwrap();
    restarted.restore_navigation(settings::load(&path).unwrap());
    assert_eq!(restarted.view, ViewMode::History);
    assert_eq!(restarted.history_limit, 120);
    assert!(restarted.graph_visible);
    assert_eq!(restarted.pr_filter, app.pr_filter);
    assert!(
        restarted.prompt.is_none() && restarted.review_pr.is_none() && restarted.seen.is_empty()
    );
    restarted.view = ViewMode::PrFiles;
    restarted.capture_navigation();
    settings::save(&path, &restarted.navigation).unwrap();
    assert_eq!(
        settings::load(&path).unwrap().repositories[0].view,
        ViewMode::Files
    );
    assert!(!std::fs::read_to_string(path).unwrap().contains("body"));
}

#[test]
fn recent_open_keeps_unsaved_files_and_settings_on_both_working_trees() {
    let repo = TestRepo::new();
    repo.write("file", "base");
    repo.commit_all("base");
    let worktree = repo.directory.0.join("linked tree");
    repo.open().create_workspace(&worktree, "topic").unwrap();
    repo.write("file", "unsaved");
    let other = git_wirdo::git::Repository::open(&worktree).unwrap();
    let mut other_app = App::new(other).unwrap();
    other_app.view = ViewMode::Branches;
    other_app.capture_navigation();
    let mut app = App::new(repo.open()).unwrap();
    app.restore_navigation(other_app.navigation);
    app.view = ViewMode::History;
    app.graph_visible = true;
    app.handle(Action::RecentRepositories);
    assert_eq!(app.navigation.repositories.len(), 2);
    app.recent_selection = 1;
    app.handle(Action::SwitchBranch);
    assert_eq!(app.repository.root(), worktree.canonicalize().unwrap());
    assert_eq!(app.view, ViewMode::Branches);
    app.handle(Action::RecentRepositories);
    app.recent_selection = 1;
    app.handle(Action::SwitchBranch);
    assert_eq!(app.repository.root(), repo.path.canonicalize().unwrap());
    assert_eq!(app.view, ViewMode::History);
    assert!(app.graph_visible);
    assert_eq!(
        std::fs::read_to_string(repo.path.join("file")).unwrap(),
        "unsaved"
    );
    assert_eq!(repo.git(&["branch", "--show-current"]).trim(), "main");
}

#[test]
fn missing_recent_path_does_not_replace_current_repo_and_forget_does_not_delete_files() {
    let current = TestRepo::new();
    let other = TestRepo::new();
    let mut app = App::new(current.open()).unwrap();
    app.restore_navigation(App::new(other.open()).unwrap().navigation);
    app.handle(Action::RecentRepositories);
    app.recent_selection = 1;
    std::fs::rename(&other.path, other.directory.0.join("moved")).unwrap();
    let before = app.navigation.clone();
    app.handle(Action::SwitchBranch);
    assert!(app.message_is_error);
    assert_eq!(app.repository.root(), current.path.canonicalize().unwrap());
    assert_eq!(app.navigation, before);
    app.handle(Action::Remove);
    app.capture_navigation();
    assert_eq!(app.navigation.repositories.len(), 1);
    assert!(other.directory.0.join("moved/.git").exists());
}

#[test]
fn corrupt_unsupported_and_oversized_settings_are_preserved() {
    let directory = TempDirectory::new();
    let path = directory.0.join("state.json");
    assert!(settings::load(&path).unwrap().repositories.is_empty());
    for bytes in [
        b"bad json".to_vec(),
        br#"{"version":99,"repositories":[]}"#.to_vec(),
        vec![b' '; 1024 * 1024 + 1],
    ] {
        std::fs::write(&path, &bytes).unwrap();
        assert!(Store::open(path.clone()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn invalid_saved_paths_limits_and_duplicates_are_rejected_before_restore() {
    let repo = TestRepo::new();
    let path = repo.directory.0.join("state.json");
    let valid = serde_json::to_value(App::new(repo.open()).unwrap().navigation).unwrap();
    let mut samples = Vec::new();
    let mut json = valid.clone();
    json["repositories"][0]["root"]["units"] = serde_json::json!([0]);
    samples.push(json);
    let mut json = valid.clone();
    json["repositories"][0]["history_limit"] = serde_json::json!(usize::MAX);
    samples.push(json);
    let mut json = valid.clone();
    json["repositories"][0]["pr_filter"]["limit"] = serde_json::json!(0);
    samples.push(json);
    let mut json = valid.clone();
    json["repositories"][0]["view"] = serde_json::json!("PrFiles");
    samples.push(json);
    let mut json = valid.clone();
    json["repositories"]
        .as_array_mut()
        .unwrap()
        .push(valid["repositories"][0].clone());
    samples.push(json);
    for json in samples {
        let bytes = serde_json::to_vec(&json).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        assert!(settings::load(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn failed_atomic_replace_cleans_temporary_file_and_keeps_destination() {
    let directory = TempDirectory::new();
    let path = directory.0.join("state.json");
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("sentinel"), "retain").unwrap();
    assert!(settings::save(&path, &Navigation::default()).is_err());
    assert_eq!(
        std::fs::read_to_string(path.join("sentinel")).unwrap(),
        "retain"
    );
    assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 1);
}

#[cfg(windows)]
#[test]
fn unpaired_utf16_path_units_round_trip() {
    use std::os::windows::ffi::OsStringExt;
    let path = std::path::PathBuf::from(std::ffi::OsString::from_wide(&[
        67, 58, 92, 0xd800, 92, 120,
    ]));
    let encoded = NativePath::new(&path);
    let decoded: NativePath =
        serde_json::from_slice(&serde_json::to_vec(&encoded).unwrap()).unwrap();
    assert_eq!(decoded.path().unwrap(), path);
}

#[test]
fn interleaved_clients_merge_other_repositories_and_honor_forgetting() {
    let directory = TempDirectory::new();
    let path = directory.0.join("state.json");
    let one = TestRepo::new();
    let two = TestRepo::new();
    let (mut first, _) = Store::open(path.clone()).unwrap();
    let (mut second, _) = Store::open(path.clone()).unwrap();
    let one_app = App::new(one.open()).unwrap();
    let two_app = App::new(two.open()).unwrap();
    first.persist(&one_app.navigation).unwrap();
    let merged = second.persist(&two_app.navigation).unwrap();
    assert_eq!(merged.repositories.len(), 2);
    assert_eq!(
        merged.repositories[0].root,
        NativePath::new(two.open().root())
    );
    let (mut forgetter, mut navigation) = Store::open(path.clone()).unwrap();
    navigation
        .repositories
        .retain(|r| r.root != NativePath::new(one.open().root()));
    forgetter.persist(&navigation).unwrap();
    assert_eq!(settings::load(&path).unwrap().repositories.len(), 1);
    let mut changed = two_app;
    changed.graph_visible = true;
    changed.capture_navigation();
    second.persist(&changed.navigation).unwrap();
    assert_eq!(settings::load(&path).unwrap().repositories.len(), 1);
    assert!(settings::load(&path).unwrap().repositories[0].graph_visible);
}

#[test]
fn reopening_current_root_updates_recency_without_erasing_another_clients_root() {
    let directory = TempDirectory::new();
    let path = directory.0.join("state.json");
    let one = TestRepo::new();
    let two = TestRepo::new();
    let (mut first, _) = Store::open(path.clone()).unwrap();
    let (mut second, _) = Store::open(path.clone()).unwrap();
    let mut app = App::new(one.open()).unwrap();
    first.persist(&app.navigation).unwrap();
    second
        .persist(&App::new(two.open()).unwrap().navigation)
        .unwrap();
    assert_eq!(
        settings::load(&path).unwrap().repositories[0].root,
        NativePath::new(two.open().root())
    );
    app.handle(Action::RecentRepositories);
    app.handle(Action::SwitchBranch);
    first.persist(&app.navigation).unwrap();
    let navigation = settings::load(&path).unwrap();
    assert_eq!(navigation.repositories.len(), 2);
    assert_eq!(
        navigation.repositories[0].root,
        NativePath::new(one.open().root())
    );
}

#[test]
fn lock_contention_and_save_failure_leave_previous_snapshot_and_release_lock() {
    let directory = TempDirectory::new();
    let path = directory.0.join("state.json");
    settings::save(&path, &Navigation::default()).unwrap();
    let before = std::fs::read(&path).unwrap();
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.0.join("state.json.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    let (mut store, _) = Store::open(path.clone()).unwrap();
    let repo = TestRepo::new();
    let app = App::new(repo.open()).unwrap();
    assert!(store.persist(&app.navigation).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    lock.unlock().unwrap();
    drop(lock);
    assert_eq!(
        store.persist(&app.navigation).unwrap().repositories.len(),
        1
    );
    std::fs::write(&path, "corrupted externally").unwrap();
    let mut changed = app;
    changed.graph_visible = true;
    changed.capture_navigation();
    assert!(store.persist(&changed.navigation).is_err());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "corrupted externally"
    );
}

#[cfg(unix)]
#[test]
fn non_utf8_paths_round_trip_and_saved_file_is_private() {
    use std::os::unix::{ffi::OsStringExt, fs::PermissionsExt};
    let directory = TempDirectory::new();
    let root = directory
        .0
        .join(std::ffi::OsString::from_vec(b"odd-\xff\nroot".to_vec()));
    let repo = TestRepo::new();
    let mut app = App::new(repo.open()).unwrap();
    app.navigation.repositories[0].root = NativePath::new(&root);
    let path = directory.0.join("state.json");
    settings::save(&path, &app.navigation).unwrap();
    assert_eq!(
        settings::load(&path).unwrap().repositories[0]
            .root
            .path()
            .unwrap(),
        root
    );
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(NativePath::new(&root).path().unwrap(), root);
}
