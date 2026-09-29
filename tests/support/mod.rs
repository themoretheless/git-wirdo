#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use git_wirdo::git::Repository;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

/// Each test owns its repositories. No tests change the project's branch, index, config, or remotes.
pub struct TempDirectory(pub PathBuf);

impl TempDirectory {
    pub fn new() -> Self {
        let id = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "git-wirdo-test-{}-{timestamp}-{id}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub struct TestRepo {
    pub directory: TempDirectory,
    pub path: PathBuf,
}

impl TestRepo {
    pub fn new() -> Self {
        let directory = TempDirectory::new();
        let path = directory.0.join("repo");
        init(&path);
        Self { directory, path }
    }

    pub fn open(&self) -> Repository {
        Repository::open(&self.path).unwrap()
    }

    pub fn git(&self, args: &[&str]) -> String {
        git(&self.path, args)
    }

    pub fn fail(&self, args: &[&str]) -> Output {
        let output = git_output(&self.path, args);
        assert!(
            !output.status.success(),
            "git {args:?} unexpectedly succeeded"
        );
        output
    }

    pub fn write(&self, name: impl AsRef<Path>, text: &str) {
        let path = self.path.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    pub fn commit_all(&self, message: &str) {
        self.git(&["add", "--all"]);
        self.git(&["commit", "--quiet", "-m", message]);
    }
}

pub fn init(path: &Path) {
    fs::create_dir_all(path).unwrap();
    git(path, &["init", "--quiet", "--initial-branch=main"]);
    for (key, value) in [
        ("user.name", "Wirdo Tests"),
        ("user.email", "wirdo-tests@example.invalid"),
        ("commit.gpgsign", "false"),
        ("tag.gpgsign", "false"),
        ("core.autocrlf", "false"),
        ("core.quotePath", "true"),
        ("color.ui", "always"),
        ("log.showSignature", "false"),
        ("rerere.enabled", "false"),
        ("pull.rebase", "false"),
    ] {
        git(path, &["config", key, value]);
    }
    let hooks = path.join(".git").join("empty-hooks");
    fs::create_dir_all(&hooks).unwrap();
    git(path, &["config", "core.hooksPath", hooks.to_str().unwrap()]);
}

pub fn git_output(path: &Path, args: &[&str]) -> Output {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(path)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_EDITOR", "true")
        .env("GIT_TERMINAL_PROMPT", "0");
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ] {
        command.env_remove(key);
    }
    command
        .output()
        .expect("Git must be installed to run the integration tests")
}

pub fn git(path: &Path, args: &[&str]) -> String {
    let output = git_output(path, args);
    assert!(
        output.status.success(),
        "git {args:?}: {}\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8(output.stdout).unwrap()
}

pub fn merge_conflict() -> TestRepo {
    let repo = TestRepo::new();
    repo.write("conflict.txt", "base\n");
    repo.commit_all("base");
    repo.git(&["switch", "-c", "topic"]);
    repo.write("conflict.txt", "theirs\n");
    repo.commit_all("topic edit");
    repo.git(&["switch", "main"]);
    repo.write("conflict.txt", "ours\n");
    repo.commit_all("main edit");
    repo.fail(&["merge", "topic"]);
    repo
}

pub fn rebase_conflict() -> TestRepo {
    let repo = TestRepo::new();
    repo.write("first.txt", "base\n");
    repo.write("second.txt", "base\n");
    repo.commit_all("base");
    repo.git(&["switch", "-c", "topic"]);
    repo.write("first.txt", "topic first\n");
    repo.commit_all("first topic change");
    repo.write("second.txt", "topic second\n");
    repo.commit_all("second topic change");
    repo.git(&["switch", "main"]);
    repo.write("first.txt", "main first\n");
    repo.write("second.txt", "main second\n");
    repo.commit_all("main changes");
    repo.git(&["switch", "topic"]);
    repo.fail(&["rebase", "main"]);
    repo
}
