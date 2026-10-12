//! Linked worktrees: listing, creation from HEAD and guarded removal.
use super::*;

impl Repository {
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
        let path = self.resolve(path);
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
}
