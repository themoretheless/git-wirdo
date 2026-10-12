//! `App` depends on Git only through `GitRunner`, so it can be driven without a Git binary.
use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, ExitStatus, Output};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use git_wirdo::app::{Action, App};
use git_wirdo::git::{GitRunner, Repository};

/// Answers Git subcommands from a table and records every invocation.
#[derive(Debug, Default)]
struct FakeGit {
    answers: Vec<(&'static [&'static str], Result<&'static str, &'static str>)>,
    calls: Mutex<Vec<Vec<String>>>,
}

impl FakeGit {
    fn answering(
        answers: Vec<(&'static [&'static str], Result<&'static str, &'static str>)>,
    ) -> Arc<Self> {
        Arc::new(Self {
            answers,
            calls: Mutex::default(),
        })
    }

    fn ran(&self, subcommand: &str) -> bool {
        let calls = self.calls.lock().unwrap();
        calls
            .iter()
            .any(|args| args.first().map(String::as_str) == Some(subcommand))
    }
}

impl GitRunner for FakeGit {
    fn output(&self, command: &mut Command, _input: Option<&[u8]>) -> Result<Output> {
        // Everything after `-C <path>` is the Git subcommand and its arguments.
        let args: Vec<OsString> = command.get_args().map(ToOwned::to_owned).collect();
        let start = args.iter().position(|arg| arg == "-C").unwrap() + 2;
        let args: Vec<String> = args[start..]
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        self.calls.lock().unwrap().push(args.clone());
        let answer = self
            .answers
            .iter()
            .find(|(prefix, _)| {
                args.iter()
                    .map(String::as_str)
                    .take(prefix.len())
                    .eq(prefix.iter().copied())
            })
            .map(|(_, answer)| *answer)
            .unwrap_or(Ok(""));
        Ok(match answer {
            Ok(stdout) => output(0, stdout, ""),
            Err(stderr) => output(1, "", stderr),
        })
    }
}

fn output(code: i32, stdout: &str, stderr: &str) -> Output {
    Output {
        status: exit_status(code),
        stdout: stdout.as_bytes().to_vec(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

#[cfg(unix)]
fn exit_status(code: i32) -> ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    ExitStatus::from_raw(code << 8)
}

#[cfg(windows)]
fn exit_status(code: i32) -> ExitStatus {
    use std::os::windows::process::ExitStatusExt;
    ExitStatus::from_raw(code as u32)
}

fn discovery() -> Vec<(&'static [&'static str], Result<&'static str, &'static str>)> {
    vec![
        (&["rev-parse", "--is-inside-work-tree"], Ok("true\n")),
        (&["rev-parse", "--show-toplevel"], Ok("/fake/work\n")),
        (
            &["rev-parse", "--absolute-git-dir"],
            Ok("/fake/work/.git\n"),
        ),
        (
            &["symbolic-ref", "--quiet", "--short", "HEAD"],
            Ok("main\n"),
        ),
    ]
}

#[test]
fn app_opens_and_refreshes_through_an_injected_runner_without_git() {
    let fake = FakeGit::answering(discovery());
    let repository = Repository::open_with(Path::new("/fake/work"), fake.clone()).unwrap();
    assert_eq!(repository.root(), Path::new("/fake/work"));

    let mut app = App::new(repository).unwrap();
    assert_eq!(app.state.branch, "main");
    assert!(app.state.files.is_empty());
    assert!(
        fake.ran("status"),
        "the snapshot reads status through the runner"
    );

    app.handle(Action::Refresh);
    assert!(!app.message_is_error, "{}", app.message);
    assert_eq!(app.message, "Refreshed");
}

#[test]
fn git_failures_from_the_runner_become_recoverable_ui_errors() {
    let mut answers = discovery();
    answers.insert(0, (&["fetch"], Err("fatal: unable to access remote")));
    let fake = FakeGit::answering(answers);
    let mut app =
        App::new(Repository::open_with(Path::new("/fake/work"), fake.clone()).unwrap()).unwrap();

    app.handle(Action::Fetch);
    assert!(app.message_is_error);
    assert!(
        app.message.contains("unable to access remote"),
        "{}",
        app.message
    );
    assert!(app.running, "a failed Git command must not end the session");
    assert!(fake.ran("fetch"));
}

#[test]
fn initialization_and_cloning_run_git_through_the_injected_runner() {
    let base = std::env::temp_dir();
    let answers = || {
        vec![
            (
                &["rev-parse", "--absolute-git-dir"][..],
                Err("fatal: not a git repository"),
            ),
            (&["init"][..], Err("fatal: simulated init failure")),
            (&["clone"][..], Err("fatal: simulated clone failure")),
        ]
    };

    let fake = FakeGit::answering(answers());
    let destination = Path::new("git-wirdo-fake-init-never-created");
    let error = Repository::initialize_with(&base, destination, "main", fake.clone()).unwrap_err();
    assert!(
        format!("{error:#}").contains("simulated init failure"),
        "{error:#}"
    );
    assert!(fake.ran("check-ref-format") && fake.ran("init"));
    assert!(!base.join(destination).exists(), "no real Git ran");

    let fake = FakeGit::answering(answers());
    let destination = Path::new("git-wirdo-fake-clone-never-created");
    let error = Repository::clone_into_with(
        &base,
        std::ffi::OsStr::new("https://example.invalid/repo.git"),
        destination,
        fake.clone(),
    )
    .unwrap_err();
    assert!(
        format!("{error:#}").contains("simulated clone failure"),
        "{error:#}"
    );
    assert!(fake.ran("clone"));
    assert!(!base.join(destination).exists(), "no real Git ran");
}
