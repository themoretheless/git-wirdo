use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Arc;

use anyhow::{Context, Result, bail, ensure};

use crate::model::{
    BranchEntry, CommitEntry, FileEntry, MergeState, Operation, RepoState, display_path,
};

const PREVIEW_LIMIT: usize = 256 * 1024;
/// Commits initially loaded for history; `+` extends the window.
pub const HISTORY_PAGE: usize = 20;
/// Output must be machine-readable: no colors, external diff drivers or textconv filters.
const PLAIN: &[&str] = &["--no-color", "--no-ext-diff", "--no-textconv"];

/// Join argument groups so shared flag sets are declared once.
fn args<'a>(groups: &[&[&'a str]]) -> Vec<&'a str> {
    groups.concat()
}

#[derive(Debug, Clone)]
pub struct Repository {
    root: PathBuf,
    git_dir: PathBuf,
    runner: Arc<dyn GitRunner>,
}

/// Executes a fully prepared Git command and captures its output.
///
/// Every Git invocation of a `Repository` goes through its runner. The default,
/// [`ProcessRunner`], spawns the process under the background task's cancellation
/// control; tests can substitute canned output to drive the client without Git.
pub trait GitRunner: Send + Sync + std::fmt::Debug {
    fn output(&self, command: &mut Command, input: Option<&[u8]>) -> Result<Output>;
}

/// Runs Git as a cancellable child process.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessRunner;

impl GitRunner for ProcessRunner {
    fn output(&self, command: &mut Command, input: Option<&[u8]>) -> Result<Output> {
        crate::process::output_with_input(command, input)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileSnapshot {
    index: Vec<u8>,
    contents: Vec<(PathBuf, SavedContent)>,
    head: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SavedContent {
    Missing,
    Symlink(PathBuf),
    File {
        content_hash: Vec<u8>,
        readonly: bool,
        #[cfg(unix)]
        mode: u32,
    },
}

#[derive(Debug, Clone)]
pub struct RestoreRequest {
    pub file: FileEntry,
    pub from_head: bool,
    snapshot: FileSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
}
impl ResetMode {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "soft" => Ok(Self::Soft),
            "mixed" => Ok(Self::Mixed),
            "hard" => Ok(Self::Hard),
            _ => bail!("Choose soft, mixed or hard"),
        }
    }
    fn flag(self) -> &'static str {
        match self {
            Self::Soft => "--soft",
            Self::Mixed => "--mixed",
            Self::Hard => "--hard",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResetSnapshot {
    head: String,
    head_ref: Vec<u8>,
    status: Vec<u8>,
    index: Vec<u8>,
    flags: Vec<u8>,
    files: Vec<FileSnapshot>,
}
#[derive(Debug, Clone)]
pub struct ResetRequest {
    pub target: String,
    snapshot: ResetSnapshot,
}

#[derive(Debug, Clone, Copy)]
pub enum ConflictSide {
    Ours,
    Theirs,
}

mod branches;
mod history;
mod remotes;
mod stash;
mod tags;
mod worktree_files;
mod worktrees;

impl Repository {
    /// Create a working tree explicitly, without reinitializing existing Git metadata.
    pub fn initialize(base: &Path, destination: &Path, branch: &str) -> Result<Self> {
        Self::initialize_with(base, destination, branch, Arc::new(ProcessRunner))
    }

    /// [`Repository::initialize`] with every Git command, including discovery, run by `runner`.
    pub fn initialize_with(
        base: &Path,
        destination: &Path,
        branch: &str,
        runner: Arc<dyn GitRunner>,
    ) -> Result<Self> {
        run_in(&*runner, base, ["check-ref-format", "--branch", branch])?;
        let destination = lifecycle_destination(&*runner, base, destination)?;
        match fs::symlink_metadata(destination.join(".git")) {
            Ok(_) => bail!("Destination already contains Git metadata"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        run_in(
            &*runner,
            base,
            [
                OsStr::new("init"),
                OsStr::new("--initial-branch"),
                OsStr::new(branch),
                OsStr::new("--"),
                destination.as_os_str(),
            ],
        )
        .with_context(|| {
            format!(
                "Initialization failed at {}; existing files retained",
                display_path(&destination)
            )
        })?;
        Self::open_with(&destination, runner)
    }

    /// Clone any Git URL or local repository. Never remove a partial destination on failure.
    pub fn clone_into(base: &Path, source: &OsStr, destination: &Path) -> Result<Self> {
        Self::clone_into_with(base, source, destination, Arc::new(ProcessRunner))
    }

    /// [`Repository::clone_into`] with every Git command, including discovery, run by `runner`.
    pub fn clone_into_with(
        base: &Path,
        source: &OsStr,
        destination: &Path,
        runner: Arc<dyn GitRunner>,
    ) -> Result<Self> {
        ensure!(!source.is_empty(), "Clone source is required");
        let destination = lifecycle_destination(&*runner, base, destination)?;
        if destination.exists() {
            ensure!(
                fs::read_dir(&destination)?.next().is_none(),
                "Clone destination must be empty"
            );
        }
        run_in(
            &*runner,
            base,
            [
                OsStr::new("clone"),
                OsStr::new("--progress"),
                OsStr::new("--"),
                source,
                destination.as_os_str(),
            ],
        )
        .with_context(|| {
            format!(
                "Clone failed at {}; inspect any partial destination before retrying",
                display_path(&destination)
            )
        })?;
        Self::open_with(&destination, runner)
    }

    /// Resolve the working tree once, so all status paths and subsequent commands share a base.
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with(path, Arc::new(ProcessRunner))
    }

    /// Open a working tree whose Git commands, including discovery, run through `runner`.
    pub fn open_with(path: &Path, runner: Arc<dyn GitRunner>) -> Result<Self> {
        let inside = run_in(&*runner, path, ["rev-parse", "--is-inside-work-tree"])
            .with_context(|| format!("Cannot open repository at {}", display_path(path)))?;
        ensure!(
            String::from_utf8_lossy(&inside).trim() == "true",
            "Git Wirdo requires a non-bare working tree"
        );
        let root = output_path(&run_in(&*runner, path, ["rev-parse", "--show-toplevel"])?)?;
        let git_dir = output_path(&run_in(
            &*runner,
            &root,
            ["rev-parse", "--absolute-git-dir"],
        )?)?;
        Ok(Self {
            root,
            git_dir,
            runner,
        })
    }

    /// The runner this repository's Git commands use; repositories opened from it share it.
    pub fn runner(&self) -> Arc<dyn GitRunner> {
        self.runner.clone()
    }

    /// Run Git in this working tree and return stdout, failing on a non-zero exit.
    fn git<I, S>(&self, args: I) -> Result<Vec<u8>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        checked(self.git_output(args)?)
    }

    /// Run Git in this working tree and return its raw output, whatever the exit status.
    fn git_output<I, S>(&self, args: I) -> Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.git_with_input(args, None)
    }

    fn git_with_input<I, S>(&self, args: I, input: Option<&[u8]>) -> Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.runner
            .output(&mut git_command(&self.root, args), input)
            .with_context(|| format!("Failed to execute Git in {}", display_path(&self.root)))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Interpret a user-entered path relative to this working tree's root.
    pub fn resolve(&self, path: &Path) -> PathBuf {
        resolve_against(&self.root, path)
    }

    pub fn ensure_clean(&self) -> Result<()> {
        let state = self.load_state()?;
        ensure!(
            !state.merge_state.in_progress(),
            "Finish the current Git operation first"
        );
        ensure!(
            state.files.is_empty(),
            "Commit or stash working changes before this operation"
        );
        Ok(())
    }

    pub(super) fn text(&self, args: &[&str]) -> Result<String> {
        Ok(String::from_utf8_lossy(&self.git(args)?).into_owned())
    }

    pub(super) fn run_paths(&self, args: &[&str], paths: &[&Path]) -> Result<Vec<u8>> {
        let args: Vec<OsString> = std::iter::once(OsString::from("--literal-pathspecs"))
            .chain(args.iter().map(OsString::from))
            .chain(std::iter::once(OsString::from("--")))
            .chain(paths.iter().map(|path| path.as_os_str().to_owned()))
            .collect();
        self.git(args)
    }

    pub(super) fn path_text(&self, args: &[&str], paths: &[&Path]) -> Result<String> {
        Ok(String::from_utf8_lossy(&self.run_paths(args, paths)?).into_owned())
    }
}

fn validate_inspection_path(path: &Path) -> Result<()> {
    ensure!(
        !path.as_os_str().is_empty()
            && path.components().all(|part| matches!(
                part,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )),
        "File path must be relative to the repository root, without parent traversal"
    );
    Ok(())
}

fn git_command<I, S>(path: &Path, args: I) -> Command
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new("git");
    command
        .arg("--no-pager")
        .args([
            "-c",
            "color.ui=false",
            "-c",
            "core.quotePath=false",
            "-c",
            "log.showSignature=false",
        ])
        .arg("-C")
        .arg(path)
        .args(args)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        // Commands run inside a raw-mode TUI. Never start an interactive Git editor there.
        .env("GIT_EDITOR", "true")
        .env("GIT_SEQUENCE_EDITOR", "true");
    // --repo must win over repository-local variables inherited from hooks or shell scripts.
    for variable in [
        "GIT_LITERAL_PATHSPECS",
        "GIT_GLOB_PATHSPECS",
        "GIT_NOGLOB_PATHSPECS",
        "GIT_ICASE_PATHSPECS",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_NAMESPACE",
        "GIT_PREFIX",
        "GIT_SHALLOW_FILE",
    ] {
        command.env_remove(variable);
    }
    command
}

fn checked(output: Output) -> Result<Vec<u8>> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let message = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        bail!(
            "Git failed ({}): {}",
            output.status,
            if message.is_empty() {
                "no diagnostic output"
            } else {
                message
            }
        );
    }
    Ok(output.stdout)
}

/// Run Git in `path` through `runner`, failing on a non-zero exit. Used before a
/// `Repository` exists; afterwards its own methods carry the runner.
fn run_in<I, S>(runner: &dyn GitRunner, path: &Path, args: I) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = runner
        .output(&mut git_command(path, args), None)
        .with_context(|| format!("Failed to execute Git in {}", display_path(path)))?;
    checked(output)
}

/// NUL-terminated records of exactly `N` fields, as produced by `-z --format=...%x00...`.
fn nul_records<const N: usize>(bytes: &[u8], what: &str) -> Result<Vec<[String; N]>> {
    let text = String::from_utf8_lossy(bytes);
    let fields: Vec<_> = text.split_terminator('\0').collect();
    let (records, remainder) = fields.as_chunks::<N>();
    ensure!(remainder.is_empty(), "Invalid {what} record from Git");
    Ok(records.iter().map(|r| r.map(str::to_owned)).collect())
}

/// Newline-terminated records whose `N` fields are separated by NUL (`%00` in for-each-ref).
fn line_records<const N: usize>(text: &str, what: &str) -> Result<Vec<[String; N]>> {
    text.lines()
        .map(|line| {
            let fields: Vec<_> = line.splitn(N, '\0').collect();
            <[&str; N]>::try_from(fields)
                .map(|r| r.map(str::to_owned))
                .map_err(|_| anyhow::anyhow!("Invalid {what} record from Git"))
        })
        .collect()
}

fn utf8_line(bytes: &[u8]) -> Result<String> {
    Ok(std::str::from_utf8(bytes.strip_suffix(b"\n").unwrap_or(bytes))?.to_owned())
}

fn output_path(bytes: &[u8]) -> Result<PathBuf> {
    path_from_bytes(bytes.strip_suffix(b"\n").unwrap_or(bytes))
}

fn path_from_bytes(bytes: &[u8]) -> Result<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(PathBuf::from(OsString::from_vec(bytes.to_vec())))
    }
    #[cfg(not(unix))]
    {
        Ok(PathBuf::from(
            std::str::from_utf8(bytes).context("Git returned a non-UTF-8 path")?,
        ))
    }
}

/// Porcelain v1 -z is independent of user color/quoting settings and preserves filename bytes.
/// Rename/copy records contain the destination first, followed by a separate source record.
fn parse_status(bytes: &[u8]) -> Result<Vec<FileEntry>> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    ensure!(bytes.ends_with(&[0]), "Truncated Git status output");
    let mut records = bytes[..bytes.len() - 1].split(|byte| *byte == 0);
    let mut files = Vec::new();
    while let Some(record) = records.next() {
        ensure!(
            record.len() >= 4 && record[2] == b' ',
            "Invalid Git status record"
        );
        let status = std::str::from_utf8(&record[..2]).context("Invalid Git status code")?;
        ensure!(
            record[..2].iter().all(|byte| b" MADRCUT?!".contains(byte)),
            "Unknown Git status code"
        );
        let original_path = if record[..2]
            .iter()
            .any(|byte| *byte == b'R' || *byte == b'C')
        {
            let source = records
                .next()
                .context("Missing source path in Git rename record")?;
            ensure!(!source.is_empty(), "Empty source path in Git rename record");
            Some(path_from_bytes(source)?)
        } else {
            None
        };
        let conflicted = matches!(status, "DD" | "AU" | "UD" | "UA" | "DU" | "AA" | "UU");
        files.push(FileEntry {
            path: path_from_bytes(&record[3..])?,
            original_path,
            status: status.to_owned(),
            staged: !conflicted && !matches!(record[0], b' ' | b'?' | b'!'),
            unstaged: conflicted || !matches!(record[1], b' ' | b'!'),
            conflicted,
        });
    }
    Ok(files)
}

/// Validate before invoking Git: preserve files, reject symlinks and Git metadata targets.
/// Relative paths typed by the user are relative to the directory the client works in.
pub fn resolve_against(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        base.join(path)
    }
}

fn lifecycle_destination(runner: &dyn GitRunner, base: &Path, path: &Path) -> Result<PathBuf> {
    ensure!(!path.as_os_str().is_empty(), "Destination is required");
    let destination = if path.is_absolute() {
        path.to_owned()
    } else {
        // canonicalize adds a Windows verbatim prefix which Git's mkdir rejects.
        // Resolve the base without changing its native representation; canonicalize
        // only below when comparing existing metadata directories.
        let base = if base.is_absolute() {
            base.to_owned()
        } else {
            std::env::current_dir()?.join(base)
        };
        base.join(path)
    };
    match fs::symlink_metadata(&destination) {
        Ok(metadata) => ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Destination must be a directory, not a file or symlink"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let ancestor = destination
        .ancestors()
        .find(|path| path.exists())
        .context("Destination has no existing parent")?;
    ensure!(ancestor.is_dir(), "Destination parent must be a directory");
    let canonical = fs::canonicalize(ancestor)?;
    // rev-parse works inside metadata and bare repositories too; opening a working tree doesn't.
    if let Ok(output) = run_in(runner, ancestor, ["rev-parse", "--absolute-git-dir"]) {
        let metadata = fs::canonicalize(output_path(&output)?)?;
        ensure!(
            !canonical.starts_with(&metadata),
            "Destination is inside existing Git metadata"
        );
    }
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_preserves_columns_whitespace_and_rename_order() {
        let files =
            parse_status(b" M  leading and trailing \0R  new\nname\0old -> name\0?? next\0")
                .unwrap();
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].status, " M");
        assert_eq!(files[0].path, Path::new(" leading and trailing "));
        assert!(!files[0].staged && files[0].unstaged);
        assert_eq!(files[1].path, Path::new("new\nname"));
        assert_eq!(
            files[1].original_path.as_deref(),
            Some(Path::new("old -> name"))
        );
        assert!(files[1].staged && !files[1].unstaged);
        assert!(files[2].unstaged && !files[2].staged);
    }

    #[cfg(unix)]
    #[test]
    fn status_preserves_non_utf8_path_bytes_without_filesystem_assumptions() {
        use std::os::unix::ffi::OsStrExt;

        let files = parse_status(b"?? non-utf8-\xff.txt\0").unwrap();
        assert_eq!(files[0].path.as_os_str().as_bytes(), b"non-utf8-\xff.txt");
    }

    #[test]
    fn copies_do_not_include_the_source_in_mutating_pathspecs() {
        let files = parse_status(b"C  copy\0source\0").unwrap();
        assert_eq!(files[0].original_path.as_deref(), Some(Path::new("source")));
        assert_eq!(files[0].paths(), vec![Path::new("copy")]);
    }

    #[test]
    fn all_unmerged_statuses_are_conflicts_not_staged_files() {
        for status in ["DD", "AU", "UD", "UA", "DU", "AA", "UU"] {
            let files = parse_status(format!("{status} file\0").as_bytes()).unwrap();
            assert!(files[0].conflicted && files[0].unstaged && !files[0].staged);
        }
    }

    #[test]
    fn malformed_records_are_errors_not_silently_dropped() {
        for bytes in [
            b" M file".as_slice(),
            b"M\0",
            b"R  new\0",
            b"R  new\0\0",
            b"XX file\0",
            b" M \0",
        ] {
            assert!(parse_status(bytes).is_err(), "{bytes:?}");
        }
        assert!(parse_status(b"").unwrap().is_empty());
    }
}
