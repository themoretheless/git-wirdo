mod support;
use git_wirdo::git::Repository;
use std::{ffi::OsStr, fs, path::Path, process::Command};
use support::{TempDirectory, TestRepo};

#[test]
fn initialize_retains_files_and_rejects_existing_metadata_invalid_branches_and_bare_targets() {
    let directory = TempDirectory::new();
    let destination = directory.0.join("new [ü] repo");
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("keep"), "ordinary file").unwrap();
    let repo =
        Repository::initialize(&directory.0, Path::new("new [ü] repo"), "custom/main").unwrap();
    assert_eq!(repo.load_state().unwrap().branch, "custom/main");
    assert_eq!(
        fs::read_to_string(destination.join("keep")).unwrap(),
        "ordinary file"
    );
    assert!(Repository::initialize(&directory.0, &destination, "other").is_err());
    assert_eq!(repo.load_state().unwrap().branch, "custom/main");
    let invalid = directory.0.join("invalid");
    assert!(Repository::initialize(&directory.0, &invalid, "bad..branch").is_err());
    assert!(!invalid.exists());
    let metadata_child = destination.join(".git/not-a-worktree");
    assert!(Repository::initialize(&directory.0, &metadata_child, "main").is_err());
    assert!(!metadata_child.exists());
    let bare = directory.0.join("bare");
    support::git(&directory.0, &["init", "--bare", bare.to_str().unwrap()]);
    assert!(Repository::initialize(&directory.0, &bare, "main").is_err());
    assert!(Repository::initialize(&directory.0, &bare.join("child"), "main").is_err());
    assert!(!bare.join(".git").exists());
}

#[test]
fn clone_preserves_source_history_and_checks_destination_before_mutating() {
    let source = TestRepo::new();
    source.write("file [ü]", "base");
    source.commit_all("base");
    source.git(&["tag", "v1"]);
    source.write("file [ü]", "uncommitted source");
    let destination = source.directory.0.join("clone ü");
    fs::create_dir(&destination).unwrap();
    let repo =
        Repository::clone_into(&source.directory.0, OsStr::new("repo"), &destination).unwrap();
    assert_eq!(
        repo.load_state().unwrap().commits[0].sha,
        source.git(&["rev-parse", "HEAD"]).trim()
    );
    assert_eq!(
        fs::read_to_string(destination.join("file [ü]")).unwrap(),
        "base"
    );
    assert_eq!(
        fs::read_to_string(source.path.join("file [ü]")).unwrap(),
        "uncommitted source"
    );
    assert_eq!(repo.tags().unwrap()[0].name, "v1");
    assert!(
        Repository::clone_into(&source.directory.0, source.path.as_os_str(), &destination).is_err()
    );
    let occupied = source.directory.0.join("occupied");
    fs::create_dir(&occupied).unwrap();
    fs::write(occupied.join("keep"), "retain").unwrap();
    assert!(
        Repository::clone_into(&source.directory.0, source.path.as_os_str(), &occupied).is_err()
    );
    assert_eq!(fs::read_to_string(occupied.join("keep")).unwrap(), "retain");
    let url_clone = source.directory.0.join("URL clone");
    let url = format!("file://{}", source.path.display());
    let cloned = Repository::clone_into(&source.directory.0, OsStr::new(&url), &url_clone).unwrap();
    assert_eq!(cloned.tags().unwrap()[0].name, "v1");
    assert_eq!(
        fs::read_to_string(url_clone.join("file [ü]")).unwrap(),
        "base"
    );
    let failed = source.directory.0.join("failed");
    let error = Repository::clone_into(&source.directory.0, OsStr::new("missing-source"), &failed)
        .unwrap_err();
    assert!(format!("{error:#}").contains("inspect any partial destination"));
    assert!(!failed.join(".git").exists());
}

#[cfg(unix)]
#[test]
fn lifecycle_rejects_symlinks_including_broken_git_metadata_and_preserves_native_paths() {
    use std::os::unix::{ffi::OsStringExt, fs::symlink};
    let directory = TempDirectory::new();
    let destination = directory.0.join("linked");
    symlink(&directory.0, &destination).unwrap();
    assert!(Repository::initialize(&directory.0, &destination, "main").is_err());
    let broken = directory.0.join("broken");
    fs::create_dir(&broken).unwrap();
    symlink("missing", broken.join(".git")).unwrap();
    assert!(Repository::initialize(&directory.0, &broken, "main").is_err());
    assert!(
        fs::symlink_metadata(broken.join(".git"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let native = directory
        .0
        .join(std::ffi::OsString::from_vec(if cfg!(target_os = "macos") {
            b"repo-native".to_vec()
        } else {
            b"repo-\xff".to_vec()
        }));
    let repo = Repository::initialize(&directory.0, &native, "main").unwrap();
    assert_eq!(repo.root(), native.canonicalize().unwrap());
}

#[test]
fn cli_lifecycle_works_outside_a_checkout_and_rejects_conflicting_requests() {
    let directory = TempDirectory::new();
    let destination = directory.0.join("new");
    let output = Command::new(env!("CARGO_BIN_EXE_git-wirdo"))
        .current_dir(&directory.0)
        .args([
            "--init",
            "new",
            "--initial-branch",
            "topic",
            "--headless",
            "--no-state",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("branch: topic")
    );
    let clone = directory.0.join("clone");
    let output = Command::new(env!("CARGO_BIN_EXE_git-wirdo"))
        .current_dir(&directory.0)
        .args([
            "--clone",
            "new",
            "--destination",
            "clone",
            "--headless",
            "--no-state",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(Repository::open(&clone).is_ok());
    let before = fs::read(destination.join(".git/HEAD")).unwrap();
    for args in [
        vec!["--init", "new", "--repo", "new"],
        vec!["--clone", "new"],
        vec!["--init", "new", "--clone", "new", "--destination", "clone"],
        vec!["--start", "--headless"],
    ] {
        assert!(
            !Command::new(env!("CARGO_BIN_EXE_git-wirdo"))
                .current_dir(&directory.0)
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    assert_eq!(fs::read(destination.join(".git/HEAD")).unwrap(), before);
}
