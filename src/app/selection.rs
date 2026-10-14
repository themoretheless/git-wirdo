//! Stable selection identities across list refreshes. Paths retain native bytes.
use super::App;
use crate::model::ViewMode;
use std::path::PathBuf;
#[derive(PartialEq, Eq)]
pub(super) enum SelectionKey {
    Path(PathBuf),
    Text(String),
    Pr(u64),
    Hunk(PathBuf, Vec<u8>),
}
impl App {
    pub(super) fn selection_key(&self, view: ViewMode, index: usize) -> Option<SelectionKey> {
        use SelectionKey::{Hunk, Path, Pr, Text};
        Some(match view {
            ViewMode::Files => Path(self.displayed_files().get(index)?.path.clone()),
            ViewMode::History => Text(self.state.commits.get(index)?.sha.clone()),
            ViewMode::Branches => Text(self.state.branches.get(index)?.name.clone()),
            ViewMode::Conflicts => Path(self.state.merge_state.conflicts.get(index)?.clone()),
            ViewMode::Workspaces => Path(self.workspaces.get(index)?.path.clone()),
            ViewMode::PullRequests => Pr(self.pull_requests.get(index)?.number),
            ViewMode::GitHubRepositories => {
                Text(self.github_repositories.get(index)?.name_with_owner.clone())
            }
            ViewMode::Stashes => Text(self.stashes.get(index)?.sha.clone()),
            ViewMode::Remotes => Text(self.remotes.get(index)?.name.clone()),
            ViewMode::RemoteBranches => Text(self.remote_branches.get(index)?.clone()),
            ViewMode::Tags => Text(self.tags.get(index)?.name.clone()),
            ViewMode::Reflog => Text(self.reflog.get(index)?.sha.clone()),
            ViewMode::PrFiles => Text(self.pr_files.get(index)?.filename.clone()),
            ViewMode::Hunks => {
                let hunk = self.hunks.get(index)?;
                Hunk(hunk.file.path.clone(), hunk.patch.clone())
            }
            ViewMode::RecentRepositories => {
                Path(self.navigation.repositories.get(index)?.root.path().ok()?)
            }
        })
    }
    pub(super) fn restore_selection(&mut self, view: ViewMode, key: Option<&SelectionKey>) {
        let Some(key) = key else {
            return;
        };
        // Core lists may be restored while another view is active.
        let len = match view {
            ViewMode::Files => self.displayed_files().len(),
            ViewMode::History => self.state.commits.len(),
            ViewMode::Branches => self.state.branches.len(),
            _ if view == self.view => self.list_len(),
            _ => return,
        };
        if let Some(index) = (0..len).find(|&i| self.selection_key(view, i).as_ref() == Some(key)) {
            *self.selection_mut_for(view) = index;
        }
    }
}
