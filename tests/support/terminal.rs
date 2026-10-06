//! Drive the real binary through a PTY and reconstruct its displayed screen.
use super::TempDirectory;
use std::{
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::PermissionsExt,
    },
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub struct TerminalFixture {
    master: Option<File>,
    slave: Option<File>,
    child: Child,
    parser: vt100::Parser,
    frame_count: u64,
    minimum_frame: u64,
    frame_tail: Vec<u8>,
    before: libc::termios,
    _config: TempDirectory,
}
impl TerminalFixture {
    pub fn spawn(binary: &Path, root: &Path) -> Self {
        Self::spawn_with(binary, root, |_| {})
    }
    pub fn spawn_with(binary: &Path, root: &Path, configure: impl FnOnce(&mut Command)) -> Self {
        let config = TempDirectory::new();
        let fake = config.0.join("gh");
        std::fs::write(
            &fake,
            "#!/bin/sh\necho 'GitHub disabled in terminal fixture' >&2\nexit 49\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut paths = vec![config.0.clone()];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        let (mut master, mut slave) = (-1, -1);
        let mut size = libc::winsize {
            ws_row: 32,
            ws_col: 200,
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
        let mut before = unsafe { std::mem::zeroed() };
        assert_eq!(
            unsafe { libc::tcgetattr(slave.as_raw_fd(), &mut before) },
            0
        );
        let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
        assert_ne!(flags, -1);
        assert_eq!(
            unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) },
            0
        );
        let mut command = Command::new(binary);
        command
            .arg("--repo")
            .arg(root)
            .arg("--no-state")
            .env("TERM", "xterm-256color")
            .env("PATH", std::env::join_paths(paths).unwrap())
            .env("GH_CONFIG_DIR", &config.0)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave.try_clone().unwrap()));
        configure(&mut command);
        let child = command.spawn().unwrap();
        let mut terminal = Self {
            master: Some(master),
            slave: Some(slave),
            child,
            parser: vt100::Parser::new(32, 200, 0),
            frame_count: 0,
            minimum_frame: 0,
            frame_tail: Vec::new(),
            before,
            _config: config,
        };
        terminal.expect("Action: Ready");
        terminal
    }
    fn drain(&mut self) {
        loop {
            let mut bytes = [0; 32_768];
            match self.master.as_mut().unwrap().read(&mut bytes) {
                Ok(0) => break,
                Ok(count) => {
                    self.parser.process(&bytes[..count]);
                    const END_FRAME: &[u8] = b"\x1b[?25l";
                    self.frame_tail.extend_from_slice(&bytes[..count]);
                    self.frame_count += self
                        .frame_tail
                        .windows(END_FRAME.len())
                        .filter(|bytes| *bytes == END_FRAME)
                        .count() as u64;
                    let keep = self.frame_tail.len().saturating_sub(END_FRAME.len() - 1);
                    self.frame_tail.drain(..keep);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) if error.raw_os_error() == Some(libc::EIO) => break,
                Err(error) => panic!("PTY read: {error}"),
            }
        }
    }
    pub fn screen(&self) -> String {
        self.parser.screen().contents()
    }
    fn idle(screen: &str) -> bool {
        !screen
            .lines()
            .filter(|line| line.contains("Action:"))
            .any(|line| line.contains("Working:") || line.contains("Applying confirmed action"))
    }
    pub fn wait(&mut self, predicate: impl Fn(&str) -> bool) -> String {
        self.wait_for("predicate", predicate)
    }
    fn wait_for(&mut self, description: &str, predicate: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.drain();
            let screen = self.screen();
            if self.frame_count >= self.minimum_frame && Self::idle(&screen) && predicate(&screen) {
                return screen;
            }
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "TUI exited unexpectedly:\n{screen}"
            );
            assert!(
                Instant::now() < deadline,
                "TUI never reached {description}:\n{screen}"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
    pub fn expect(&mut self, text: &str) -> String {
        self.wait_for(text, |screen| screen.contains(text))
    }
    pub fn send(&mut self, bytes: &[u8]) {
        self.drain();
        // Ratatui hides the cursor after drawing each frame. Allow an in-flight
        // old frame, then require a complete frame after the input was read.
        self.minimum_frame = self.frame_count + 2;
        self.master.as_mut().unwrap().write_all(bytes).unwrap();
    }
    pub fn key(&mut self, bytes: &[u8], expected: &str) -> String {
        self.send(bytes);
        self.expect(expected)
    }
    pub fn field(&mut self, text: &str, expected: &str) -> String {
        // Paste is one input event, preserving Unicode/newlines and shortcut letters.
        self.send(format!("\x1b[200~{text}\x1b[201~\r").as_bytes());
        self.minimum_frame += 1; // paste and Enter are two distinct input events
        self.expect(expected)
    }
    pub fn view(&mut self, wanted: &str) {
        for _ in 0..16 {
            self.drain();
            let previous = self
                .screen()
                .lines()
                .find_map(|line| {
                    line.split_once("View: ").map(|(_, tail)| {
                        tail.split_whitespace()
                            .next()
                            .unwrap()
                            .trim_end_matches('│')
                            .to_owned()
                    })
                })
                .unwrap();
            if previous == wanted {
                return;
            }
            self.send(b"\t");
            self.wait(|screen| {
                !screen.contains(&format!("View: {previous}")) && screen.contains("View: ")
            });
        }
        panic!("View {wanted} is unreachable: {}", self.screen());
    }
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let mut size = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        assert_eq!(
            unsafe {
                libc::ioctl(
                    self.slave.as_ref().unwrap().as_raw_fd(),
                    libc::TIOCSWINSZ,
                    &mut size,
                )
            },
            0
        );
        self.parser.screen_mut().set_size(rows, cols);
        assert_eq!(
            unsafe { libc::kill(self.child.id().try_into().unwrap(), libc::SIGWINCH) },
            0
        );
    }
    pub fn quit(&mut self) {
        self.send(b"q");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            self.drain();
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "TUI failed: {}", self.screen());
                break;
            }
            assert!(
                Instant::now() < deadline,
                "TUI did not quit: {}",
                self.screen()
            );
            thread::sleep(Duration::from_millis(10));
        }
        let mut after = unsafe { std::mem::zeroed::<libc::termios>() };
        assert_eq!(
            unsafe { libc::tcgetattr(self.slave.as_ref().unwrap().as_raw_fd(), &mut after) },
            0
        );
        assert_eq!(
            (
                after.c_iflag,
                after.c_oflag,
                after.c_cflag,
                after.c_lflag,
                after.c_cc
            ),
            (
                self.before.c_iflag,
                self.before.c_oflag,
                self.before.c_cflag,
                self.before.c_lflag,
                self.before.c_cc
            )
        );
        assert!(!self.parser.screen().alternate_screen());
    }
}
impl Drop for TerminalFixture {
    fn drop(&mut self) {
        if self.child.try_wait().is_ok_and(|status| status.is_none()) {
            let _ = self.master.as_mut().unwrap().write_all(b"q");
            let deadline = Instant::now() + Duration::from_secs(2);
            while self.child.try_wait().is_ok_and(|status| status.is_none())
                && Instant::now() < deadline
            {
                self.drain();
                thread::sleep(Duration::from_millis(10));
            }
            let _ = self.child.kill();
        }
        // Close both PTY ends before waiting to avoid a macOS tty close drain.
        self.master.take();
        self.slave.take();
        let _ = self.child.wait();
    }
}
