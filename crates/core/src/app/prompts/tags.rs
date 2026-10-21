//! Creating, deleting and publishing tags.
use anyhow::{Result, ensure};

use super::Submitted;
use crate::app::App;
use crate::model::TagEntry;

#[derive(Debug, Clone)]
pub enum TagPrompt {
    Create,
    Delete(TagEntry),
    Publish(TagEntry),
    DeleteRemote(TagEntry),
}

impl TagPrompt {
    pub(super) fn submit(self, app: &mut App, v: &[String]) -> Result<Submitted> {
        Ok(match self {
            Self::Create => {
                app.repository.create_tag(&v[0], &v[1], &v[2])?;
                "Created tag".into()
            }
            Self::Delete(tag) => {
                ensure!(v[0] == "delete", "Tag deletion cancelled");
                app.repository.delete_tag(&tag)?;
                "Deleted selected local tag".into()
            }
            Self::Publish(tag) => {
                app.repository.publish_tag(&tag, &v[0])?;
                "Published selected tag".into()
            }
            Self::DeleteRemote(tag) => {
                ensure!(v[1] == "delete", "Remote tag deletion cancelled");
                app.repository.delete_remote_tag(&tag, &v[0])?;
                "Deleted remote tag; local tag retained".into()
            }
        })
    }
}
