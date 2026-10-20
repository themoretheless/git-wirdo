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
