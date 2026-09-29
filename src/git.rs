use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use anyhow::{Context, Result, bail, ensure};

use crate::model::{BranchEntry, CommitEntry, FileEntry, MergeState, RepoState, display_path};

const PREVIEW_LIMIT: usize = 256 * 1024;

#[derive(Debug, Clone)]
pub struct Repository {
    root: PathBuf,
    git_dir: PathBuf,
}

#[derive(Debug, Clone, Copy)]
pub enum ConflictSide {
    Ours,
    Theirs,
}

impl Repository {
    /// Resolve the working tree once, so all status paths and subsequent commands share a base.
    pub fn open(path: &Path) -> Result<Self> {
        let inside = git(path, ["rev-parse", "--is-inside-work-tree"])
            .with_context(|| format!("Cannot open repository at {}", display_path(path)))?;
        ensure!(
            String::from_utf8_lossy(&inside).trim() == "true",
            "Git Wirdo requires a non-bare working tree"
        );
        let root = output_path(&git(path, ["rev-parse", "--show-toplevel"])?)?;
        let git_dir = output_path(&git(&root, ["rev-parse", "--absolute-git-dir"])?)?;
        Ok(Self { root, git_dir })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn load_state(&self) -> Result<RepoState> {
        let files = parse_status(&git(
            &self.root,
            ["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        )?)?;
        let merge_state = self.merge_state(&files)?;
        Ok(RepoState {
            branch: self.current_branch()?,
            files,
            branches: self.branches()?,
            commits: self.commits()?,
            merge_state,
        })
    }

    fn has_head(&self) -> Result<bool> {
        let output = git_output(&self.root, ["rev-parse", "--verify", "--quiet", "HEAD"])?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false), // An unborn branch is not an error.
            _ => checked(output).map(|_| false),
        }
    }

    fn current_branch(&self) -> Result<String> {
        let output = git_output(&self.root, ["symbolic-ref", "--quiet", "--short", "HEAD"])?;
        match output.status.code() {
            Some(0) => utf8_line(&output.stdout),
            Some(1) => {
                let sha = git(&self.root, ["rev-parse", "--short", "HEAD"])?;
                Ok(format!("HEAD ({})", utf8_line(&sha)?))
            }
            _ => checked(output).and_then(|bytes| utf8_line(&bytes)),
        }
    }

    fn branches(&self) -> Result<Vec<BranchEntry>> {
        let bytes = git(
            &self.root,
            [
                "for-each-ref",
                "--format=%(HEAD)%00%(refname:strip=2)",
                "refs/heads/",
            ],
        )?;
        let text = std::str::from_utf8(&bytes).context("Git returned a non-UTF-8 branch name")?;
        text.lines()
            .map(|line| {
                let (marker, name) = line
                    .split_once('\0')
                    .context("Invalid branch record from Git")?;
                Ok(BranchEntry {
                    name: name.to_owned(),
                    current: marker == "*",
                })
            })
            .collect()
    }

    fn commits(&self) -> Result<Vec<CommitEntry>> {
        if !self.has_head()? {
            return Ok(Vec::new());
        }
        let bytes = git(
            &self.root,
            [
                "log",
                "--no-color",
                "-z",
                "-n",
                "20",
                "--date=short",
                "--format=%H%x00%h%x00%ad%x00%an%x00%s",
            ],
        )?;
        let text = String::from_utf8_lossy(&bytes);
        let fields: Vec<_> = text.split_terminator('\0').collect();
        let (records, remainder) = fields.as_chunks::<5>();
        ensure!(remainder.is_empty(), "Invalid commit record from Git");
        Ok(records
            .iter()
            .map(|parts| CommitEntry {
                sha: parts[0].to_owned(),
                short_sha: parts[1].to_owned(),
                date: parts[2].to_owned(),
                author: parts[3].to_owned(),
                subject: parts[4].to_owned(),
            })
            .collect())
    }

    fn merge_state(&self, files: &[FileEntry]) -> Result<MergeState> {
        // A linked worktree has a .git *file*, and its operation state lives in its own git dir.
        Ok(MergeState {
            merge_in_progress: self.git_dir.join("MERGE_HEAD").try_exists()?,
            rebase_in_progress: self.git_dir.join("rebase-merge").try_exists()?
                || self.git_dir.join("rebase-apply").try_exists()?,
            conflicts: files
                .iter()
                .filter(|file| file.conflicted)
                .map(|file| file.path.clone())
                .collect(),
        })
    }

    pub fn file_detail(&self, file: &FileEntry) -> Result<String> {
        if file.status == "??" {
            return self.untracked_preview(&file.path);
        }
        let paths = file.paths();
        let staged = self.path_text(
            &[
                "diff",
                "--no-color",
                "--no-ext-diff",
                "--no-textconv",
                "--cached",
            ],
            &paths,
        )?;
        let unstaged = self.path_text(
            &["diff", "--no-color", "--no-ext-diff", "--no-textconv"],
            &paths,
        )?;
        let mut chunks = Vec::new();
        if !staged.is_empty() {
            chunks.push(format!("[STAGED]\n{staged}"));
        }
        if !unstaged.is_empty() {
            chunks.push(format!("[UNSTAGED]\n{unstaged}"));
        }
        Ok(if chunks.is_empty() {
            "No diff for this file".to_owned()
        } else {
            chunks.join("\n")
        })
    }

    fn untracked_preview(&self, path: &Path) -> Result<String> {
        let full_path = self.root.join(path);
        let metadata = fs::symlink_metadata(&full_path)
            .with_context(|| format!("Cannot preview {}", display_path(path)))?;
        if metadata.file_type().is_symlink() {
            return Ok(format!(
                "[UNTRACKED SYMLINK]\n{}",
                display_path(&fs::read_link(full_path)?)
            ));
        }
        if !metadata.is_file() {
            return Ok("Untracked directory or special file; no text preview".to_owned());
        }
        let mut bytes = Vec::new();
        fs::File::open(full_path)?
            .take((PREVIEW_LIMIT + 1) as u64)
            .read_to_end(&mut bytes)?;
        let truncated = bytes.len() > PREVIEW_LIMIT;
        bytes.truncate(PREVIEW_LIMIT);
        let text = match std::str::from_utf8(&bytes) {
            Ok(text) if !bytes.contains(&0) => text,
            // A bounded read can end in the middle of a valid UTF-8 character.
            Err(error) if truncated && error.error_len().is_none() && !bytes.contains(&0) => {
                std::str::from_utf8(&bytes[..error.valid_up_to()])?
            }
            _ => return Ok("[UNTRACKED]\nBinary or non-UTF-8 file; no text preview".to_owned()),
        };
        let mut text = text.to_owned();
        if truncated {
            text.push_str("\n[Preview truncated at 256 KiB]");
        }
        Ok(format!("[UNTRACKED]\n{text}"))
    }

    pub fn commit_detail(&self, commit: &CommitEntry) -> Result<String> {
        self.text(&[
            "show",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--stat",
            "--decorate=short",
            "--oneline",
            &commit.sha,
            "--",
        ])
    }

    pub fn conflict_detail(&self, path: &Path) -> Result<String> {
        self.path_text(
            &["diff", "--no-color", "--no-ext-diff", "--no-textconv"],
            &[path],
        )
    }

    pub fn stage(&self, file: &FileEntry) -> Result<()> {
        if !file.unstaged && !file.conflicted {
            return Ok(());
        }
        // An index rename already staged its source deletion. Only the destination needs adding.
        self.run_paths(&["add", "--all"], &[&file.path])?;
        Ok(())
    }

    pub fn unstage(&self, file: &FileEntry) -> Result<()> {
        if !file.staged && !file.conflicted {
            return Ok(());
        }
        if self.has_head()? {
            // Both names are necessary: resetting only the destination leaves the old name deleted.
            self.run_paths(&["reset", "--quiet", "HEAD"], &file.paths())?;
        } else {
            // HEAD does not exist before the first commit. Never remove the working copy.
            self.run_paths(&["rm", "--cached", "--force"], &[&file.path])?;
        }
        Ok(())
    }

    pub fn commit(&self, message: &str) -> Result<()> {
        ensure!(!message.trim().is_empty(), "Commit message cannot be empty");
        git(&self.root, ["commit", "-m", message])?;
        Ok(())
    }

    pub fn switch_branch(&self, name: &str) -> Result<()> {
        git(&self.root, ["switch", "--no-guess", "--", name])?;
        Ok(())
    }

    pub fn switch_or_create_branch(&self, name: &str) -> Result<()> {
        git(&self.root, ["check-ref-format", "--branch", name])?;
        let reference = format!("refs/heads/{name}");
        let output = git_output(&self.root, ["show-ref", "--verify", "--quiet", &reference])?;
        match output.status.code() {
            Some(0) => self.switch_branch(name),
            Some(1) => {
                git(&self.root, ["switch", "-c", name])?;
                Ok(())
            }
            _ => checked(output).map(|_| ()),
        }
    }

    pub fn resolve_side(&self, path: &Path, side: ConflictSide) -> Result<()> {
        let entries = self.run_paths(&["ls-files", "--unmerged", "-z"], &[path])?;
        ensure!(
            !entries.is_empty(),
            "{} is no longer conflicted; refresh first",
            display_path(path)
        );
        let (stage, flag) = match side {
            ConflictSide::Ours => (b'2', "--ours"),
            ConflictSide::Theirs => (b'3', "--theirs"),
        };
        let side_exists = entries.split(|byte| *byte == 0).any(|entry| {
            let metadata = entry
                .split(|byte| *byte == b'\t')
                .next()
                .unwrap_or_default();
            metadata.last() == Some(&stage)
        });
        if side_exists {
            self.run_paths(&["checkout", flag], &[path])?;
            self.mark_resolved(path)?;
        } else {
            // In a modify/delete conflict, a missing stage means the chosen side deleted the file.
            self.run_paths(&["rm", "--force"], &[path])?;
        }
        Ok(())
    }

    pub fn mark_resolved(&self, path: &Path) -> Result<()> {
        self.run_paths(&["add", "--all"], &[path])?;
        Ok(())
    }

    pub fn continue_operation(&self) -> Result<()> {
        let state = self.load_state()?.merge_state;
        ensure!(
            state.conflicts.is_empty(),
            "Resolve and stage all conflicts before continuing"
        );
        if state.rebase_in_progress {
            git(&self.root, ["rebase", "--continue"])?;
        } else if state.merge_in_progress {
            git(&self.root, ["commit", "--no-edit"])?;
        } else {
            bail!("No merge or rebase in progress");
        }
        Ok(())
    }

    pub fn skip_rebase(&self) -> Result<()> {
        ensure!(
            self.merge_state(&[])?.rebase_in_progress,
            "No rebase in progress"
        );
        git(&self.root, ["rebase", "--skip"])?;
        Ok(())
    }

    pub fn abort_operation(&self) -> Result<()> {
        let state = self.merge_state(&[])?;
        if state.rebase_in_progress {
            git(&self.root, ["rebase", "--abort"])?;
        } else if state.merge_in_progress {
            git(&self.root, ["merge", "--abort"])?;
        } else {
            bail!("No merge or rebase in progress");
        }
        Ok(())
    }

    pub fn fetch(&self) -> Result<()> {
        git(&self.root, ["fetch", "--all"])?;
        Ok(())
    }

    pub fn pull(&self) -> Result<()> {
        git(&self.root, ["pull", "--ff-only"])?;
        Ok(())
    }

    pub fn push(&self) -> Result<()> {
        git(&self.root, ["push"])?;
        Ok(())
    }

    fn text(&self, args: &[&str]) -> Result<String> {
        Ok(String::from_utf8_lossy(&git(&self.root, args)?).into_owned())
    }

    fn run_paths(&self, args: &[&str], paths: &[&Path]) -> Result<Vec<u8>> {
        let args: Vec<OsString> = args
            .iter()
            .map(OsString::from)
            .chain(std::iter::once(OsString::from("--")))
            .chain(paths.iter().map(|path| path.as_os_str().to_owned()))
            .collect();
        git(&self.root, args)
    }

    fn path_text(&self, args: &[&str], paths: &[&Path]) -> Result<String> {
        Ok(String::from_utf8_lossy(&self.run_paths(args, paths)?).into_owned())
    }
}

fn git_output<I, S>(path: &Path, args: I) -> Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new("git");
    command
        .arg("--no-pager")
        .arg("--literal-pathspecs")
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
        .output()
        .with_context(|| format!("Failed to execute Git in {}", display_path(path)))
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

fn git<I, S>(path: &Path, args: I) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    checked(git_output(path, args)?)
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
