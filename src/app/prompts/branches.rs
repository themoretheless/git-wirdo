//! Creating, switching, renaming, deleting and integrating local branches.
use anyhow::{Result, ensure};

use super::Submitted;
use crate::app::App;

#[derive(Debug, Clone)]
pub enum BranchPrompt {
    SwitchOrCreate,
    Rename(String),
    Delete(String),
    /// Merge the named branch, or rebase onto it when `true`.
    Integrate(String, bool),
}

impl BranchPrompt {
    pub(super) fn submit(self, app: &mut App, v: &[String]) -> Result<Submitted> {
        Ok(match self {
            Self::SwitchOrCreate => {
                app.repository.switch_or_create_branch(&v[0])?;
                "Switched branch".into()
            }
            Self::Rename(name) => {
                app.repository.rename_branch(&name, &v[0])?;
                "Renamed branch".into()
            }
            Self::Delete(name) => {
                ensure!(v[0] == "delete", "Deletion cancelled: type delete exactly");
                app.repository.delete_branch(&name)?;
                "Deleted merged branch".into()
            }
            Self::Integrate(name, rebase) => {
                ensure!(
                    v[0] == if rebase { "rebase" } else { "merge" },
                    "Operation cancelled: confirmation does not match"
                );
                app.refresh_after(|app| app.repository.integrate_branch(&name, rebase))?;
                "Branch integrated".into()
            }
        })
    }
}
