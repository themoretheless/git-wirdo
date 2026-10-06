#![cfg(unix)]
mod support;
use std::{fs, path::Path};
use support::{TempDirectory, TestRepo, terminal::TerminalFixture};
fn binary() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_git-wirdo"))
}

#[test]
fn recent_screen_initializes_and_clones_without_touching_previous_dirty_work() {
    let source = TestRepo::new();
    source.write("base", "committed");
    source.commit_all("base");
    source.write("dirty", "retained");
    let created = source.directory.0.join("new ü");
    let cloned = source.directory.0.join("clone ü");
    let mut terminal = TerminalFixture::spawn(binary(), &source.path);
    terminal.key(b"I", "View: RecentRepositories");
    terminal.key(b"N", "New repository path:");
    terminal.field(created.to_str().unwrap(), "Initial branch:");
    terminal.key(b"\r", "Branch: main");
    assert!(git_wirdo::git::Repository::open(&created).is_ok());
    terminal.key(b"I", "View: RecentRepositories");
    terminal.key(b"C", "Git URL or local source path:");
    terminal.field(
        source.path.to_str().unwrap(),
        "Clone destination (empty or new directory):",
    );
    terminal.field(cloned.to_str().unwrap(), "Opened repository");
    assert_eq!(
        fs::read_to_string(cloned.join("base")).unwrap(),
        "committed"
    );
    assert!(!cloned.join("dirty").exists());
    assert_eq!(
        fs::read_to_string(source.path.join("dirty")).unwrap(),
        "retained"
    );
    terminal.key(b"I", "View: RecentRepositories");
    terminal.key(b"N", "New repository path:");
    terminal.field(source.path.to_str().unwrap(), "Initial branch:");
    terminal.key(b"\r", "Destination already contains Git metadata");
    assert_eq!(source.git(&["status", "--porcelain"]).trim(), "?? dirty");
    terminal.quit();
}

#[test]
fn startup_outside_checkout_initializes_then_enters_the_full_ui_and_restores_terminal() {
    let directory = TempDirectory::new();
    let mut terminal = TerminalFixture::spawn_starting(binary(), None, |command| {
        command.current_dir(&directory.0);
    });
    terminal.expect("choose a repository");
    terminal.key(b"N", "New repository path:");
    terminal.field("new [ü]", "Initial branch:");
    terminal.key(b"\r", "Branch: main");
    assert!(git_wirdo::git::Repository::open(&directory.0.join("new [ü]")).is_ok());
    terminal.quit();
}

#[test]
fn startup_clone_errors_are_recoverable_and_open_form_can_be_cancelled() {
    let source = TestRepo::new();
    source.write("file", "base");
    source.commit_all("base");
    let mut terminal = TerminalFixture::spawn_starting(binary(), None, |command| {
        command.current_dir(&source.directory.0).arg("--start");
    });
    terminal.key(b"O", "Repository path:");
    terminal.key(b"\x1b", "Action: Cancelled");
    terminal.key(b"C", "Git URL or local source path:");
    terminal.field(
        "missing-source",
        "Clone destination (empty or new directory):",
    );
    terminal.field("failed", "inspect any partial destination");
    terminal.key(b"C", "Git URL or local source path:");
    terminal.field("repo", "Clone destination (empty or new directory):");
    terminal.field("cloned", "Branch: main");
    assert_eq!(
        fs::read_to_string(source.directory.0.join("cloned/file")).unwrap(),
        "base"
    );
    terminal.quit();
}

#[test]
fn startup_clone_cancellation_cleans_process_tree_and_allows_opening_existing_repo() {
    blocked_clone(false);
}

#[test]
fn quitting_during_startup_clone_waits_for_cleanup_and_restores_terminal() {
    blocked_clone(true);
}

fn blocked_clone(quit: bool) {
    use std::{
        os::unix::fs::PermissionsExt,
        thread,
        time::{Duration, Instant},
    };
    let source = TestRepo::new();
    let shim = source.directory.0.join("ssh-shim");
    let pidfile = source.directory.0.join("pid");
    fs::write(&shim, "#!/bin/sh\necho $$ > \"$CLONE_PID_FILE\"\ntrap 'exit 143' TERM\nwhile :; do sleep 1; done\n").unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o700)).unwrap();
    let mut terminal = TerminalFixture::spawn_starting(binary(), None, |command| {
        command
            .current_dir(&source.directory.0)
            .env("GIT_SSH_COMMAND", &shim)
            .env("GIT_SSH_VARIANT", "ssh")
            .env("CLONE_PID_FILE", &pidfile);
    });
    terminal.key(b"C", "Git URL or local source path:");
    terminal.field(
        "ssh://invalid.example/repo",
        "Clone destination (empty or new directory):",
    );
    terminal.send(b"\x1b[200~partial\x1b[201~\r");
    let deadline = Instant::now() + Duration::from_secs(10);
    let pid = loop {
        if let Ok(text) = fs::read_to_string(&pidfile)
            && let Ok(pid) = text.trim().parse::<i32>()
        {
            break pid;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    };
    if quit {
        terminal.quit();
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        return;
    }
    terminal.key(b"\x1b", "Operation cancelled");
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    terminal.key(b"O", "Repository path:");
    terminal.field("repo", "Branch: main");
    terminal.quit();
}

#[test]
fn startup_recent_repository_opens_from_saved_state_without_initializing_current_directory() {
    let source = TestRepo::new();
    source.write("dirty", "retained");
    let state = source.directory.0.join("navigation.json");
    let app = git_wirdo::app::App::new(source.open()).unwrap();
    git_wirdo::settings::save(&state, &app.navigation).unwrap();
    let mut terminal = TerminalFixture::spawn_saved_starting(binary(), &state, |command| {
        command.current_dir(&source.directory.0);
    });
    terminal.expect("choose a repository");
    terminal.key(b"\r", "dirty");
    terminal.quit();
    assert!(!source.directory.0.join(".git").exists());
    assert_eq!(
        fs::read_to_string(source.path.join("dirty")).unwrap(),
        "retained"
    );
    let saved = git_wirdo::settings::load(&state).unwrap();
    assert_eq!(saved.repositories.len(), 1);
    assert_eq!(
        saved.repositories[0].root.path().unwrap(),
        source.path.canonicalize().unwrap()
    );
}

#[test]
fn long_recent_list_keeps_selected_path_and_operation_controls_visible() {
    use git_wirdo::settings::NativePath;
    let source = TestRepo::new();
    source.write("last-entry-work", "retained");
    let state = source.directory.0.join("navigation.json");
    let app = git_wirdo::app::App::new(source.open()).unwrap();
    let mut navigation = app.navigation.clone();
    let mut actual = navigation.repositories[0].clone();
    actual.last_opened = 0;
    navigation.repositories = (1..=49)
        .map(|number| {
            let mut entry = actual.clone();
            entry.root = NativePath::new(&source.directory.0.join(format!("missing-{number:02}")));
            entry.last_opened = number;
            entry
        })
        .collect();
    navigation.repositories.push(actual);
    git_wirdo::settings::save(&state, &navigation).unwrap();
    let mut terminal = TerminalFixture::spawn_saved_starting(binary(), &state, |command| {
        command.current_dir(&source.directory.0);
    });
    assert!(
        !terminal
            .screen()
            .contains(&source.path.to_string_lossy().to_string())
    );
    let screen = terminal.key(
        &b"j".repeat(49),
        &git_wirdo::model::display_path(&source.path),
    );
    assert!(screen.contains("Action: Ready"));
    terminal.key(b"\r", "last-entry-work");
    terminal.quit();
    assert!(!source.directory.0.join(".git").exists());
}
