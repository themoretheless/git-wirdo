#![cfg(windows)]
mod support;

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::{
    io::{Read, Write},
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use support::TestRepo;

struct Terminal {
    child: Box<dyn Child + Send + Sync>,
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    output: mpsc::Receiver<Vec<u8>>,
    reader: Option<JoinHandle<()>>,
    parser: vt100::Parser,
    settled_after: Instant,
}
impl Terminal {
    fn new(repo: &TestRepo) -> Self {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 32,
                cols: 200,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let writer = Arc::new(Mutex::new(pair.master.take_writer().unwrap()));
        let mut read = pair.master.try_clone_reader().unwrap();
        let reply = writer.clone();
        let (send, output) = mpsc::channel();
        // ConPTY requests the inherited cursor before startup. Service that query
        // concurrently with spawning, and keep draining while ClosePseudoConsole runs.
        let reader = thread::spawn(move || {
            let mut tail = Vec::new();
            let mut bytes = [0; 32768];
            while let Ok(count) = read.read(&mut bytes) {
                if count == 0 {
                    break;
                }
                tail.extend_from_slice(&bytes[..count]);
                for _ in 0..tail.windows(4).filter(|part| *part == b"\x1b[6n").count() {
                    let mut writer = reply.lock().unwrap();
                    let _ = writer.write_all(b"\x1b[1;1R");
                    let _ = writer.flush();
                }
                let keep = tail.len().saturating_sub(3);
                tail.drain(..keep);
                if send.send(bytes[..count].to_vec()).is_err() {
                    break;
                }
            }
        });
        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_git-wirdo"));
        command.args(["--repo"]);
        command.arg(&repo.path);
        command.arg("--no-state");
        command.cwd(&repo.path);
        command.env("TERM", "xterm-256color");
        command.env("GIT_CONFIG_NOSYSTEM", "1");
        command.env("GIT_CONFIG_GLOBAL", "NUL");
        command.env("GH_CONFIG_DIR", &repo.directory.0);
        let child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let mut terminal = Self {
            child,
            master: Some(pair.master),
            writer,
            output,
            reader: Some(reader),
            parser: vt100::Parser::new(32, 200, 0),
            settled_after: Instant::now(),
        };
        terminal.expect("Action: Ready");
        terminal
    }
    fn drain(&mut self) {
        for bytes in self.output.try_iter() {
            self.parser.process(&bytes);
        }
    }
    fn expect(&mut self, expected: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            self.drain();
            let screen = self.parser.screen().contents();
            if Instant::now() >= self.settled_after
                && screen.contains(expected)
                && !screen.contains("Action: Working:")
                && !screen.contains("Applying confirmed action")
            {
                return screen;
            }
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "TUI exited before {expected}:\n{screen}"
            );
            assert!(
                Instant::now() < deadline,
                "ConPTY did not display {expected}:\n{screen}"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn send(&mut self, bytes: &[u8]) {
        self.drain();
        self.settled_after = Instant::now() + Duration::from_millis(250);
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(bytes).unwrap();
        writer.flush().unwrap();
    }
    fn key(&mut self, key: &[u8], expected: &str) -> String {
        self.send(key);
        self.expect(expected)
    }
    fn field(&mut self, text: &str, expected: &str) -> String {
        self.send(format!("{text}\r").as_bytes());
        self.expect(expected)
    }
    fn quit(&mut self) {
        self.send(b"q");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.drain();
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "TUI exit: {status:?}");
                break;
            }
            assert!(Instant::now() < deadline, "TUI did not quit");
            thread::sleep(Duration::from_millis(10));
        }
        drop(self.master.take());
        if let Some(reader) = self.reader.take() {
            reader.join().unwrap();
        }
        self.drain();
        assert!(
            !self.parser.screen().alternate_screen(),
            "Alternate screen was not restored"
        );
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        if self.child.try_wait().is_ok_and(|status| status.is_none()) {
            let _ = self.writer.lock().unwrap().write_all(b"q");
            let deadline = Instant::now() + Duration::from_secs(2);
            while self.child.try_wait().is_ok_and(|status| status.is_none())
                && Instant::now() < deadline
            {
                self.drain();
                thread::sleep(Duration::from_millis(10));
            }
            if self.child.try_wait().is_ok_and(|status| status.is_none()) {
                let _ = self.child.kill();
            }
            let _ = self.child.wait();
        }
        drop(self.master.take());
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

#[test]
fn native_windows_terminal_stages_commits_creates_branch_and_restores_screen() {
    let repo = TestRepo::new();
    repo.write("literal [ü] file.txt", "base\n");
    let mut terminal = Terminal::new(&repo);
    terminal.expect("literal [ü] file.txt");
    terminal.key(b"s", "Action: Staged");
    assert_eq!(
        repo.git(&["diff", "--cached", "--name-only", "-z"]),
        "literal [ü] file.txt\0"
    );
    terminal.key(b"c", "Commit message:");
    terminal.field(
        "native Windows terminal commit",
        "Action: Committed changes",
    );
    assert_eq!(
        repo.git(&["log", "-1", "--format=%s"]).trim(),
        "native Windows terminal commit"
    );
    terminal.key(b"b", "Create or switch branch:");
    terminal.field("windows-topic", "Switched branch");
    assert_eq!(
        repo.git(&["branch", "--show-current"]).trim(),
        "windows-topic"
    );
    terminal
        .master
        .as_ref()
        .unwrap()
        .resize(PtySize {
            rows: 26,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    terminal.parser.screen_mut().set_size(26, 120);
    let screen = terminal.key(b"r", "Action: Refreshed");
    assert!(screen.contains("Branch: windows-topic"));
    terminal.quit();
}

#[test]
fn native_windows_terminal_opens_worktrees_and_generic_local_clone() {
    let repo = TestRepo::new();
    repo.write("file", "base");
    repo.commit_all("base");
    repo.write("retain", "dirty original");
    let workspace = repo.directory.0.join("native workspace ü");
    let cloned = repo.directory.0.join("native clone ü");
    let mut terminal = Terminal::new(&repo);
    terminal.key(b"I", "View: RecentRepositories");
    terminal.key(b"C", "Git URL or local source path:");
    terminal.field(
        repo.path.to_str().unwrap(),
        "Clone destination (empty or new directory):",
    );
    terminal.field(cloned.to_str().unwrap(), "Opened repository");
    assert_eq!(
        std::fs::read_to_string(cloned.join("file")).unwrap(),
        "base"
    );
    assert!(!cloned.join("retain").exists());
    for view in ["History", "Branches", "Conflicts", "Workspaces"] {
        terminal.key(b"\t", &format!("View: {view}"));
    }
    terminal.key(b"N", "Workspace path (relative to repo or absolute):");
    terminal.field(workspace.to_str().unwrap(), "New branch name:");
    terminal.field("native-worktree", "Workspace created");
    terminal.key(b"O", "Repository path:");
    terminal.field(workspace.to_str().unwrap(), "Opened repository");
    assert_eq!(
        support::git(&workspace, &["branch", "--show-current"]).trim(),
        "native-worktree"
    );
    assert_eq!(
        std::fs::read_to_string(repo.path.join("retain")).unwrap(),
        "dirty original"
    );
    terminal.quit();
}

#[test]
fn native_windows_terminal_cancels_a_git_hook_and_keeps_staged_work() {
    let repo = TestRepo::new();
    repo.write("file", "base");
    repo.commit_all("base");
    let head = repo.git(&["rev-parse", "HEAD"]);
    repo.write("file", "staged");
    repo.git(&["add", "file"]);
    let hooks = repo.path.join(".git/native-hooks");
    std::fs::create_dir(&hooks).unwrap();
    std::fs::write(
        hooks.join("pre-commit"),
        "#!/bin/sh\nprintf 'native-hook-ready\\n' >&2\nsleep 30\n",
    )
    .unwrap();
    repo.git(&["config", "core.hooksPath", hooks.to_str().unwrap()]);
    let mut terminal = Terminal::new(&repo);
    terminal.key(b"c", "Commit message:");
    terminal.send(b"cancelled native commit\r");
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        terminal.drain();
        if terminal
            .parser
            .screen()
            .contents()
            .contains("native-hook-ready")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Git hook did not start: {}",
            terminal.parser.screen().contents()
        );
        assert!(terminal.child.try_wait().unwrap().is_none());
        thread::sleep(Duration::from_millis(10));
    }
    terminal.key(b"\x1b", "Operation cancelled");
    assert_eq!(repo.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repo.git(&["show", ":file"]), "staged");
    terminal.quit();
}

#[test]
fn native_windows_file_history_and_blame_keep_pending_work_and_index_unchanged() {
    let repo = TestRepo::new();
    repo.write("old [a] ü.txt", "original line\n");
    repo.commit_all("original creation");
    repo.git(&["mv", "old [a] ü.txt", "new [a] ü.txt"]);
    repo.commit_all("rename only");
    repo.write("new [a] ü.txt", "pending edit\n");
    let mut terminal = Terminal::new(&repo);
    terminal.expect("pending edit");
    terminal.key(b"h", "File history at HEAD");
    terminal.expect("rename only");
    terminal.key(b"s", "File inspection is read-only");
    assert!(repo.git(&["diff", "--cached"]).is_empty());
    terminal.key(b"Q", "Line authorship at HEAD");
    terminal.expect("original line");
    terminal.key(b"Q", "pending edit");
    assert_eq!(
        std::fs::read_to_string(repo.path.join("new [a] ü.txt")).unwrap(),
        "pending edit\n"
    );
    terminal.quit();
}

#[test]
fn native_windows_browses_clean_files_and_exports_a_patch_usable_by_git_am() {
    let repo = TestRepo::new();
    repo.write("clean [a] ü.txt", "committed content\n");
    repo.commit_all("exported native root");
    let mut terminal = Terminal::new(&repo);
    terminal.expect("Ready");
    terminal.key(b"J", "Browsing tracked files");
    terminal.expect("committed content");
    terminal.key(b"h", "File history at HEAD");
    terminal.key(b"Q", "Line authorship at HEAD");
    terminal.key(b"J", "Action: Working changes");
    terminal.key(b"\t", "View: History");
    terminal.key(b"!", "New mail patch destination");
    let path = repo.directory.0.join("native patch ü.patch");
    terminal.field(path.to_str().unwrap(), "Exported mail patch");
    let target = TestRepo::new();
    target.git(&["am", path.to_str().unwrap()]);
    assert_eq!(
        std::fs::read_to_string(target.path.join("clean [a] ü.txt")).unwrap(),
        "committed content\n"
    );
    terminal.quit();
}

#[test]
fn native_windows_palette_selects_and_imports_mail_through_confirmed_forms() {
    let source = TestRepo::new();
    source.write("import [a] ü.txt", "incoming content\n");
    source.commit_all("incoming native mail");
    let target = TestRepo::new();
    let path = source.directory.0.join("native mail ü.patch");
    source.open().export_commit("HEAD", &path).unwrap();
    let mut terminal = Terminal::new(&target);
    terminal.expect("Ready");
    terminal.key(b":", "Commands:");
    terminal.field("view commit history", "View: History");
    terminal.key(b":", "Commands:");
    terminal.field("import mail", "Mail patch file:");
    terminal.field(path.to_str().unwrap(), "Type apply to import commits");
    terminal.field("apply", "Imported mail patch");
    assert_eq!(
        std::fs::read_to_string(target.path.join("import [a] ü.txt")).unwrap(),
        "incoming content\n"
    );
    assert_eq!(
        target.git(&["log", "-1", "--format=%s"]).trim(),
        "incoming native mail"
    );
    terminal.quit();
}

#[test]
fn native_windows_mail_conflict_recovery_continues_and_aborts_the_correct_operation() {
    for abort in [true, false] {
        let source = TestRepo::new();
        source.write("file", "base\n");
        source.commit_all("base");
        source.write("file", "incoming\n");
        source.commit_all("incoming native change");
        let target = TestRepo::new();
        target.write("file", "base\n");
        target.commit_all("base");
        target.write("file", "local\n");
        target.commit_all("local");
        let before = target.git(&["rev-parse", "HEAD"]);
        let path = source.directory.0.join("native conflict.patch");
        source.open().export_commit("HEAD", &path).unwrap();
        let mut terminal = Terminal::new(&target);
        terminal.expect("Ready");
        terminal.key(b"@", "Mail patch file:");
        terminal.field(path.to_str().unwrap(), "Type apply to import commits");
        terminal.field("apply", "View: Conflicts");
        terminal.key(b"r", "Mail patch import in progress");
        if abort {
            terminal.key(b"x", "Aborted Git operation");
            assert_eq!(target.git(&["rev-parse", "HEAD"]), before);
        } else {
            terminal.key(b"t", "Resolved file with Theirs");
            terminal.key(b"e", "Continued Git operation");
            assert_eq!(
                target.git(&["log", "-1", "--format=%s"]).trim(),
                "incoming native change"
            );
            assert_eq!(
                std::fs::read_to_string(target.path.join("file")).unwrap(),
                "incoming\n"
            );
        }
        assert!(
            !target
                .open()
                .load_state()
                .unwrap()
                .merge_state
                .in_progress()
        );
        terminal.quit();
    }
}

#[test]
fn native_windows_refresh_keeps_file_identity_and_viewport_before_staging() {
    let repo = TestRepo::new();
    repo.write(
        "b",
        &(0..100)
            .map(|i| format!("TARGET LINE {i}\n"))
            .collect::<String>(),
    );
    repo.write("c", "other\n");
    let mut terminal = Terminal::new(&repo);
    terminal.expect("TARGET LINE 0");
    terminal.key(b"\x1b[6~", "TARGET LINE 10");
    repo.write("a", "external insertion\n");
    let screen = terminal.key(b"r", "Action: Refreshed");
    assert!(screen.contains("TARGET LINE 10"), "{screen}");
    assert!(
        !screen.contains("TARGET LINE 0"),
        "viewport reset: {screen}"
    );
    terminal.key(b"s", "Action: Staged b");
    assert_eq!(repo.git(&["diff", "--cached", "--name-only"]).trim(), "b");
    assert_eq!(
        std::fs::read_to_string(repo.path.join("a")).unwrap(),
        "external insertion\n"
    );
    terminal.quit();
}

#[test]
fn filters_lines_and_interactive_rebase_in_conpty() {
    let repo = TestRepo::new();
    repo.write("base", "base\n");
    repo.commit_all("base");
    repo.write("aaa", "a\n");
    repo.commit_all("a");
    repo.write("zzz", "one\ntwo\nthree\n");
    repo.commit_all("b");
    repo.write("zzz", "ONE\ntwo\nTHREE\n");
    let mut terminal = Terminal::new(&repo);
    terminal.expect("zzz");
    terminal.key(b"\\", "Name filter");
    terminal.field("zzz", "Filter: zzz (1 matches)");
    terminal.key(b"i", "Unstaged hunk");
    terminal.key(b"=", "Changed-line numbers");
    terminal.field("1 2", "Updated selected lines in index");
    assert_eq!(
        repo.git(&["show", ":zzz"]).replace("\r\n", "\n"),
        "ONE\ntwo\nthree\n"
    );
    terminal.quit();
    repo.git(&["reset", "--hard", "HEAD"]);
    let mut terminal = Terminal::new(&repo);
    terminal.key(b"\t", "History");
    terminal.key(b"j", "a");
    terminal.key(b"j", "base");
    terminal.key(b"~", "Interactive rebase | oldest first");
    terminal.key(b"J", "Interactive rebase | oldest first");
    terminal.key(b"f", "fixup");
    terminal.key(b"v", "2 original commits -> 1 resulting commits");
    terminal.key(b"\r", "Type rebase");
    terminal.field("rebase", "Interactive rebase completed");
    assert_eq!(repo.git(&["rev-list", "--count", "HEAD"]).trim(), "2");
    terminal.quit();
}

#[test]
fn visual_lines_live_filters_and_selected_stash_in_conpty() {
    let repo = TestRepo::new();
    repo.write("aaa", "base\n");
    repo.write("zzz", "one\ntwo\nthree\n");
    repo.commit_all("base");
    repo.write("aaa", "other staged\n");
    repo.git(&["add", "aaa"]);
    repo.write("zzz", "ONE\ntwo\nTHREE\n");
    let mut terminal = Terminal::new(&repo);
    terminal.key(b"\\", "Name filter");
    terminal.key(b"zzz", "Filter: zzz (1 matches)");
    terminal.key(b"\r", "Filter: zzz (1 matches)");
    terminal.key(b"i", "Unstaged hunk");
    terminal.key(b" ", "Select lines | 0 selected");
    terminal.key(b"v", "1 selected");
    terminal.key(b"j", "2 selected");
    terminal.key(b"s", "Updated selected lines in index");
    assert_eq!(repo.git(&["show", ":zzz"]), "ONE\ntwo\nthree\n");
    terminal.key(b":", "Commands:");
    terminal.field("View files", "View: Files");
    terminal.key(b" ", "1 files selected for stash");
    terminal.key(b"$", "Selected stash message");
    terminal.field("only zzz", "Type stash");
    terminal.field("stash", "Saved selected files in stash");
    assert_eq!(repo.git(&["show", ":aaa"]), "other staged\n");
    assert_eq!(
        repo.git(&["diff", "stash@{0}^1", "stash@{0}", "--name-only"])
            .trim(),
        "zzz"
    );
    terminal.quit();
}
