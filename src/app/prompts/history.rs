//! Mail patches and operations on existing commits.
use std::path::Path;

use anyhow::{Context, Result, ensure};

use super::Submitted;
use crate::app::App;
use crate::git::{ResetMode, ResetRequest};
use crate::model::{ViewMode, display_path};

#[derive(Debug, Clone)]
pub enum HistoryPrompt {
    Interactive(crate::git::InteractiveRebase),
    ImportPatch,
    ExportCommit(String),
    /// Create a branch at a reflog commit.
    Recover(String),
    Reset(ResetRequest),
    /// Cherry-pick, or revert when `true`.
    Apply(String, bool),
}

impl HistoryPrompt {
    pub(super) fn submit(self, app: &mut App, v: &[String]) -> Result<Submitted> {
        Ok(match self {
            Self::Interactive(plan) => {
                ensure!(v[1] == "rebase", "Rebase cancelled");
                let result =
                    app.refresh_after(|app| app.repository.run_interactive_rebase(&plan, &v[0]));
                if app.state.merge_state.in_progress() {
                    app.view = ViewMode::Conflicts;
                    app.refresh_detail();
                }
                result?;
                "Interactive rebase completed; original HEAD retained in recovery refs".into()
            }
            Self::ImportPatch => {
                ensure!(v[1] == "apply", "Import cancelled: type apply exactly");
                let result =
                    app.refresh_after(|app| app.repository.import_mail_patch(Path::new(&v[0])));
                if app.state.merge_state.am_in_progress {
                    app.view = ViewMode::Conflicts;
                    app.refresh_detail();
                }
                result?;
                "Imported mail patch".into()
            }
            Self::ExportCommit(sha) => {
                let path = app.repository.export_commit(&sha, Path::new(&v[0]))?;
                format!("Exported mail patch {}", display_path(&path)).into()
            }
            Self::Recover(sha) => {
                app.repository.recover_commit(&sha, &v[0])?;
                format!("Recovered {} into branch {}; checkout unchanged", sha, v[0]).into()
            }
            Self::Reset(request) => {
                let mode = ResetMode::parse(v[0].trim())?;
                ensure!(
                    v[1] == request.target,
                    "Reset cancelled: confirm full target commit ID"
                );
                let recovery =
                    app.refresh_after(|app| app.repository.reset_commit(&request, mode))?;
                format!(
                    "Reset {} to {}; previous HEAD saved as {}",
                    v[0], request.target, recovery
                )
                .into()
            }
            Self::Apply(sha, revert) => {
                ensure!(
                    v[0] == sha,
                    "Commit operation cancelled: confirm full commit ID"
                );
                let mainline = if v[1].trim().is_empty() {
                    None
                } else {
                    Some(
                        v[1].trim()
                            .parse::<usize>()
                            .context("Mainline must be a parent number")?,
                    )
                };
                app.run_action(
                    |repo| repo.apply_commit(&sha, revert, mainline),
                    if revert {
                        "Reverted commit"
                    } else {
                        "Cherry-picked commit"
                    },
                )?;
                app.message.clone().into()
            }
        })
    }
}
