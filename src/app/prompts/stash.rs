//! Saving, applying and dropping stashes.
use anyhow::{Result, ensure};

use super::Submitted;
use crate::app::App;
use crate::model::StashEntry;

#[derive(Debug, Clone)]
pub enum StashPrompt {
    Save,
    SaveSelected(crate::git::StashFilesRequest),
    /// Apply the stash; `true` also removes it on success.
    Apply(StashEntry, bool),
    Drop(StashEntry),
}

impl StashPrompt {
    pub(super) fn submit(self, app: &mut App, v: &[String]) -> Result<Submitted> {
        Ok(match self {
            Self::SaveSelected(request) => {
                ensure!(v[1] == "stash", "Selective stash cancelled");
                app.refresh_after(|app| app.repository.save_stash_files(&request, &v[0]))?;
                app.stash_files.clear();
                "Saved selected files in stash".into()
            }
            Self::Save => {
                app.refresh_after(|app| app.repository.save_stash(&v[0]))?;
                "Saved stash including untracked files".into()
            }
            Self::Apply(stash, pop) => {
                ensure!(
                    v[0] == if pop { "pop" } else { "apply" },
                    "Stash operation cancelled"
                );
                app.refresh_after(|app| app.repository.apply_stash(&stash, pop))?;
                if pop {
                    "Applied and removed stash".into()
                } else {
                    "Applied stash; entry retained".into()
                }
            }
            Self::Drop(stash) => {
                ensure!(v[0] == "drop", "Drop cancelled: type drop exactly");
                app.repository.drop_stash(&stash)?;
                "Dropped selected stash".into()
            }
        })
    }
}
