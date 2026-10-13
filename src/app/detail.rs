//! Detail pane contents for the selected entry, and searching inside them.
use anyhow::Result;

use super::{App, FileDetail};
use crate::model::{ViewMode, display_path};

/// Text for an empty list, or the mapped selected entry.
fn describe<T>(entry: Option<&T>, empty: &str, describe: impl FnOnce(&T) -> String) -> String {
    entry.map(describe).unwrap_or_else(|| empty.to_owned())
}

impl App {
    pub(super) fn selected_detail(&self) -> Result<String> {
        let root = self.repository.root();
        Ok(match self.view {
            ViewMode::RecentRepositories => {
                match self.navigation.repositories.get(self.recent_selection) {
                    Some(settings) => format!(
                        "{}\nSaved view: {:?}\nEnter opens this working tree; O opens another path.\nD forgets this entry without deleting any files.",
                        display_path(&settings.root.path()?),
                        settings.view
                    ),
                    None => "No recent repositories; O opens a local repository.".into(),
                }
            }
            ViewMode::PrFiles => match (
                self.review_pr.as_ref(),
                self.pr_files.get(self.pr_file_selection),
            ) {
                (Some(pr), Some(file)) => file.detail(pr)?,
                _ => "No PR file selected; use i in PullRequests to load reviewed files.".into(),
            },
            ViewMode::Reflog => match self.reflog.get(self.reflog_selection) {
                Some(entry) => self.repository.commit_detail(&entry.sha)?,
                None => "No reflog entry selected".into(),
            },
            ViewMode::Hunks => describe(
                self.hunks.get(self.hunk_selection),
                "No text hunks. d switches staged/unstaged. Use full-file staging for untracked, binary, rename or mode changes.",
                |hunk| {
                    format!(
                        "{} hunk {}/{}\n{}\n\n{}",
                        if hunk.staged { "Staged" } else { "Unstaged" },
                        self.hunk_selection + 1,
                        self.hunks.len(),
                        display_path(&hunk.file.path),
                        String::from_utf8_lossy(&hunk.patch)
                    )
                },
            ),
            ViewMode::Tags => match self.tags.get(self.tag_selection) {
                Some(tag) => format!(
                    "{} ({})\n{}\n\n{}",
                    tag.name,
                    tag.kind,
                    tag.sha,
                    self.repository.tag_detail(tag)?
                ),
                None => "No tags; N creates one.".into(),
            },
            ViewMode::Remotes => describe(
                self.remotes.get(self.remote_selection),
                "No remotes. N adds one.",
                |r| {
                    format!(
                        "{}\nFetch URLs:\n{}\nPush URLs:\n{}",
                        r.name, r.fetch_urls, r.push_urls
                    )
                },
            ),
            ViewMode::RemoteBranches => describe(
                self.remote_branches.get(self.remote_branch_selection),
                "No remote branches; fetch first.",
                |b| format!("{b}\nEnter creates a tracking local branch; U sets current upstream."),
            ),
            ViewMode::Stashes => match self.stashes.get(self.stash_selection) {
                Some(stash) => format!(
                    "{} {}\n{}\n\n{}",
                    stash.selector,
                    stash.subject,
                    stash.sha,
                    self.repository.stash_detail(stash)?
                ),
                None => "No stashes. S saves tracked and untracked changes.".into(),
            },
            ViewMode::GitHubRepositories => describe(
                self.github_repositories.get(self.github_repo_selection),
                "No GitHub repositories. N creates one for this checkout.",
                |r| {
                    format!(
                        "{}\n{}\nPrivate: {}\nEnter clone and open | N create repository",
                        r.name_with_owner, r.url, r.is_private
                    )
                },
            ),
            ViewMode::Workspaces => describe(
                self.workspaces.get(self.workspace_selection),
                "No workspace",
                |w| {
                    format!(
                        "{}\nBranch: {}\nLocked: {}\nEnter open | N create | D remove",
                        display_path(&w.path),
                        w.branch,
                        w.locked
                    )
                },
            ),
            ViewMode::PullRequests => match self.pull_requests.get(self.pr_selection) {
                Some(pr) => format!(
                    "{}\n{}",
                    crate::github::detail(root, pr.number)?,
                    crate::pr_review::comments(root, pr.number)?
                ),
                None => "No open pull requests. N creates a draft PR; authenticate with gh auth login outside the TUI.".into(),
            },
            ViewMode::Files => match self.displayed_files().get(self.file_selection) {
                Some(file) if self.file_detail != FileDetail::Diff => {
                    let path = file.original_path.as_deref().unwrap_or(&file.path);
                    let (label, text) = if self.file_detail == FileDetail::History {
                        (
                            "File history at HEAD (up to 100 commits; follows renames)",
                            self.repository.file_history(path, 100)?,
                        )
                    } else {
                        (
                            "Line authorship at HEAD (pending edits excluded)",
                            self.repository.file_blame(path)?,
                        )
                    };
                    format!("{label}: {}\n\n{text}", display_path(path))
                }
                Some(file) if self.browse_tracked => self.repository.file_contents(&file.path)?,
                Some(file) if self.upstream_comparison => self.repository.upstream_detail(file)?,
                Some(file) => self.repository.file_detail(file)?,
                None => "No files selected".to_owned(),
            },
            ViewMode::History if self.graph_visible => {
                let selected = self
                    .state
                    .commits
                    .get(self.history_selection)
                    .map(|c| c.sha.as_str());
                self.history_graph
                    .lines()
                    .map(|line| {
                        let marker = if selected.is_some_and(|sha| line.contains(sha)) {
                            "> "
                        } else {
                            "  "
                        };
                        format!("{marker}{line}")
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            ViewMode::History => match self.state.commits.get(self.history_selection) {
                Some(commit) => self.repository.commit_detail(&commit.sha)?,
                None => "No commit selected".to_owned(),
            },
            ViewMode::Branches => match self.state.branches.get(self.branch_selection) {
                Some(branch) if branch.current => format!("Current branch: {}", branch.name),
                Some(branch) => format!(
                    "Branch: {}\n\nPress Enter to switch to this branch.",
                    branch.name
                ),
                None => "No branch selected".to_owned(),
            },
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
                    Some(path) => format!(
                        "{header}\n\nConflict: {}\n\n{}\n\n{help}",
                        display_path(path),
                        self.repository.conflict_detail(path)?
                    ),
                    None => format!(
                        "{header}\n\nNo conflicted file selected.\n\n{help}\n\n{}",
                        if self.state.merge_state.am_in_progress {
                            self.repository.current_mail_patch()?
                        } else {
                            String::new()
                        }
                    ),
                }
            }
        })
    }

    /// Find the next occurrence after `start`, wrapping to the beginning.
    fn wrapping_find(len: usize, start: usize, matches: impl Fn(usize) -> bool) -> Option<usize> {
        (start..len).chain(0..start.min(len)).find(|i| matches(*i))
    }

    pub(super) fn find_match(&mut self, next: bool) {
        if self.search.is_empty() {
            return;
        }
        if self.view == ViewMode::History {
            let start = if next {
                self.history_selection.saturating_add(1)
            } else {
                0
            };
            let found = Self::wrapping_find(self.state.commits.len(), start, |index| {
                let c = &self.state.commits[index];
                [&c.sha, &c.subject, &c.author, &c.date]
                    .iter()
                    .any(|value| value.contains(&self.search))
            });
            if let Some(index) = found {
                self.history_selection = index;
                self.refresh_detail();
            }
            self.message = if found.is_some() {
                format!("History match: {}", self.search)
            } else {
                format!(
                    "No loaded history match: {} (use + to load older commits)",
                    self.search
                )
            };
            return;
        }
        let start = if next {
            usize::from(self.detail_scroll) + 1
        } else {
            0
        };
        let lines: Vec<_> = self.detail_text.lines().collect();
        let found = Self::wrapping_find(lines.len(), start, |index| {
            lines[index].contains(&self.search)
        });
        self.detail_column = 0;
        if let Some(line) = found {
            self.detail_scroll = line.min(u16::MAX as usize) as u16;
        }
        self.message = if found.is_some() {
            format!("Find: {}", self.search)
        } else {
            format!("No match: {}", self.search)
        };
    }
}
