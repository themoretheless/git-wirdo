//! Opening, creating and cloning checkouts, and managing linked worktrees.
use std::path::Path;

use anyhow::{Result, ensure};

use super::Submitted;
use crate::app::App;
use crate::git::Repository;
use crate::model::Workspace;

#[derive(Debug, Clone)]
pub enum RepositoryPrompt {
    Open,
    Initialize,
    CloneGit,
    /// Clone the named GitHub repository.
    CloneGitHub(String),
    CreateWorktree,
    RemoveWorktree(Workspace),
}

impl RepositoryPrompt {
    pub(super) fn submit(self, app: &mut App, v: &[String]) -> Result<Submitted> {
        let opened = match self {
            Self::Open => app.repository.resolve(Path::new(&v[0])),
            Self::Initialize => {
                Repository::initialize(app.repository.root(), Path::new(&v[0]), &v[1])?
                    .root()
                    .to_owned()
            }
            Self::CloneGit => Repository::clone_into(
                app.repository.root(),
                std::ffi::OsStr::new(&v[0]),
                Path::new(&v[1]),
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
                return Ok("Workspace created".into());
            }
            Self::RemoveWorktree(workspace) => {
                ensure!(v[0] == "remove", "Removal cancelled: type remove exactly");
                app.repository.remove_workspace(&workspace)?;
                return Ok("Workspace removed; branch retained".into());
            }
        };
        app.open_repository(&opened)?;
        Ok(Submitted::Done)
    }
}
