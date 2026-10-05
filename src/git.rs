use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{Read, Write};
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

    pub fn workspaces(&self) -> Result<Vec<crate::model::Workspace>> {
        let bytes = git(&self.root, ["worktree", "list", "--porcelain", "-z"])?;
        let mut result = Vec::new();
        let mut current: Option<crate::model::Workspace> = None;
        for record in bytes.split(|b| *b == 0) {
            if let Some(path) = record.strip_prefix(b"worktree ") {
                if let Some(entry) = current.take() {
                    result.push(entry);
                }
                current = Some(crate::model::Workspace {
                    path: path_from_bytes(path)?,
                    branch: "detached".into(),
                    locked: false,
                    bare: false,
                });
            } else if let Some(entry) = &mut current {
                if let Some(branch) = record.strip_prefix(b"branch refs/heads/") {
                    entry.branch = utf8_line(branch)?;
                }
                entry.locked |= record == b"locked" || record.starts_with(b"locked ");
                entry.bare |= record == b"bare";
            }
        }
        if let Some(entry) = current {
            result.push(entry);
        }
        Ok(result)
    }

    pub fn create_workspace(&self, path: &Path, branch: &str) -> Result<()> {
        git(&self.root, ["check-ref-format", "--branch", branch])?;
        let path = if path.is_absolute() {
            path.to_owned()
        } else {
            self.root.join(path)
        };
        git(
            &self.root,
            [
                OsString::from("worktree"),
                OsString::from("add"),
                OsString::from("-b"),
                OsString::from(branch),
                OsString::from("--"),
                path.into_os_string(),
                OsString::from("HEAD"),
            ],
        )?;
        Ok(())
    }

    pub fn remove_workspace(&self, workspace: &crate::model::Workspace) -> Result<()> {
        ensure!(
            !workspace.locked && !workspace.bare,
            "Locked or bare workspace cannot be removed"
        );
        ensure!(
            workspace.path != self.root,
            "Cannot remove the current workspace"
        );
        // Git refuses the primary checkout, dirty worktrees and locked worktrees. Never force.
        git(
            &self.root,
            [
                OsString::from("worktree"),
                OsString::from("remove"),
                OsString::from("--"),
                workspace.path.as_os_str().to_owned(),
            ],
        )?;
        Ok(())
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

    pub fn stashes(&self) -> Result<Vec<crate::model::StashEntry>> {
        let bytes = git(
            &self.root,
            ["stash", "list", "-z", "--format=%gd%x00%H%x00%s"],
        )?;
        let text = std::str::from_utf8(&bytes).context("Invalid UTF-8 stash metadata")?;
        let fields: Vec<_> = text.split_terminator('\0').collect();
        let (records, remainder) = fields.as_chunks::<3>();
        ensure!(remainder.is_empty(), "Invalid stash list record");
        Ok(records
            .iter()
            .map(|r| crate::model::StashEntry {
                selector: r[0].into(),
                sha: r[1].into(),
                subject: r[2].into(),
            })
            .collect())
    }

    pub fn stash_detail(&self, stash: &crate::model::StashEntry) -> Result<String> {
        self.text(&[
            "stash",
            "show",
            "--include-untracked",
            "--patch",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            &stash.sha,
        ])
    }

    pub fn save_stash(&self, message: &str) -> Result<()> {
        let state = self.load_state()?;
        ensure!(
            !state.merge_state.in_progress() && state.merge_state.conflicts.is_empty(),
            "Finish the current operation before saving a stash"
        );
        ensure!(!state.files.is_empty(), "No changes to stash");
        git(
            &self.root,
            ["stash", "push", "--include-untracked", "--message", message],
        )?;
        Ok(())
    }

    fn verify_stash(&self, stash: &crate::model::StashEntry) -> Result<()> {
        ensure!(
            self.stashes()?
                .iter()
                .any(|current| current.selector == stash.selector && current.sha == stash.sha),
            "Stash list changed; refresh and select the entry again"
        );
        Ok(())
    }

    pub fn apply_stash(&self, stash: &crate::model::StashEntry, pop: bool) -> Result<()> {
        self.ensure_clean()?;
        self.verify_stash(stash)?;
        // Native pop retains the reflog entry on conflict. --index restores staged changes.
        git(
            &self.root,
            [
                "stash",
                if pop { "pop" } else { "apply" },
                "--index",
                &stash.selector,
            ],
        )?;
        Ok(())
    }

    pub fn drop_stash(&self, stash: &crate::model::StashEntry) -> Result<()> {
        self.verify_stash(stash)?;
        git(&self.root, ["stash", "drop", &stash.selector])?;
        Ok(())
    }

    pub fn tags(&self) -> Result<Vec<crate::model::TagEntry>> {
        let text = self.text(&[
            "for-each-ref",
            "--sort=-creatordate",
            "--format=%(refname:strip=2)%00%(objectname)%00%(objecttype)%00%(subject)",
            "refs/tags/",
        ])?;
        text.lines()
            .map(|line| {
                let fields: Vec<_> = line.splitn(4, '\0').collect();
                ensure!(fields.len() == 4, "Invalid tag record");
                Ok(crate::model::TagEntry {
                    name: fields[0].into(),
                    sha: fields[1].into(),
                    kind: fields[2].into(),
                    subject: fields[3].into(),
                })
            })
            .collect()
    }

    pub fn tag_detail(&self, tag: &crate::model::TagEntry) -> Result<String> {
        self.text(&[
            "show",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--stat",
            &tag.sha,
            "--",
        ])
    }

    pub fn create_tag(&self, name: &str, target: &str, message: &str) -> Result<()> {
        let reference = format!("refs/tags/{name}");
        git(&self.root, ["check-ref-format", &reference])?;
        let target = if target.trim().is_empty() {
            "HEAD"
        } else {
            target
        };
        let target = format!("{target}^{{commit}}");
        let sha = self.text(&["rev-parse", "--verify", "--end-of-options", &target])?;
        if message.trim().is_empty() {
            git(&self.root, ["tag", "--no-sign", "--", name, sha.trim()])?;
        } else {
            git(
                &self.root,
                ["tag", "-a", "-m", message, "--", name, sha.trim()],
            )?;
        }
        Ok(())
    }

    fn verify_tag(&self, tag: &crate::model::TagEntry) -> Result<()> {
        ensure!(
            self.tags()?
                .iter()
                .any(|t| t.name == tag.name && t.sha == tag.sha),
            "Tag changed or disappeared; refresh first"
        );
        Ok(())
    }

    pub fn delete_tag(&self, tag: &crate::model::TagEntry) -> Result<()> {
        // Compare-and-swap ref deletion refuses a tag changed since selection.
        let reference = format!("refs/tags/{}", tag.name);
        git(&self.root, ["update-ref", "-d", &reference, &tag.sha])?;
        Ok(())
    }

    pub fn publish_tag(&self, tag: &crate::model::TagEntry, remote: &str) -> Result<()> {
        self.verify_remote(remote)?;
        self.verify_tag(tag)?;
        let reference = format!("refs/tags/{}", tag.name);
        // Publish the selected object, not an unrelated tag or a later replacement.
        let spec = format!("{}:{reference}", tag.sha);
        git(&self.root, ["push", "--", remote, &spec])?;
        Ok(())
    }

    pub fn delete_remote_tag(&self, tag: &crate::model::TagEntry, remote: &str) -> Result<()> {
        self.verify_remote(remote)?;
        let reference = format!("refs/tags/{}", tag.name);
        let lease = format!("--force-with-lease={reference}:{}", tag.sha);
        let spec = format!(":{reference}");
        git(&self.root, ["push", &lease, "--", remote, &spec])?;
        Ok(())
    }

    pub fn remotes(&self) -> Result<Vec<crate::model::RemoteEntry>> {
        self.text(&["remote"])?
            .lines()
            .map(|name| {
                Ok(crate::model::RemoteEntry {
                    name: name.into(),
                    fetch_urls: self.text(&["remote", "get-url", "--all", name])?,
                    push_urls: self.text(&["remote", "get-url", "--push", "--all", name])?,
                })
            })
            .collect()
    }

    fn verify_remote(&self, name: &str) -> Result<()> {
        ensure!(
            self.remotes()?.iter().any(|r| r.name == name),
            "Remote no longer exists; refresh first"
        );
        Ok(())
    }

    pub fn add_remote(&self, name: &str, url: &str) -> Result<()> {
        ensure!(
            !name.is_empty() && !name.starts_with('-') && !url.trim().is_empty(),
            "Remote name and URL are required"
        );
        git(&self.root, ["remote", "add", "--", name, url])?;
        Ok(())
    }

    pub fn edit_remote(&self, name: &str, url: &str) -> Result<()> {
        self.verify_remote(name)?;
        ensure!(!url.trim().is_empty(), "URL is required");
        git(&self.root, ["remote", "set-url", "--", name, url])?;
        Ok(())
    }

    pub fn edit_push_url(&self, name: &str, url: &str) -> Result<()> {
        self.verify_remote(name)?;
        ensure!(!url.trim().is_empty(), "Push URL is required");
        git(&self.root, ["remote", "set-url", "--push", "--", name, url])?;
        Ok(())
    }

    pub fn remove_remote(&self, name: &str) -> Result<()> {
        self.verify_remote(name)?;
        git(&self.root, ["remote", "remove", "--", name])?;
        Ok(())
    }

    pub fn remote_branches(&self) -> Result<Vec<String>> {
        let text = self.text(&[
            "for-each-ref",
            "--format=%(refname:strip=2)%00%(symref)",
            "refs/remotes/",
        ])?;
        Ok(text
            .lines()
            .filter_map(|line| line.split_once('\0'))
            .filter(|(_, symref)| symref.is_empty())
            .map(|(name, _)| name.to_owned())
            .collect())
    }

    pub fn checkout_remote(&self, remote_branch: &str, local: &str) -> Result<()> {
        ensure!(
            self.remote_branches()?.iter().any(|b| b == remote_branch),
            "Remote branch no longer exists"
        );
        git(&self.root, ["check-ref-format", "--branch", local])?;
        let reference = format!("refs/remotes/{remote_branch}");
        git(&self.root, ["switch", "--track", "-c", local, &reference])?;
        Ok(())
    }

    pub fn tracking(&self) -> Result<crate::model::Tracking> {
        let branch = self.current_branch()?;
        let reference = format!("refs/heads/{branch}");
        let upstream = self
            .text(&["for-each-ref", "--format=%(upstream:short)", &reference])?
            .trim()
            .to_owned();
        if upstream.is_empty() {
            return Ok(crate::model::Tracking::default());
        }
        let range = format!("HEAD...{upstream}");
        let counts = self.text(&["rev-list", "--left-right", "--count", &range])?;
        let mut counts = counts.split_whitespace();
        Ok(crate::model::Tracking {
            error: None,
            upstream: Some(upstream),
            ahead: counts.next().context("Missing ahead count")?.parse()?,
            behind: counts.next().context("Missing behind count")?.parse()?,
        })
    }

    pub fn set_upstream(&self, reference: &str) -> Result<()> {
        if reference.trim().is_empty() {
            git(&self.root, ["branch", "--unset-upstream"])?;
            return Ok(());
        }
        let arg = format!("--set-upstream-to={reference}");
        git(&self.root, ["branch", &arg])?;
        Ok(())
    }

    pub fn publish_branch(&self, remote: &str) -> Result<()> {
        self.verify_remote(remote)?;
        let branch = self.current_branch()?;
        let reference = format!("refs/heads/{branch}");
        git(&self.root, ["show-ref", "--verify", &reference])?;
        git(
            &self.root,
            ["push", "--set-upstream", "--", remote, &reference],
        )?;
        Ok(())
    }

    pub fn pull_strategy(&self, strategy: &str) -> Result<()> {
        self.ensure_clean()?;
        match strategy {
            "ff-only" => {
                git(&self.root, ["pull", "--ff-only"])?;
            }
            "merge" => {
                git(&self.root, ["pull", "--no-rebase", "--ff", "--no-edit"])?;
            }
            "rebase" => {
                git(&self.root, ["pull", "--rebase", "--ff"])?;
            }
            _ => bail!("Choose ff-only, merge or rebase"),
        }
        Ok(())
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
            commits: self.history(20)?,
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

    pub fn history_graph(&self, limit: usize) -> Result<String> {
        if !self.has_head()? {
            return Ok(String::new());
        }
        self.text(&[
            "log",
            "--no-color",
            "--no-show-signature",
            "--all",
            "--topo-order",
            "--graph",
            "-n",
            &limit.to_string(),
            "--decorate=short",
            "--format=%H %s %d",
        ])
    }

    pub fn history(&self, limit: usize) -> Result<Vec<CommitEntry>> {
        let limit = limit.to_string();
        if !self.has_head()? {
            return Ok(Vec::new());
        }
        let bytes = git(
            &self.root,
            [
                "log",
                "--no-color",
                "--no-show-signature",
                "-z",
                "-n",
                &limit,
                "--all",
                "--topo-order",
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
            cherry_pick_in_progress: self.git_dir.join("CHERRY_PICK_HEAD").try_exists()?,
            revert_in_progress: self.git_dir.join("REVERT_HEAD").try_exists()?,
            conflicts: files
                .iter()
                .filter(|file| file.conflicted)
                .map(|file| file.path.clone())
                .collect(),
        })
    }

    fn file_snapshot(&self, file: &FileEntry) -> Result<FileSnapshot> {
        let mut contents = Vec::new();
        for path in file.paths() {
            let full = self.root.join(path);
            let content = match fs::symlink_metadata(&full) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    SavedContent::Symlink(fs::read_link(&full)?)
                }
                Ok(metadata) if metadata.is_file() => SavedContent::File {
                    content_hash: self.run_paths(&["hash-object", "--no-filters"], &[path])?,
                    readonly: metadata.permissions().readonly(),
                    #[cfg(unix)]
                    mode: {
                        use std::os::unix::fs::PermissionsExt;
                        metadata.permissions().mode()
                    },
                },
                Ok(_) => bail!("Cannot restore directories or special files"),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => SavedContent::Missing,
                Err(error) => return Err(error.into()),
            };
            contents.push((path.to_owned(), content));
        }
        let head = if self.has_head()? {
            Some(self.text(&["rev-parse", "HEAD"])?.trim().into())
        } else {
            None
        };
        Ok(FileSnapshot {
            index: self.run_paths(&["ls-files", "--stage", "-z"], &file.paths())?,
            contents,
            head,
        })
    }

    pub fn prepare_restore(&self, file: &FileEntry, from_head: bool) -> Result<RestoreRequest> {
        ensure!(
            self.load_state()?
                .files
                .iter()
                .any(|current| current == file),
            "File status changed; refresh first"
        );
        ensure!(
            !file.conflicted,
            "Use conflict resolution for conflicted files"
        );
        let snapshot = self.file_snapshot(file)?;
        ensure!(
            !from_head || file.status == "??" || snapshot.head.is_some(),
            "HEAD does not exist; unstage the initial file instead"
        );
        Ok(RestoreRequest {
            file: file.clone(),
            from_head,
            snapshot,
        })
    }

    pub fn restore_file(&self, request: &RestoreRequest) -> Result<()> {
        ensure!(
            self.file_snapshot(&request.file)? == request.snapshot,
            "File or index changed; refresh and confirm again"
        );
        if request.file.status == "??" {
            // Remove only the selected file or symlink. Never follow a symlink or recurse.
            fs::remove_file(self.root.join(&request.file.path))?;
        } else if request.from_head {
            let source = format!(
                "--source={}",
                request
                    .snapshot
                    .head
                    .as_deref()
                    .context("HEAD unavailable")?
            );
            self.run_paths(
                &["restore", &source, "--staged", "--worktree"],
                &request.file.paths(),
            )?;
        } else {
            self.run_paths(&["restore", "--worktree"], &[&request.file.path])?;
        }
        Ok(())
    }

    fn hunk_diff(&self, file: &FileEntry, staged: bool) -> Result<Vec<u8>> {
        if file.conflicted || file.original_path.is_some() || file.status == "??" {
            return Ok(Vec::new());
        }
        let mut args = vec![
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            "--unified=3",
            "--inter-hunk-context=0",
        ];
        if staged {
            args.push("--cached");
        }
        self.run_paths(&args, &[&file.path])
    }

    pub fn hunks(&self, file: &FileEntry, staged: bool) -> Result<Vec<crate::patch::Hunk>> {
        Ok(crate::patch::split(
            file,
            staged,
            self.hunk_diff(file, staged)?,
        ))
    }

    pub fn apply_hunk(&self, hunk: &crate::patch::Hunk) -> Result<()> {
        ensure!(
            self.hunk_diff(&hunk.file, hunk.staged)? == hunk.snapshot,
            "Diff changed; refresh before applying a hunk"
        );
        let mut args = vec!["apply", "--cached", "--whitespace=nowarn"];
        if hunk.staged {
            args.push("--reverse");
        }
        args.push("-");
        let mut child = git_command(&self.root, &args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let written = child
            .stdin
            .take()
            .context("Git apply stdin unavailable")?
            .write_all(&hunk.patch);
        let output = child.wait_with_output()?;
        checked(output)?;
        written?;
        Ok(())
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

    /// Compare the working tree to the merge base with its configured upstream.
    pub fn upstream_files(&self) -> Result<Vec<FileEntry>> {
        let base = self.upstream_base()?;
        let bytes = git(
            &self.root,
            ["diff", "--name-status", "--no-renames", "-z", &base, "--"],
        )?;
        let mut records = bytes.split(|byte| *byte == 0);
        let mut files = Vec::new();
        while let Some(status) = records.next().filter(|status| !status.is_empty()) {
            let path = records.next().context("Missing upstream diff path")?;
            files.push(FileEntry {
                path: path_from_bytes(path)?,
                original_path: None,
                status: utf8_line(status)?,
                staged: false,
                unstaged: false,
                conflicted: false,
            });
        }
        Ok(files)
    }

    fn upstream_base(&self) -> Result<String> {
        let upstream = self
            .text(&["rev-parse", "--symbolic-full-name", "@{upstream}"])
            .context("Configure an upstream branch before comparing changes")?;
        Ok(self
            .text(&["merge-base", "HEAD", upstream.trim()])?
            .trim()
            .to_owned())
    }

    pub fn upstream_detail(&self, file: &FileEntry) -> Result<String> {
        let base = self.upstream_base()?;
        self.path_text(
            &[
                "diff",
                "--no-color",
                "--no-ext-diff",
                "--no-textconv",
                "--no-renames",
                &base,
            ],
            &[&file.path],
        )
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

    pub fn amend(&self, message: &str) -> Result<()> {
        ensure!(!message.trim().is_empty(), "Commit message cannot be empty");
        git(&self.root, ["commit", "--amend", "-m", message])?;
        Ok(())
    }

    pub fn rename_branch(&self, old: &str, new: &str) -> Result<()> {
        git(&self.root, ["check-ref-format", "--branch", new])?;
        git(&self.root, ["branch", "-m", "--", old, new])?;
        Ok(())
    }

    pub fn delete_branch(&self, branch: &str) -> Result<()> {
        ensure!(
            branch != self.current_branch()?,
            "Cannot delete the current branch"
        );
        ensure!(
            !branch.starts_with("release/") && !branch.starts_with("hotfix/"),
            "Release and hotfix branches are protected"
        );
        // Explicit ancestry check avoids branch -d accepting commits merged only in its upstream.
        let tip = format!("refs/heads/{branch}");
        let output = git_output(&self.root, ["merge-base", "--is-ancestor", &tip, "HEAD"])?;
        ensure!(
            output.status.success(),
            "Branch contains commits not merged into the current HEAD"
        );
        git(&self.root, ["branch", "-d", "--", branch])?;
        Ok(())
    }

    pub fn integrate_branch(&self, branch: &str, rebase: bool) -> Result<()> {
        self.ensure_clean()?;
        let reference = format!("refs/heads/{branch}");
        git(&self.root, ["show-ref", "--verify", &reference])?;
        if rebase {
            git(&self.root, ["rebase", &reference])?;
        } else {
            git(&self.root, ["merge", "--no-edit", &reference])?;
        }
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

    fn verify_commit_id(&self, sha: &str) -> Result<()> {
        ensure!(
            matches!(sha.len(), 40 | 64) && sha.bytes().all(|b| b.is_ascii_hexdigit()),
            "Select a full commit ID"
        );
        let target = format!("{sha}^{{commit}}");
        ensure!(
            self.text(&["rev-parse", "--verify", "--end-of-options", &target])?
                .trim()
                == sha,
            "Selected commit changed"
        );
        Ok(())
    }

    pub fn reflog(&self, limit: usize) -> Result<Vec<crate::model::ReflogEntry>> {
        let bytes = if self.has_head()? {
            git(
                &self.root,
                [
                    "reflog",
                    "show",
                    "--no-color",
                    "--no-show-signature",
                    "-z",
                    "-n",
                    &limit.to_string(),
                    "--format=%H%x00%gD%x00%gs",
                    "HEAD",
                    "--",
                ],
            )?
        } else {
            Vec::new()
        };
        let text = String::from_utf8_lossy(&bytes);
        let fields: Vec<_> = text.split_terminator('\0').collect();
        let (records, remainder) = fields.as_chunks::<3>();
        ensure!(remainder.is_empty(), "Invalid reflog record from Git");
        let mut entries: Vec<_> = records
            .iter()
            .map(|p| crate::model::ReflogEntry {
                sha: p[0].into(),
                selector: p[1].into(),
                subject: p[2].into(),
            })
            .collect();
        // Persistent pre-reset references remain discoverable even after HEAD reflog expires.
        let backups = self.text(&[
            "for-each-ref",
            "--sort=-refname",
            &format!("--count={limit}"),
            "--format=%(objectname)%00%(refname)%00%(subject)",
            "refs/git-wirdo/recovery/",
        ])?;
        for line in backups.lines() {
            let fields: Vec<_> = line.split('\0').collect();
            ensure!(
                fields.len() == 3,
                "Invalid recovery reference record from Git"
            );
            entries.push(crate::model::ReflogEntry {
                sha: fields[0].into(),
                selector: fields[1].into(),
                subject: fields[2].into(),
            });
        }
        Ok(entries)
    }

    pub fn recover_commit(&self, sha: &str, branch: &str) -> Result<()> {
        self.verify_commit_id(sha)?;
        ensure!(
            !branch.trim().is_empty(),
            "Enter a new recovery branch name"
        );
        git(&self.root, ["check-ref-format", "--branch", branch])?;
        git(&self.root, ["branch", "--", branch, sha])?;
        Ok(())
    }

    fn reset_snapshot(&self) -> Result<ResetSnapshot> {
        let state = self.load_state()?;
        ensure!(
            !state.merge_state.in_progress() && state.merge_state.conflicts.is_empty(),
            "Finish the current Git operation first"
        );
        ensure!(self.has_head()?, "HEAD does not exist");
        let head_ref = git_output(&self.root, ["symbolic-ref", "--quiet", "HEAD"])?;
        ensure!(
            head_ref.status.success() || head_ref.status.code() == Some(1),
            "Cannot inspect HEAD reference"
        );
        let flags = git(&self.root, ["ls-files", "-v", "-z"])?;
        let mut tracked = state
            .files
            .into_iter()
            .filter(|file| file.status != "??")
            .collect::<Vec<_>>();
        // Status deliberately hides assume-unchanged and skip-worktree paths; include their
        // content in confirmation checks as reset --hard may overwrite hidden changes.
        for record in flags.split(|b| *b == 0).filter(|r| r.len() > 2) {
            if record[0].is_ascii_lowercase() || record[0] == b'S' {
                let path = path_from_bytes(&record[2..])?;
                if !tracked.iter().any(|f| f.path == path) {
                    tracked.push(FileEntry {
                        path,
                        original_path: None,
                        status: String::new(),
                        staged: false,
                        unstaged: false,
                        conflicted: false,
                    });
                }
            }
        }
        let files = tracked
            .iter()
            .filter(|file| !self.root.join(&file.path).is_dir())
            .map(|file| self.file_snapshot(file))
            .collect::<Result<Vec<_>>>()?;
        Ok(ResetSnapshot {
            head: self.text(&["rev-parse", "HEAD"])?.trim().into(),
            head_ref: head_ref.stdout,
            status: git(
                &self.root,
                ["status", "--porcelain=v1", "-z", "--untracked-files=all"],
            )?,
            index: git(&self.root, ["ls-files", "--stage", "-z"])?,
            flags,
            files,
        })
    }

    pub fn prepare_reset(&self, sha: &str) -> Result<ResetRequest> {
        self.verify_commit_id(sha)?;
        Ok(ResetRequest {
            target: sha.into(),
            snapshot: self.reset_snapshot()?,
        })
    }

    pub fn reset_commit(&self, request: &ResetRequest, mode: ResetMode) -> Result<String> {
        ensure!(
            self.reset_snapshot()? == request.snapshot,
            "Repository changed since reset preview; refresh and confirm again"
        );
        self.verify_commit_id(&request.target)?;
        if mode == ResetMode::Hard {
            let target = git(
                &self.root,
                ["ls-tree", "-r", "-z", "--name-only", &request.target],
            )?;
            // Include ignored files: reset --hard can remove untracked paths that obstruct its tree.
            let others = git(&self.root, ["ls-files", "--others", "-z"])?;
            let normalize = |path: PathBuf| {
                #[cfg(any(target_os = "macos", windows))]
                {
                    PathBuf::from(path.to_string_lossy().to_lowercase())
                }
                #[cfg(not(any(target_os = "macos", windows)))]
                {
                    path
                }
            };
            let targets = target
                .split(|b| *b == 0)
                .filter(|p| !p.is_empty())
                .map(|raw| path_from_bytes(raw).map(normalize))
                .collect::<Result<std::collections::HashSet<_>>>()?;
            let ancestors = targets
                .iter()
                .flat_map(|path| path.ancestors().skip(1).map(Path::to_owned))
                .collect::<std::collections::HashSet<_>>();
            for raw in others.split(|b| *b == 0).filter(|p| !p.is_empty()) {
                let other = path_from_bytes(raw)?;
                let comparable = normalize(other.clone());
                let overlap = ancestors.contains(&comparable)
                    || comparable.ancestors().any(|p| targets.contains(p));
                ensure!(
                    !overlap,
                    "Hard reset would remove untracked/ignored path {}; move it first",
                    display_path(&other)
                );
            }
        }
        ensure!(
            self.reset_snapshot()? == request.snapshot,
            "Repository changed during reset preparation; refresh and confirm again"
        );
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let recovery = format!("refs/git-wirdo/recovery/{stamp}-{}", std::process::id());
        let zero = "0".repeat(request.snapshot.head.len());
        git(
            &self.root,
            [
                "update-ref",
                "--create-reflog",
                "-m",
                "git-wirdo before reset",
                &recovery,
                &request.snapshot.head,
                &zero,
            ],
        )?;
        git(&self.root, ["reset", mode.flag(), &request.target, "--"])?;
        Ok(recovery)
    }

    /// Apply or undo exactly one immutable commit, preserving Git's conflict state.
    pub fn apply_commit(&self, sha: &str, revert: bool, mainline: Option<usize>) -> Result<()> {
        self.ensure_clean()?;
        ensure!(
            matches!(sha.len(), 40 | 64) && sha.bytes().all(|b| b.is_ascii_hexdigit()),
            "Select a full commit ID"
        );
        let target = format!("{sha}^{{commit}}");
        let resolved = self.text(&["rev-parse", "--verify", "--end-of-options", &target])?;
        ensure!(resolved.trim() == sha, "Selected commit changed");
        let parents = self.text(&["rev-list", "--parents", "-n", "1", sha])?;
        let count = parents.split_whitespace().count().saturating_sub(1);
        if count > 1 {
            ensure!(
                mainline.is_some_and(|n| n > 0 && n <= count),
                "Merge commit requires a mainline parent from 1 to {count}"
            );
        } else {
            ensure!(
                mainline.is_none(),
                "Mainline is only valid for merge commits"
            );
        }
        let mut args = vec![
            if revert {
                "revert".to_owned()
            } else {
                "cherry-pick".to_owned()
            },
            "--no-edit".into(),
        ];
        if let Some(n) = mainline {
            args.extend(["--mainline".into(), n.to_string()]);
        }
        args.extend(["--".into(), sha.to_owned()]);
        git(&self.root, args)?;
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
        } else if state.cherry_pick_in_progress {
            git(&self.root, ["cherry-pick", "--continue"])?;
        } else if state.revert_in_progress {
            git(&self.root, ["revert", "--continue"])?;
        } else if state.merge_in_progress {
            git(&self.root, ["commit", "--no-edit"])?;
        } else {
            bail!("No Git operation in progress");
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
        } else if state.cherry_pick_in_progress {
            git(&self.root, ["cherry-pick", "--abort"])?;
        } else if state.revert_in_progress {
            git(&self.root, ["revert", "--abort"])?;
        } else if state.merge_in_progress {
            git(&self.root, ["merge", "--abort"])?;
        } else {
            bail!("No Git operation in progress");
        }
        Ok(())
    }

    pub fn fetch_remote(&self, name: &str) -> Result<()> {
        self.verify_remote(name)?;
        git(&self.root, ["fetch", "--", name])?;
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
        let args: Vec<OsString> = std::iter::once(OsString::from("--literal-pathspecs"))
            .chain(args.iter().map(OsString::from))
            .chain(std::iter::once(OsString::from("--")))
            .chain(paths.iter().map(|path| path.as_os_str().to_owned()))
            .collect();
        git(&self.root, args)
    }

    fn path_text(&self, args: &[&str], paths: &[&Path]) -> Result<String> {
        Ok(String::from_utf8_lossy(&self.run_paths(args, paths)?).into_owned())
    }
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

fn git_output<I, S>(path: &Path, args: I) -> Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    git_command(path, args)
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
