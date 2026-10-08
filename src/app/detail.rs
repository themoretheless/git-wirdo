//! Text for the detail pane of the selected entry in the current view.
use anyhow::Result;

use super::{App, FileDetail};
use crate::model::{ViewMode, display_path};

impl App {
    pub(super) fn selected_detail(&self) -> Result<String> {
        match self.view {
            ViewMode::RecentRepositories => match self.navigation.repositories.get(self.recent_selection) {
                Some(settings) => Ok(format!("{}\nSaved view: {:?}\nEnter opens this working tree; O opens another path.\nD forgets this entry without deleting any files.", display_path(&settings.root.path()?), settings.view)),
                None => Ok("No recent repositories; O opens a local repository.".into()),
            },
            ViewMode::PrFiles => match (self.review_pr.as_ref(), self.pr_files.get(self.pr_file_selection)) {
                (Some(pr), Some(file)) => file.detail(pr),
                _ => Ok("No PR file selected; use i in PullRequests to load reviewed files.".into()),
            },
            ViewMode::Reflog => match self.reflog.get(self.reflog_selection) {
                Some(entry) => self.repository.commit_detail(&crate::model::CommitEntry { sha: entry.sha.clone(), short_sha: String::new(), date: String::new(), author: String::new(), subject: entry.subject.clone() }),
                None => Ok("No reflog entry selected".into()),
            },
            ViewMode::Hunks => Ok(self.hunks.get(self.hunk_selection).map(|hunk| format!("{} hunk {}/{}\n{}\n\n{}", if hunk.staged { "Staged" } else { "Unstaged" }, self.hunk_selection + 1, self.hunks.len(), display_path(&hunk.file.path), String::from_utf8_lossy(&hunk.patch))).unwrap_or_else(|| "No text hunks. d switches staged/unstaged. Use full-file staging for untracked, binary, rename or mode changes.".into())),
            ViewMode::Tags => match self.tags.get(self.tag_selection) {
                Some(tag) => Ok(format!("{} ({})\n{}\n\n{}", tag.name, tag.kind, tag.sha, self.repository.tag_detail(tag)?)),
                None => Ok("No tags; N creates one.".into()),
            },
            ViewMode::Remotes => Ok(self.remotes.get(self.remote_selection).map(|r| format!("{}\nFetch URLs:\n{}\nPush URLs:\n{}", r.name, r.fetch_urls, r.push_urls)).unwrap_or_else(|| "No remotes. N adds one.".into())),
            ViewMode::RemoteBranches => Ok(self.remote_branches.get(self.remote_branch_selection).map(|b| format!("{b}\nEnter creates a tracking local branch; U sets current upstream.")).unwrap_or_else(|| "No remote branches; fetch first.".into())),
            ViewMode::Stashes => match self.stashes.get(self.stash_selection) {
                Some(stash) => Ok(format!("{} {}\n{}\n\n{}", stash.selector, stash.subject, stash.sha, self.repository.stash_detail(stash)?)),
                None => Ok("No stashes. S saves tracked and untracked changes.".into()),
            },
            ViewMode::GitHubRepositories => Ok(self.github_repositories.get(self.github_repo_selection).map(|r| format!("{}\n{}\nPrivate: {}\nEnter clone and open", r.name_with_owner, r.url, r.is_private)).unwrap_or_else(|| "No GitHub repositories".into())),
            ViewMode::Workspaces => Ok(self.workspaces.get(self.workspace_selection).map(|w| format!("{}\nBranch: {}\nLocked: {}\nEnter open | N create | D remove", display_path(&w.path), w.branch, w.locked)).unwrap_or_else(|| "No workspace".into())),
            ViewMode::PullRequests => match self.pull_requests.get(self.pr_selection) {
                Some(pr) => Ok(format!("{}\n{}", crate::github::detail(self.repository.root(), pr.number)?, crate::pr_review::comments(self.repository.root(), pr.number)?)),
                None => Ok("No open pull requests. N creates a draft PR; authenticate with gh auth login outside the TUI.".into()),
            },
            ViewMode::Files => match self.displayed_files().get(self.file_selection) {
                Some(file) if self.file_detail != FileDetail::Diff => {
                    let path = file.original_path.as_deref().unwrap_or(&file.path);
                    let (label, text) = if self.file_detail == FileDetail::History {
                        ("File history at HEAD (up to 100 commits; follows renames)", self.repository.file_history(path, 100)?)
                    } else {
                        ("Line authorship at HEAD (pending edits excluded)", self.repository.file_blame(path)?)
                    };
                    Ok(format!("{label}: {}\n\n{text}", display_path(path)))
                }
                Some(file) if self.browse_tracked => self.repository.file_contents(&file.path),
                Some(file) if self.upstream_comparison => self.repository.upstream_detail(file),
                Some(file) => self.repository.file_detail(file),
                None => Ok("No files selected".to_owned()),
            },
            ViewMode::History if self.graph_visible => {
                let selected = self.state.commits.get(self.history_selection).map(|c| c.sha.as_str());
                Ok(self.history_graph.lines().map(|line| format!("{}{}", if selected.is_some_and(|sha| line.contains(sha)) { "> " } else { "  " }, line)).collect::<Vec<_>>().join("\n"))
            }
            ViewMode::History => match self.state.commits.get(self.history_selection) {
                Some(commit) => self.repository.commit_detail(commit),
                None => Ok("No commit selected".to_owned()),
            },
            ViewMode::Branches => Ok(match self.state.branches.get(self.branch_selection) {
                Some(branch) if branch.current => format!("Current branch: {}", branch.name),
                Some(branch) => format!(
                    "Branch: {}\n\nPress Enter to switch to this branch.",
                    branch.name
                ),
                None => "No branch selected".to_owned(),
            }),
            ViewMode::Conflicts => {
                let header = self.state.merge_state.summary();
                let help =
                    "Keys: o ours | t theirs | a mark resolved | e continue | K skip | x abort";
                match self
                    .state
                    .merge_state
                    .conflicts
                    .get(self.conflict_selection)
                {
                    Some(path) => Ok(format!(
                        "{header}\n\nConflict: {}\n\n{}\n\n{help}",
                        display_path(path),
                        self.repository.conflict_detail(path)?
                    )),
                    None => Ok(format!(
                        "{header}\n\nNo conflicted file selected.\n\n{help}\n\n{}",
                        if self.state.merge_state.am_in_progress { self.repository.current_mail_patch()? } else { String::new() }
                    )),
                }
            }
        }
    }
}
