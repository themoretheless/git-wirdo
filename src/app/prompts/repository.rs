//! Opening, creating and cloning checkouts, and managing linked worktrees.
use std::path::Path;

use anyhow::{Result, ensure};

use super::Submitted;
use crate::app::App;
use crate::git::Repository;
use crate::git::WorktreeRemoval;

#[derive(Debug, Clone)]
pub enum RepositoryPrompt {
    Open,
    Initialize,
    CloneGit,
    /// Clone the named GitHub repository.
    CloneGitHub(String),
    CreateWorktree,
    CreateWorktreeFrom,
    CreateWorktreeExisting,
    LockWorktree(crate::model::Workspace),
    UnlockWorktree(crate::model::Workspace),
    MoveWorktree(WorktreeRemoval),
    RemoveWorktree(WorktreeRemoval),
}

impl RepositoryPrompt {
    pub(super) fn submit(self, app: &mut App, v: &[String]) -> Result<Submitted> {
        let opened = match self {
            Self::Open => app.repository.resolve(Path::new(&v[0])),
            Self::Initialize => Repository::initialize_with(
                app.repository.root(),
                Path::new(&v[0]),
                &v[1],
                app.repository.runner(),
            )?
            .root()
            .to_owned(),
            Self::CloneGit => Repository::clone_into_with(
                app.repository.root(),
                std::ffi::OsStr::new(&v[0]),
                Path::new(&v[1]),
                app.repository.runner(),
            )?
            .root()
            .to_owned(),
            Self::CloneGitHub(name) => {
                let path = app.repository.resolve(Path::new(&v[0]));
                crate::github::clone_repository(app.repository.root(), &name, &path)?;
                path
            }
            Self::CreateWorktree => {
                app.repository.create_workspace(Path::new(&v[0]), &v[1])?;
                return Ok("Worktree created".into());
            }
            Self::CreateWorktreeFrom => {
                app.repository
                    .create_worktree_from(Path::new(&v[0]), &v[1], &v[2])?;
                return Ok("Worktree created from selected base".into());
            }
            Self::CreateWorktreeExisting => {
                app.repository
                    .create_worktree_existing(Path::new(&v[0]), &v[1])?;
                return Ok("Worktree created for existing branch".into());
            }
            Self::LockWorktree(workspace) => {
                app.repository.set_worktree_lock(&workspace, true, &v[0])?;
                return Ok("Worktree locked".into());
            }
            Self::UnlockWorktree(workspace) => {
                ensure!(v[0] == "unlock", "Type unlock exactly");
                app.repository.set_worktree_lock(&workspace, false, "")?;
                return Ok("Worktree unlocked".into());
            }
            Self::MoveWorktree(request) => {
                app.repository.move_worktree(&request, Path::new(&v[0]))?;
                return Ok("Worktree moved; open its new path to continue".into());
            }
            Self::RemoveWorktree(workspace) => {
                ensure!(v[0] == "remove", "Removal cancelled: type remove exactly");
                app.repository.remove_prepared_worktree(&workspace)?;
                return Ok("Worktree removed; branch retained".into());
            }
        };
        app.open_repository(&opened)?;
        Ok(Submitted::Done)
    }
}
