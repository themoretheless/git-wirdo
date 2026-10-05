mod support;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use git_wirdo::{
    app::App,
    process::{Control, controlled, output},
    session::Session,
};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use support::TestRepo;

fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}
fn finish(session: &mut Session) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while session.busy() {
        assert!(
            Instant::now() < deadline,
            "background task never finished: {}",
            session.app.message
        );
        session.tick();
        thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn modal_input_is_immediate_and_commit_runs_as_one_serialized_task() {
    let r = TestRepo::new();
    r.write("file", "one");
    r.git(&["add", "file"]);
    let mut session = Session::new(App::new(r.open()).unwrap());
    session.handle_key(key('c'));
    assert!(!session.busy() && session.app.prompt.is_some());
    for c in "background commit".chars() {
        session.handle_key(key(c));
    }
    session.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(session.busy());
    // A second mutation is rejected, not queued for execution after the first task.
    session.handle_key(key('c'));
    finish(&mut session);
    assert!(
        session.app.prompt.is_none() && !session.app.message_is_error,
        "{}",
        session.app.message
    );
    assert_eq!(
        r.git(&["log", "-1", "--format=%s"]).trim(),
        "background commit"
    );
    assert!(session.app.state.files.is_empty());
}
#[test]
fn asynchronous_navigation_and_refresh_return_updated_snapshots() {
    let r = TestRepo::new();
    r.write("file", "one");
    r.commit_all("first");
    let mut session = Session::new(App::new(r.open()).unwrap());
    session.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    finish(&mut session);
    assert_eq!(session.app.view, git_wirdo::model::ViewMode::History);
    assert!(session.app.detail_text.contains("first"));
    session.handle_key(key('g'));
    finish(&mut session);
    assert!(session.app.graph_visible && session.app.detail_text.contains("*"));
}

fn sleeping_command() -> std::process::Command {
    #[cfg(unix)]
    {
        let mut c = std::process::Command::new("sh");
        c.args(["-c", "printf 'progress-ready\\n' >&2; sleep 30 & wait"]);
        c
    }
    #[cfg(windows)]
    {
        let mut c = std::process::Command::new("cmd");
        c.args([
            "/C",
            "echo progress-ready 1>&2 & ping -n 30 127.0.0.1 > nul",
        ]);
        c
    }
}
#[test]
fn controlled_process_streams_progress_and_cancellation_reaps_the_tree() {
    let control = Arc::new(Control::default());
    let worker_control = control.clone();
    let worker =
        thread::spawn(move || controlled(worker_control, 0, || output(&mut sleeping_command())));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !control.progress().contains("progress-ready") {
        assert!(
            Instant::now() < deadline && !worker.is_finished(),
            "command did not report progress: {}",
            control.progress()
        );
        thread::sleep(Duration::from_millis(10));
    }
    let cancelled = Instant::now();
    control.cancel();
    let error = worker.join().unwrap().unwrap_err();
    assert!(error.to_string().contains("cancelled"));
    assert!(
        cancelled.elapsed() < Duration::from_secs(3),
        "pipe readers retained a descendant"
    );
}

#[cfg(unix)]
fn slow_commit() -> (TestRepo, Session) {
    use std::os::unix::fs::PermissionsExt;
    let r = TestRepo::new();
    r.write("file", "base");
    r.commit_all("base");
    r.write("file", "staged");
    r.git(&["add", "file"]);
    let hooks = r.path.join(".git/slow-hooks");
    std::fs::create_dir(&hooks).unwrap();
    let script = hooks.join("pre-commit");
    std::fs::write(
        &script,
        "#!/bin/sh\nsleep 30 &\necho $! > .git/descendant-pid\nprintf 'hook-ready\\n' >&2\nwait\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    r.git(&["config", "core.hooksPath", hooks.to_str().unwrap()]);
    let mut session = Session::new(App::new(r.open()).unwrap());
    session.handle_key(key('c'));
    for c in "slow".chars() {
        session.handle_key(key(c));
    }
    session.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !r.path.join(".git/descendant-pid").exists() {
        assert!(Instant::now() < deadline && session.busy());
        session.tick();
        thread::sleep(Duration::from_millis(10));
    }
    (r, session)
}
#[cfg(unix)]
fn assert_descendant_stopped(r: &TestRepo) {
    let pid = std::fs::read_to_string(r.path.join(".git/descendant-pid"))
        .unwrap()
        .trim()
        .parse::<i32>()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while unsafe { libc::kill(pid, 0) } == 0 {
        // Linux may retain an orphan zombie briefly; it cannot execute or retain pipes.
        if let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            && status.split_whitespace().nth(2) == Some("Z")
        {
            return;
        }
        assert!(Instant::now() < deadline, "hook descendant still alive");
        thread::sleep(Duration::from_millis(10));
    }
}
#[cfg(unix)]
#[test]
fn hook_can_be_cancelled_without_committing_and_tui_remains_usable() {
    let (r, mut session) = slow_commit();
    let head = r.git(&["rev-parse", "HEAD"]);
    session.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
    assert!(session.busy() && session.app.running);
    session.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    finish(&mut session);
    assert!(session.app.message_is_error && session.app.message.contains("cancelled"));
    assert_eq!(r.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(r.git(&["show", ":file"]), "staged");
    assert!(session.app.state.files.iter().any(|f| f.staged));
    assert_descendant_stopped(&r);
    session.handle_key(key('r'));
    finish(&mut session);
    assert!(!session.app.message_is_error);
}
#[cfg(unix)]
#[test]
fn quit_and_drop_cancel_pending_hooks_before_returning() {
    for explicit_quit in [true, false] {
        let (r, mut session) = slow_commit();
        let head = r.git(&["rev-parse", "HEAD"]);
        if explicit_quit {
            session.handle_key(key('q'));
            finish(&mut session);
            assert!(!session.app.running);
        }
        let stopped = Instant::now();
        drop(session);
        assert!(stopped.elapsed() < Duration::from_secs(3));
        assert_eq!(r.git(&["rev-parse", "HEAD"]), head);
        assert_descendant_stopped(&r);
        assert!(!r.path.join(".git/index.lock").exists());
    }
}

#[test]
fn cancelled_scope_starts_no_new_command_and_does_not_poison_cli_calls() {
    let control = Arc::new(Control::default());
    control.cancel();
    let error = controlled(control, 0, || {
        output(std::process::Command::new("git").arg("--version"))
    })
    .unwrap_err();
    assert!(error.to_string().contains("cancelled"));
    assert!(
        output(std::process::Command::new("git").arg("--version"))
            .unwrap()
            .status
            .success()
    );
}
#[test]
fn task_stdin_and_binary_stdout_are_drained_without_deadlocking() {
    let r = TestRepo::new();
    let data = vec![b'x'; 2 * 1024 * 1024];
    std::fs::write(r.path.join("large"), &data).unwrap();
    let expected = r.git(&["hash-object", "large"]);
    let control = Arc::new(Control::default());
    let mut command = std::process::Command::new("git");
    command
        .args(["hash-object", "--stdin"])
        .current_dir(&r.path);
    let result = controlled(control, 0, || {
        git_wirdo::process::output_with_input(&mut command, Some(&data))
    })
    .unwrap();
    assert!(result.status.success());
    assert_eq!(String::from_utf8(result.stdout).unwrap(), expected);
}
#[cfg(unix)]
#[test]
fn uncooperative_descendants_are_forced_down_after_the_grace_period() {
    let control = Arc::new(Control::default());
    let task_control = control.clone();
    let worker = thread::spawn(move || {
        let mut command = std::process::Command::new("sh");
        command.args([
            "-c",
            "trap '' TERM; printf 'ready-for-force\\n' >&2; sleep 30 & wait",
        ]);
        controlled(task_control, 0, || output(&mut command))
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !control.progress().contains("ready-for-force") {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    let stopped = Instant::now();
    control.cancel();
    assert!(
        worker
            .join()
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("cancelled")
    );
    assert!(stopped.elapsed() < Duration::from_secs(3));
}
