//! Commits and file restores in the current working tree.
use anyhow::{Result, ensure};

use super::Submitted;
use crate::app::App;
use crate::git::RestoreRequest;

#[derive(Debug, Clone)]
pub enum WorkingTreePrompt {
    /// Commit staged changes; `true` amends the latest commit.
    Commit(bool),
    Lines(crate::patch::Hunk),
    Restore(RestoreRequest),
}

impl WorkingTreePrompt {
    pub(super) fn submit(self, app: &mut App, v: &[String]) -> Result<Submitted> {
        Ok(match self {
            Self::Lines(hunk) => {
                let selected = v[0]
                    .split_whitespace()
                    .map(str::parse)
                    .collect::<std::result::Result<Vec<usize>, _>>()?;
                let partial = hunk.select_lines(&selected)?;
                app.refresh_after(|app| app.repository.apply_hunk(&partial))?;
                "Updated selected lines in index".into()
            }
            Self::Commit(amend) => {
                // Reload after hooks even when they fail, preserving partial state changes.
                app.refresh_after(|app| {
                    if amend {
                        app.repository.amend(&v[0])
                    } else {
                        app.repository.commit(&v[0])
                    }
                })?;
                "Committed changes".into()
            }
            Self::Restore(request) => {
                ensure!(v[0] == "discard", "Discard cancelled");
                app.refresh_after(|app| app.repository.restore_file(&request))?;
                "Restored selected file".into()
            }
        })
    }
}
