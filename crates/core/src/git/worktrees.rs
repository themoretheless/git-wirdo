//! Linked worktrees: listing, creation from HEAD and guarded removal.
use super::*;

/// Identity reviewed before confirming removal; never trusts a path alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeRemoval {
    workspace: crate::model::Workspace,
    git_dir: PathBuf,
    head: Vec<u8>,
    head_ref: Vec<u8>,
}

impl WorktreeRemoval {
    pub fn is_detached(&self) -> bool {
        std::str::from_utf8(&self.head_ref).is_ok_and(|name| name.trim() == "HEAD")
    }
}

impl Repository {
    pub fn workspaces(&self) -> Result<Vec<crate::model::Workspace>> {
        let bytes = self.git(["worktree", "list", "--porcelain", "-z"])?;
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
        self.git(["check-ref-format", "--branch", branch])?;
        let path = self.resolve(path);
        self.git([
            OsString::from("worktree"),
            OsString::from("add"),
            OsString::from("-b"),
            OsString::from(branch),
            OsString::from("--"),
            path.into_os_string(),
            OsString::from("HEAD"),
        ])?;
        Ok(())
    }

    /// Create a new branch at a resolved commit, keeping the original HEAD unchanged.
    pub fn create_worktree_from(&self, path: &Path, branch: &str, base: &str) -> Result<()> {
        self.git(["check-ref-format", "--branch", branch])?;
        let commit = self.text(&[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{base}^{{commit}}"),
        ])?;
        self.git([
            OsString::from("worktree"),
            OsString::from("add"),
            OsString::from("-b"),
            OsString::from(branch),
            OsString::from("--"),
            self.resolve(path).into_os_string(),
            OsString::from(commit.trim()),
        ])?;
        Ok(())
    }

    /// Git rejects a branch already checked out elsewhere; never overrides that guard.
    pub fn create_worktree_existing(&self, path: &Path, branch: &str) -> Result<()> {
        self.git(["check-ref-format", "--branch", branch])?;
        self.git(["show-ref", "--verify", &format!("refs/heads/{branch}")])?;
        self.git([
            OsString::from("worktree"),
            OsString::from("add"),
            OsString::from("--"),
            self.resolve(path).into_os_string(),
            OsString::from(branch),
        ])?;
        Ok(())
    }

    fn verify_workspace(&self, workspace: &crate::model::Workspace) -> Result<()> {
        ensure!(
            self.workspaces()?.iter().any(|entry| entry == workspace),
            "Worktree changed; refresh before continuing"
        );
        Ok(())
    }

    pub fn set_worktree_lock(
        &self,
        workspace: &crate::model::Workspace,
        locked: bool,
        reason: &str,
    ) -> Result<()> {
        self.verify_workspace(workspace)?;
        ensure!(!workspace.bare, "Bare repository cannot be locked");
        ensure!(
            workspace.locked != locked,
            "Worktree lock state already changed; refresh"
        );
        let mut args = vec![OsString::from("worktree")];
        if locked {
            args.extend([
                OsString::from("lock"),
                OsString::from("--reason"),
                OsString::from(reason),
            ]);
        } else {
            args.push(OsString::from("unlock"));
        }
        args.extend([OsString::from("--"), workspace.path.as_os_str().to_owned()]);
        self.git(args)?;
        Ok(())
    }

    /// Move a linked checkout with its files intact. Active and primary checkouts stay put.
    pub fn move_worktree(&self, request: &WorktreeRemoval, destination: &Path) -> Result<()> {
        ensure!(
            self.prepare_worktree_removal(&request.workspace)? == *request,
            "Worktree HEAD or identity changed; refresh and confirm the move again"
        );
        self.git([
            OsString::from("worktree"),
            OsString::from("move"),
            OsString::from("--"),
            request.workspace.path.as_os_str().to_owned(),
            self.resolve(destination).into_os_string(),
        ])?;
        Ok(())
    }

    pub fn prepare_worktree_removal(
        &self,
        workspace: &crate::model::Workspace,
    ) -> Result<WorktreeRemoval> {
        let current = self
            .workspaces()?
            .into_iter()
            .find(|entry| entry.path == workspace.path)
            .context("Worktree disappeared; refresh before removing")?;
        ensure!(
            current == *workspace,
            "Worktree changed; refresh before removing"
        );
        ensure!(
            !workspace.locked && !workspace.bare,
            "Locked or bare worktree cannot be removed"
        );
        ensure!(
            workspace.path != self.root,
            "Cannot remove the current worktree"
        );
        let target = Self::open_with(&workspace.path, self.runner.clone())?;
        ensure!(
            !target.merge_state(&[])?.in_progress(),
            "Finish or abort the Git operation in this worktree before moving or removing it"
        );
        Ok(WorktreeRemoval {
            workspace: workspace.clone(),
            git_dir: fs::canonicalize(&target.git_dir)?,
            head: target.git(["rev-parse", "--verify", "HEAD"])?,
            head_ref: target.git(["rev-parse", "--symbolic-full-name", "HEAD"])?,
        })
    }

    pub fn remove_prepared_worktree(&self, request: &WorktreeRemoval) -> Result<()> {
        ensure!(
            self.prepare_worktree_removal(&request.workspace)? == *request,
            "Worktree HEAD or identity changed; refresh and confirm removal again"
        );
        let target = Self::open_with(&request.workspace.path, self.runner.clone())?;
        ensure!(
            !target.merge_state(&[])?.in_progress(),
            "Finish or abort the Git operation in this worktree before removing it"
        );
        let flags = target.git(["ls-files", "-v", "-z"])?;
        ensure!(
            !flags.split(|byte| *byte == 0).any(|entry| {
                entry
                    .first()
                    .is_some_and(|flag| flag.is_ascii_lowercase() || *flag == b'S')
            }),
            "Worktree has assume-unchanged or skip-worktree entries; inspect and clear them before removing"
        );
        ensure!(
            target
                .git([
                    "status",
                    "--porcelain=v1",
                    "-z",
                    "--untracked-files=all",
                    "--ignored",
                    "--ignore-submodules=none",
                ])?
                .is_empty(),
            "Worktree contains modified, untracked or ignored files; save or move them before removing"
        );
        if request.is_detached() {
            let head = utf8_line(&request.head)?;
            self.verify_commit_id(&head)?;
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos();
            let recovery = format!(
                "refs/git-wirdo/recovery/worktree-{stamp}-{}",
                std::process::id()
            );
            // A detached checkout has no branch retaining its commits. Save its tip
            // before deletion; failure must leave the checkout intact. Native GC and
            // reflog expiration cannot discard a commit reachable through this ref.
            self.git([
                "update-ref",
                "--create-reflog",
                &recovery,
                &head,
                &"0".repeat(head.len()),
            ])?;
        }
        // Git refuses the primary checkout, dirty worktrees and locked worktrees. Never force.
        self.git([
            OsString::from("worktree"),
            OsString::from("remove"),
            OsString::from("--"),
            request.workspace.path.as_os_str().to_owned(),
        ])?;
        Ok(())
    }

    pub fn remove_workspace(&self, workspace: &crate::model::Workspace) -> Result<()> {
        self.remove_prepared_worktree(&self.prepare_worktree_removal(workspace)?)
    }
}
