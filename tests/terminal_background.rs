#![cfg(unix)]
mod support;
use std::{
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::PermissionsExt,
    },
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use support::TestRepo;

struct Pty {
    master: Option<File>,
    slave: Option<File>,
    child: Child,
}
impl Drop for Pty {
    fn drop(&mut self) {
        let _ = self.master.as_mut().unwrap().write_all(b"q");
        let deadline = Instant::now() + Duration::from_secs(2);
        while self.child.try_wait().is_ok_and(|state| state.is_none()) && Instant::now() < deadline
        {
            let mut drain = [0; 16384];
            let _ = self.master.as_mut().unwrap().read(&mut drain);
            thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        // Closing PTY descriptors before waiting also releases a macOS terminal close drain.
        self.master.take();
        self.slave.take();
        let _ = self.child.wait();
    }
}
#[test]
fn actual_terminal_stays_responsive_during_hook_and_quits_with_restored_modes() {
    let r = TestRepo::new();
    r.write("file", "base");
    r.commit_all("base");
    let head = r.git(&["rev-parse", "HEAD"]);
    r.write("file", "staged");
    r.git(&["add", "file"]);
    let hooks = r.path.join(".git/pty-hooks");
    std::fs::create_dir(&hooks).unwrap();
    let script = hooks.join("pre-commit");
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf 'pty-hook-ready\\n' >&2\nsleep 30 &\nwait\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    r.git(&["config", "core.hooksPath", hooks.to_str().unwrap()]);
    let mut master = -1;
    let mut slave = -1;
    let mut size = libc::winsize {
        ws_row: 30,
        ws_col: 180,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        },
        0
    );
    let master = unsafe { File::from_raw_fd(master) };
    let slave = unsafe { File::from_raw_fd(slave) };
    let mut before = unsafe { std::mem::zeroed::<libc::termios>() };
    assert_eq!(
        unsafe { libc::tcgetattr(slave.as_raw_fd(), &mut before) },
        0
    );
    let mut command = Command::new(env!("CARGO_BIN_EXE_git-wirdo"));
    command
        .args(["--repo"])
        .arg(&r.path)
        .env("TERM", "xterm-256color")
        .stdin(Stdio::from(slave.try_clone().unwrap()))
        .stdout(Stdio::from(slave.try_clone().unwrap()))
        .stderr(Stdio::from(slave.try_clone().unwrap()));
    let child = command.spawn().unwrap();
    let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
    assert_ne!(flags, -1);
    assert_eq!(
        unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) },
        0
    );
    let mut pty = Pty {
        master: Some(master),
        slave: Some(slave),
        child,
    };
    let mut transcript = Vec::new();
    fn wait_text(pty: &mut Pty, transcript: &mut Vec<u8>, text: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut chunk = [0; 16384];
        loop {
            match pty.master.as_mut().unwrap().read(&mut chunk) {
                Ok(n) => transcript.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => panic!("terminal read: {e}"),
            }
            if String::from_utf8_lossy(transcript).contains(text) {
                return;
            }
            assert!(Instant::now() < deadline, "terminal never showed {text}");
            thread::sleep(Duration::from_millis(10));
        }
    }
    wait_text(&mut pty, &mut transcript, "Git Wirdo");
    pty.master
        .as_mut()
        .unwrap()
        .write_all(b"cslow terminal commit\r")
        .unwrap();
    wait_text(&mut pty, &mut transcript, "pty-hook-ready");
    assert!(pty.child.try_wait().unwrap().is_none());
    // Rendered diagnostics prove the event loop continued while the hook was sleeping.
    assert!(String::from_utf8_lossy(&transcript).contains("cancels"));
    pty.master.as_mut().unwrap().write_all(b"q").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut drain = [0; 16384];
        let _ = pty.master.as_mut().unwrap().read(&mut drain);
        if let Some(status) = pty.child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "quit did not clean up pending hook"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let mut after = unsafe { std::mem::zeroed::<libc::termios>() };
    assert_eq!(
        unsafe { libc::tcgetattr(pty.slave.as_ref().unwrap().as_raw_fd(), &mut after) },
        0
    );
    assert_eq!(
        (
            before.c_iflag,
            before.c_oflag,
            before.c_cflag,
            before.c_lflag,
            before.c_cc
        ),
        (
            after.c_iflag,
            after.c_oflag,
            after.c_cflag,
            after.c_lflag,
            after.c_cc
        )
    );
    assert_eq!(r.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(r.git(&["show", ":file"]), "staged");
}
