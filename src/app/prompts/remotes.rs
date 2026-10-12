//! Remotes, upstream tracking and network transfers that need input.
use anyhow::{Result, ensure};

use super::Submitted;
use crate::app::App;

#[derive(Debug, Clone)]
pub enum RemotePrompt {
    Add,
    EditFetchUrl(String),
    EditPushUrl(String),
    Remove(String),
    /// Create a local branch tracking this remote branch.
    CheckoutTracking(String),
    SetUpstream,
    PublishBranch,
    PullStrategy,
}

impl RemotePrompt {
    pub(super) fn submit(self, app: &mut App, v: &[String]) -> Result<Submitted> {
        Ok(match self {
            Self::Add => {
                app.repository.add_remote(&v[0], &v[1])?;
                "Added remote".into()
            }
            Self::EditFetchUrl(name) => {
                app.repository.edit_remote(&name, &v[0])?;
                "Updated fetch URL".into()
            }
            Self::EditPushUrl(name) => {
                app.repository.edit_push_url(&name, &v[0])?;
                "Updated push URL".into()
            }
            Self::Remove(name) => {
                ensure!(v[0] == "remove", "Remote removal cancelled");
                app.repository.remove_remote(&name)?;
                "Removed remote".into()
            }
            Self::CheckoutTracking(reference) => {
                app.repository.checkout_remote(&reference, &v[0])?;
                "Checked out tracking branch".into()
            }
            Self::SetUpstream => {
                app.repository.set_upstream(&v[0])?;
                "Updated upstream".into()
            }
            Self::PublishBranch => {
                app.refresh_after(|app| app.repository.publish_branch(&v[0]))?;
                "Published current branch and set upstream".into()
            }
            Self::PullStrategy => {
                app.refresh_after(|app| app.repository.pull_strategy(&v[0]))?;
                "Pulled changes".into()
            }
        })
    }
}
