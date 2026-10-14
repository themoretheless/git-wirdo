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
    pub(crate) fn same_selected_object(&self, other: &Self) -> bool {
        self.repository.root() == other.repository.root()
            && self.view == other.view
            && self
                .selection_key(self.view, self.selection())
                .is_some_and(|key| {
                    other.selection_key(other.view, other.selection()).as_ref() == Some(&key)
                })
    }

    pub(super) fn selection_key(&self, view: ViewMode, index: usize) -> Option<SelectionKey> {
        use SelectionKey::{Hunk, Path, Pr, Text};
        Some(match view {
            ViewMode::Files => Path(self.displayed_files().get(index)?.path.clone()),
            ViewMode::History => Text(self.state.commits.get(index)?.sha.clone()),
            ViewMode::Branches => Text(self.state.branches.get(index)?.name.clone()),
            ViewMode::Conflicts => Path(self.state.merge_state.conflicts.get(index)?.clone()),
            ViewMode::Workspaces => Path(self.workspaces.items.get(index)?.path.clone()),
            ViewMode::PullRequests => Pr(self.pull_requests.items.get(index)?.number),
            ViewMode::GitHubRepositories => Text(
                self.github_repositories
                    .items
                    .get(index)?
                    .name_with_owner
                    .clone(),
            ),
            ViewMode::Stashes => Text(self.stashes.items.get(index)?.sha.clone()),
            ViewMode::Remotes => Text(self.remotes.items.get(index)?.name.clone()),
            ViewMode::RemoteBranches => Text(self.remote_branches.items.get(index)?.clone()),
            ViewMode::Tags => Text(self.tags.items.get(index)?.name.clone()),
            ViewMode::Reflog => Text(self.reflog.items.get(index)?.sha.clone()),
            ViewMode::PrFiles => Text(self.pr_files.items.get(index)?.filename.clone()),
            ViewMode::Hunks => {
                let hunk = self.hunks.items.get(index)?;
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
            _ if view == self.view => self.cursor().1,
            _ => return,
        };
        if let Some(index) = (0..len).find(|&i| self.selection_key(view, i).as_ref() == Some(key)) {
            match view {
                ViewMode::Files => self.file_selection = index,
                ViewMode::History => self.history_selection = index,
                ViewMode::Branches => self.branch_selection = index,
                _ => {
                    *self.cursor().0 = index;
                }
            }
        }
    }
}
