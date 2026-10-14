//! Pull request filtering, editing, review and creation through the GitHub CLI.
use anyhow::{Context, Result, ensure};

use super::Submitted;
use crate::app::App;
use crate::github::{self, EditablePr, MergeMethod, PrFilter, PullRequest};
use crate::pr_review::{self, PrFile};

#[derive(Debug, Clone)]
pub enum GitHubPrompt {
    Filter,
    Edit(EditablePr),
    /// Toggle between draft and ready for review.
    Ready(PullRequest),
    InlineComment(PullRequest, PrFile),
    Create,
    Merge(PullRequest),
    /// Submit a review with the given `gh pr review` verdict flag.
    Review(PullRequest, &'static str),
    Comment(u64),
    /// Create a repository on GitHub, optionally linking it as a remote without pushing.
    CreateRepository,
}

impl GitHubPrompt {
    pub(super) fn submit(self, app: &mut App, v: &[String]) -> Result<Submitted> {
        let root = app.repository.root().to_owned();
        Ok(match self {
            Self::Filter => {
                let filter = PrFilter {
                    state: v[0].trim().into(),
                    search: v[1].clone(),
                    limit: 100,
                };
                filter.validate()?;
                let prs = github::list_filtered(&root, &filter)?;
                app.pr_filter = filter;
                app.pull_requests.items = prs;
                app.pull_requests.selection = 0;
                "Updated PR filter".into()
            }
            Self::Edit(expected) => github::edit(&root, &expected, &v[0], &v[1], &v[2])?.into(),
            Self::Ready(pr) => {
                ensure!(
                    v[0] == pr.number.to_string(),
                    "Draft transition cancelled: confirm PR number"
                );
                github::set_ready(&root, &pr)?.into()
            }
            Self::InlineComment(pr, file) => pr_review::inline_comment(
                &root,
                &pr,
                &file,
                v[0].trim(),
                v[1].trim()
                    .parse()
                    .context("Enter a positive diff line number")?,
                &v[2],
            )?
            .into(),
            Self::Create => {
                app.repository.ensure_clean()?;
                github::create(&root, &v[0], &v[1], &v[2], &v[3])?.into()
            }
            Self::Merge(pr) => {
                ensure!(
                    v[0] == pr.number.to_string(),
                    "Merge cancelled: enter the selected PR number"
                );
                github::merge_with_method(&root, &pr, MergeMethod::parse(v[1].trim())?)?.into()
            }
            Self::Review(pr, verdict) => {
                github::review_selected(&root, &pr, verdict, &v[0])?.into()
            }
            Self::Comment(number) => github::comment(&root, number, &v[0])?.into(),
            Self::CreateRepository => {
                ensure!(v[3] == "create", "Repository creation cancelled");
                github::create_repository(&root, &v[0], v[1].trim(), v[2].trim())?.into()
            }
        })
    }
}
