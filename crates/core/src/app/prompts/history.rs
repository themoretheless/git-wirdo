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
    ConfirmInteractive(crate::git::InteractiveRebase, String),
    Reword(String),
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
            Self::Reword(sha) => {
                let editor = app
                    .rebase_editor
                    .as_mut()
                    .context("Rebase editor is closed")?;
                editor.plan.reword(&sha, &v[0])?;
                let row = editor
                    .rows
                    .iter_mut()
                    .find(|row| row.split_whitespace().nth(1) == Some(sha.as_str()))
                    .context("Commit no longer present")?;
                *row = format!(
                    "reword {sha} # {}",
                    v[0].lines()
                        .next()
                        .unwrap_or("")
                        .escape_debug()
                        .to_string()
                        .replace(';', "\\u{3b}")
                );
                return Ok(Submitted::Done);
            }
            Self::ConfirmInteractive(plan, todo) => {
                ensure!(v[0] == "rebase", "Rebase cancelled");
                app.rebase_editor = None;
                let result =
                    app.refresh_after(|app| app.repository.run_interactive_rebase(&plan, &todo));
                if app.state.merge_state.in_progress() {
                    app.view = ViewMode::Conflicts;
                    app.refresh_detail();
                }
                result?;
                if app.state.merge_state.rebase_in_progress {
                    "Interactive rebase paused; amend the commit, then continue or abort".into()
                } else {
                    "Interactive rebase completed; original HEAD retained in recovery refs".into()
                }
            }
            Self::Interactive(plan) => {
                ensure!(v[1] == "rebase", "Rebase cancelled");
                let result =
                    app.refresh_after(|app| app.repository.run_interactive_rebase(&plan, &v[0]));
                if app.state.merge_state.in_progress() {
                    app.view = ViewMode::Conflicts;
                    app.refresh_detail();
                }
                result?;
                if app.state.merge_state.rebase_in_progress {
                    "Interactive rebase paused; amend the commit, then continue or abort".into()
                } else {
                    "Interactive rebase completed; original HEAD retained in recovery refs".into()
                }
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
