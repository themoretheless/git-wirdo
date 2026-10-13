//! Action dispatch. The current view claims an action first; the rest are global.
use anyhow::Result;

use super::{Action, App, FileDetail, PromptKind};
use crate::git::{ConflictSide, Repository};
use crate::model::{ViewMode, display_path};

/// Widest line of the detail pane, for horizontal scrolling bounds.
fn widest_line(text: &str) -> u16 {
    text.lines()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0)
        .min(u16::MAX as usize) as u16
}

impl App {
    /// Whether `action` only opens a form or moves the viewport in the current view, so it can
    /// run on the UI thread. Actions that may run Git or GitHub belong to the background worker.
    pub fn runs_locally(&self, action: Action) -> bool {
        match (self.view, action) {
            // Editing a PR first downloads its current metadata.
            (ViewMode::PullRequests, Action::EditRemote) => false,
            // Discarding a file snapshots it before asking for confirmation.
            (ViewMode::Files, Action::Remove) => false,
            (_, Action::Remove) => true,
            _ => matches!(
                action,
                Action::CommandPalette
                    | Action::ImportPatch
                    | Action::ExportCommit
                    | Action::Commit
                    | Action::AmendCommit
                    | Action::Branch
                    | Action::New
                    | Action::Search
                    | Action::OpenRepository
                    | Action::SaveStash
                    | Action::SetUpstream
                    | Action::PublishBranch
                    | Action::PullStrategy
                    | Action::RenameBranch
                    | Action::MergeBranch
                    | Action::RebaseBranch
                    | Action::MergePr
                    | Action::ApprovePr
                    | Action::RequestChanges
                    | Action::CommentPr
                    | Action::ApplyStash
                    | Action::PopStash
                    | Action::EditRemote
                    | Action::EditPushUrl
                    | Action::DeleteRemoteTag
                    | Action::PageDown
                    | Action::PageUp
                    | Action::ScrollLeft
                    | Action::ScrollRight
            ),
        }
    }

    pub(super) fn apply(&mut self, action: Action) -> Result<()> {
        if self.view == ViewMode::Files && self.browse_tracked && action == Action::OpenHunks {
            self.message = "Return to working changes before selecting editable hunks".into();
            return Ok(());
        }
        if self.view == ViewMode::Files
            && (self.browse_tracked || self.file_detail != FileDetail::Diff)
            && matches!(
                action,
                Action::Stage
                    | Action::Unstage
                    | Action::Remove
                    | Action::RestoreFromIndex
                    | Action::ToggleSeen
            )
        {
            self.message = "File inspection is read-only; return to working changes first".into();
            return Ok(());
        }
        let handled = match self.view {
            ViewMode::Files => self.files_action(action)?,
            ViewMode::History => self.history_action(action)?,
            ViewMode::Branches => self.branches_action(action)?,
            ViewMode::Conflicts => self.conflicts_action(action)?,
            ViewMode::Workspaces => self.workspaces_action(action)?,
            ViewMode::PullRequests => self.pull_requests_action(action)?,
            ViewMode::GitHubRepositories => self.github_repositories_action(action),
            ViewMode::Stashes => self.stashes_action(action),
            ViewMode::Remotes => self.remotes_action(action)?,
            ViewMode::RemoteBranches => self.remote_branches_action(action),
            ViewMode::Tags => self.tags_action(action),
            ViewMode::Hunks => self.hunks_action(action)?,
            ViewMode::Reflog => self.reflog_action(action)?,
            ViewMode::PrFiles => self.pr_files_action(action),
            ViewMode::RecentRepositories => self.recent_repositories_action(action)?,
        };
        if handled {
            return Ok(());
        }
        self.global_action(action)
    }

    fn global_action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::CommandPalette => {
                self.palette_selection = 0;
                self.start_prompt(PromptKind::CommandPalette, vec!["Command search"]);
            }
            Action::ImportPatch => self.start_prompt(
                PromptKind::ImportPatch,
                vec!["Mail patch file", "Type apply to import commits"],
            ),
            Action::RecentRepositories => {
                self.capture_navigation();
                self.view = ViewMode::RecentRepositories;
                self.recent_selection = 0;
                self.refresh_detail();
            }
            Action::SetUpstream => self.start_prompt(
                PromptKind::SetUpstream,
                vec!["Upstream ref (origin/main; empty unsets)"],
            ),
            Action::PublishBranch => self.start_prompt(
                PromptKind::PublishBranch,
                vec!["Remote to publish current branch"],
            ),
            Action::PullStrategy => self.start_prompt(
                PromptKind::PullStrategy,
                vec!["Pull strategy: ff-only, merge or rebase"],
            ),
            Action::SaveStash => self.start_prompt(
                PromptKind::SaveStash,
                vec!["Stash message (includes untracked files)"],
            ),
            Action::GitHubStatus => {
                let status = crate::github::run(self.repository.root(), &["auth", "status"])?;
                self.set_detail(status);
            }
            Action::OpenRepository => {
                self.start_prompt(PromptKind::OpenRepository, vec!["Repository path"])
            }
            Action::ScrollLeft => self.detail_column = self.detail_column.saturating_sub(10),
            Action::ScrollRight => {
                self.detail_column = self
                    .detail_column
                    .saturating_add(10)
                    .min(widest_line(&self.detail_text));
            }
            Action::PageDown => {
                let last = self.detail_text.lines().count().saturating_sub(1);
                self.detail_scroll = self
                    .detail_scroll
                    .saturating_add(10)
                    .min(last.min(u16::MAX as usize) as u16);
            }
            Action::PageUp => self.detail_scroll = self.detail_scroll.saturating_sub(10),
            Action::Search => {
                self.searching = true;
                self.search.clear();
            }
            Action::NextMatch => self.find_match(true),
            Action::Quit => self.running = false,
            Action::Refresh => {
                self.refresh()?;
                self.notify("Refreshed");
            }
            Action::NextView => self.enter_view(self.view.next())?,
            Action::NextItem => self.move_selection(1),
            // Navigation must not silently skip a rebase commit in the conflicts view.
            Action::PreviousItem => self.move_selection(-1),
            Action::Commit => self.start_prompt(PromptKind::Commit(false), vec!["Commit message"]),
            Action::AmendCommit => self.start_selected_prompt(
                "Amend rewrites the latest commit; enter its replacement message".into(),
                PromptKind::Commit(true),
                vec!["Amend commit message"],
            ),
            Action::Branch => {
                self.start_prompt(PromptKind::Branch, vec!["Create or switch branch"])
            }
            Action::Fetch => self.run_action(Repository::fetch, "Fetched remotes")?,
            Action::Pull => self.run_action(Repository::pull, "Pulled changes")?,
            Action::Push => self.run_action(Repository::push, "Pushed changes")?,
            _ => {}
        }
        Ok(())
    }

    fn files_action(&mut self, action: Action) -> Result<bool> {
        let editable = !self.upstream_comparison;
        match action {
            Action::BrowseTracked => {
                self.browse_tracked = !self.browse_tracked;
                self.upstream_comparison = false;
                self.file_detail = FileDetail::Diff;
                self.file_selection = 0;
                self.tracked_files = if self.browse_tracked {
                    self.load_tracked_files()?
                } else {
                    Vec::new()
                };
                self.refresh_detail();
                self.message = if self.browse_tracked {
                    "Browsing tracked files (read-only)"
                } else {
                    "Working changes"
                }
                .into();
            }
            Action::FileHistory | Action::FileBlame => {
                let mode = if action == Action::FileHistory {
                    FileDetail::History
                } else {
                    FileDetail::Blame
                };
                self.file_detail = if self.file_detail == mode {
                    FileDetail::Diff
                } else {
                    mode
                };
                self.refresh_detail();
            }
            Action::Remove | Action::RestoreFromIndex if editable => {
                if let Some(file) = self.state.files.get(self.file_selection) {
                    let request = self
                        .repository
                        .prepare_restore(file, action == Action::Remove)?;
                    let effect = if file.status == "??" {
                        "delete untracked file"
                    } else if request.from_head {
                        "restore HEAD in index and working tree"
                    } else {
                        "restore working file from index; staged changes retained"
                    };
                    self.start_selected_prompt(
                        format!("Discard {}: {effect}", file.label()),
                        PromptKind::RestoreFile(request),
                        vec!["Type discard to confirm selected file"],
                    );
                }
            }
            Action::OpenHunks if editable => {
                self.hunk_selection = 0;
                self.enter_view(ViewMode::Hunks)?;
            }
            Action::ToggleComparison => {
                if !self.upstream_comparison {
                    self.review_files = self.repository.upstream_files()?;
                }
                self.browse_tracked = false;
                self.file_detail = FileDetail::Diff;
                self.upstream_comparison = !self.upstream_comparison;
                self.file_selection = 0;
                self.refresh_detail();
                self.notify(if self.upstream_comparison {
                    "Compared with upstream merge base"
                } else {
                    "Working changes"
                });
            }
            Action::ToggleSeen => {
                if let Some(file) = self.displayed_files().get(self.file_selection) {
                    let key = (self.upstream_comparison, file.path.clone());
                    if self.seen.remove(&key).is_none() {
                        let detail = self.selected_detail()?;
                        self.seen.insert(key, detail);
                    }
                }
            }
            Action::Stage | Action::Unstage if editable => {
                if let Some(file) = self.state.files.get(self.file_selection).cloned() {
                    if action == Action::Stage {
                        self.run_action(
                            |repo| repo.stage(&file),
                            format!("Staged {}", file.label()),
                        )?;
                    } else {
                        self.run_action(
                            |repo| repo.unstage(&file),
                            format!("Unstaged {}", file.label()),
                        )?;
                    }
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn history_action(&mut self, action: Action) -> Result<bool> {
        match action {
            Action::ToggleGraph => {
                self.graph_visible = !self.graph_visible;
                self.load_view_data(true)?;
                self.refresh_detail();
            }
            Action::LoadHistory => {
                self.history_limit = self.history_limit.saturating_add(100);
                self.load_view_data(true)?;
                self.refresh_detail();
                self.notify(format!(
                    "Loaded {} commits across branches",
                    self.state.commits.len()
                ));
            }
            Action::ExportCommit
            | Action::ResetCommit
            | Action::CherryPick
            | Action::RevertCommit => {
                if let Some(commit) = self.state.commits.get(self.history_selection).cloned() {
                    self.commit_action(commit, action)?;
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn commit_action(&mut self, commit: crate::model::CommitEntry, action: Action) -> Result<()> {
        match action {
            Action::ExportCommit => self.start_prompt(
                PromptKind::ExportCommit(commit.sha),
                vec!["New mail patch destination (existing files refused)"],
            ),
            Action::ResetCommit => {
                let request = self.repository.prepare_reset(&commit.sha)?;
                self.start_selected_prompt(
                    format!(
                        "Reset to {}: soft keeps index/files; mixed unstages; hard discards tracked changes",
                        request.target
                    ),
                    PromptKind::ResetCommit(request),
                    vec![
                        "Reset mode: soft, mixed, hard",
                        "Type full target commit ID to confirm",
                    ],
                );
            }
            Action::CherryPick | Action::RevertCommit => {
                let revert = action == Action::RevertCommit;
                self.start_selected_prompt(
                    format!(
                        "{} {}: {}",
                        if revert { "Revert" } else { "Cherry-pick" },
                        commit.short_sha,
                        commit.subject
                    ),
                    PromptKind::ApplyCommit(commit.sha, revert),
                    vec![
                        "Type full selected commit ID to confirm",
                        "Mainline parent for merge commit (blank otherwise)",
                    ],
                );
            }
            _ => {}
        }
        Ok(())
    }

    fn branches_action(&mut self, action: Action) -> Result<bool> {
        let Some(branch) = self.state.branches.get(self.branch_selection).cloned() else {
            return Ok(matches!(
                action,
                Action::SwitchBranch
                    | Action::RenameBranch
                    | Action::MergeBranch
                    | Action::RebaseBranch
                    | Action::Remove
            ));
        };
        let name = branch.name;
        let selected = format!("Selected branch: {name}");
        match action {
            Action::SwitchBranch => {
                self.run_action(
                    |repo| repo.switch_branch(&name),
                    format!("Switched to {name}"),
                )?;
            }
            Action::RenameBranch => self.start_selected_prompt(
                selected,
                PromptKind::RenameBranch(name),
                vec!["New branch name"],
            ),
            Action::MergeBranch => self.start_selected_prompt(
                selected,
                PromptKind::IntegrateBranch(name, false),
                vec!["Type merge to merge into the current branch"],
            ),
            Action::RebaseBranch => self.start_selected_prompt(
                selected,
                PromptKind::IntegrateBranch(name, true),
                vec!["Type rebase to rebase the current branch onto selection"],
            ),
            Action::Remove => self.start_selected_prompt(
                selected,
                PromptKind::DeleteBranch(name),
                vec!["Type delete to remove a merged branch"],
            ),
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn conflicts_action(&mut self, action: Action) -> Result<bool> {
        let selected = self
            .state
            .merge_state
            .conflicts
            .get(self.conflict_selection)
            .cloned();
        match action {
            Action::TakeOurs | Action::TakeTheirs => {
                if let Some(path) = selected {
                    let side = if action == Action::TakeOurs {
                        ConflictSide::Ours
                    } else {
                        ConflictSide::Theirs
                    };
                    self.run_action(
                        |repo| repo.resolve_side(&path, side),
                        format!("Resolved {} with {side:?}", display_path(&path)),
                    )?;
                }
            }
            Action::MarkResolved => {
                if let Some(path) = selected {
                    self.run_action(
                        |repo| repo.mark_resolved(&path),
                        format!("Marked {} as resolved", display_path(&path)),
                    )?;
                }
            }
            Action::Continue => {
                self.run_action(Repository::continue_operation, "Continued Git operation")?
            }
            Action::SkipRebase => {
                self.run_action(Repository::skip_rebase, "Skipped current rebase commit")?
            }
            Action::Abort => {
                self.run_action(Repository::abort_operation, "Aborted Git operation")?
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn workspaces_action(&mut self, action: Action) -> Result<bool> {
        match action {
            Action::New => self.start_prompt(
                PromptKind::Workspace,
                vec![
                    "Workspace path (relative to repo or absolute)",
                    "New branch name",
                ],
            ),
            Action::Remove => {
                if let Some(w) = self.workspaces.get(self.workspace_selection).cloned() {
                    self.start_selected_prompt(
                        format!("Remove {}? Branch will remain.", display_path(&w.path)),
                        PromptKind::Remove(w),
                        vec!["Type remove to confirm"],
                    );
                }
            }
            Action::SwitchBranch => {
                if let Some(w) = self.workspaces.get(self.workspace_selection) {
                    let path = w.path.clone();
                    self.open_repository(&path)?;
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn pull_requests_action(&mut self, action: Action) -> Result<bool> {
        match action {
            Action::New => self.start_prompt(
                PromptKind::CreatePr,
                vec![
                    "Draft PR title",
                    "PR body",
                    "Base branch",
                    "Published head branch (or owner:branch)",
                ],
            ),
            Action::LoadHistory => {
                self.pr_filter.limit = self.pr_filter.limit.saturating_add(100);
                self.load_view_data(true)?;
                self.refresh_detail();
                self.notify(format!(
                    "Loaded {} PRs (limit {}); search may be capped by GitHub, refine U filter",
                    self.pull_requests.len(),
                    self.pr_filter.limit
                ));
            }
            Action::SetUpstream => self.start_prefilled_prompt(
                PromptKind::FilterPr,
                vec![
                    "PR state: open, closed, merged, all",
                    "GitHub search query (blank = none)",
                ],
                vec![self.pr_filter.state.clone(), self.pr_filter.search.clone()],
            ),
            _ => return self.selected_pull_request_action(action),
        }
        Ok(true)
    }

    fn selected_pull_request_action(&mut self, action: Action) -> Result<bool> {
        let Some(pr) = self.pull_requests.get(self.pr_selection).cloned() else {
            return Ok(matches!(
                action,
                Action::EditRemote
                    | Action::PopStash
                    | Action::OpenHunks
                    | Action::MergePr
                    | Action::ApprovePr
                    | Action::RequestChanges
                    | Action::CommentPr
                    | Action::PrDiff
                    | Action::SwitchBranch
            ));
        };
        let root = self.repository.root().to_owned();
        let selected = format!("Selected PR #{}: {} ({})", pr.number, pr.title, pr.url);
        match action {
            Action::EditRemote => {
                let expected = crate::github::editable(&root, pr.number)?;
                let defaults = vec![
                    expected.title.clone(),
                    expected.body.clone(),
                    expected.base_ref_name.clone(),
                ];
                self.message = format!("Edit PR #{}: {}", pr.number, pr.url);
                self.start_prefilled_prompt(
                    PromptKind::EditPr(expected),
                    vec![
                        "PR title",
                        "PR body (Ctrl-U clears; Alt-Enter newline; paste supported)",
                        "Base branch",
                    ],
                    defaults,
                );
            }
            Action::PopStash => {
                let transition = if pr.is_draft {
                    "draft -> ready"
                } else {
                    "ready -> draft"
                };
                self.start_selected_prompt(
                    format!("PR #{} {transition}: {}", pr.number, pr.url),
                    PromptKind::ReadyPr(pr),
                    vec!["Type selected PR number to confirm draft transition"],
                );
            }
            Action::OpenHunks => {
                self.review_pr = Some(pr);
                self.pr_files.clear();
                self.pr_file_selection = 0;
                self.enter_view(ViewMode::PrFiles)?;
            }
            Action::MergePr => self.start_selected_prompt(
                selected,
                PromptKind::Merge(pr),
                vec![
                    "Confirm merge: enter PR number",
                    "Merge method: merge, squash, rebase",
                ],
            ),
            Action::ApprovePr => self.start_selected_prompt(
                selected,
                PromptKind::Review(pr, "--approve"),
                vec!["Approve review body"],
            ),
            Action::RequestChanges => self.start_selected_prompt(
                selected,
                PromptKind::Review(pr, "--request-changes"),
                vec!["Requested changes"],
            ),
            Action::CommentPr => self.start_selected_prompt(
                selected,
                PromptKind::Comment(pr.number),
                vec!["PR comment"],
            ),
            Action::PrDiff => {
                let diff = crate::github::run(
                    &root,
                    &["pr", "diff", &pr.number.to_string(), "--color", "never"],
                )?;
                self.set_detail(diff);
            }
            Action::SwitchBranch => {
                self.repository.ensure_clean()?;
                let number = pr.number;
                let result = crate::github::checkout(&root, number)
                    .map(|_| format!("Checked out PR #{number}"));
                self.finish(result)?;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn github_repositories_action(&mut self, action: Action) -> bool {
        match action {
            Action::SwitchBranch => {
                if let Some(repo) = self.github_repositories.get(self.github_repo_selection) {
                    self.start_prompt(
                        PromptKind::CloneRepository(repo.name_with_owner.clone()),
                        vec!["Clone destination (new directory)"],
                    );
                }
            }
            Action::New => self.start_selected_prompt(
                "Create a GitHub repository; a remote name links it to this checkout without pushing"
                    .into(),
                PromptKind::CreateGitHubRepository,
                vec![
                    "Repository name (name or owner/name)",
                    "Visibility: private, public or internal",
                    "Remote name to add for this checkout (blank = none)",
                    "Type create to confirm",
                ],
            ),
            _ => return false,
        }
        true
    }

    fn stashes_action(&mut self, action: Action) -> bool {
        let Some(stash) = self.stashes.get(self.stash_selection).cloned() else {
            return matches!(
                action,
                Action::ApplyStash | Action::PopStash | Action::Remove
            );
        };
        let selected = format!("Selected {}: {}", stash.selector, stash.subject);
        match action {
            Action::ApplyStash => self.start_selected_prompt(
                selected,
                PromptKind::ApplyStash(stash, false),
                vec!["Type apply to restore stash and index"],
            ),
            Action::PopStash => self.start_selected_prompt(
                selected,
                PromptKind::ApplyStash(stash, true),
                vec!["Type pop to restore and remove on success"],
            ),
            Action::Remove => self.start_selected_prompt(
                selected,
                PromptKind::DropStash(stash),
                vec!["Type drop to delete selected stash"],
            ),
            _ => return false,
        }
        true
    }

    fn remotes_action(&mut self, action: Action) -> Result<bool> {
        if action == Action::New {
            self.start_prompt(PromptKind::AddRemote, vec!["Remote name", "Remote URL"]);
            return Ok(true);
        }
        let Some(remote) = self.remotes.get(self.remote_selection) else {
            return Ok(matches!(
                action,
                Action::EditRemote | Action::EditPushUrl | Action::Remove | Action::Fetch
            ));
        };
        let name = remote.name.clone();
        let selected = format!("Selected remote: {name}");
        match action {
            Action::EditPushUrl => self.start_selected_prompt(
                selected,
                PromptKind::EditPushUrl(name),
                vec!["New push URL"],
            ),
            Action::EditRemote => self.start_selected_prompt(
                selected,
                PromptKind::EditRemote(name),
                vec!["New fetch URL"],
            ),
            Action::Remove => self.start_selected_prompt(
                selected,
                PromptKind::RemoveRemote(name),
                vec!["Type remove to remove remote and tracking refs"],
            ),
            Action::Fetch => {
                self.run_action(|repo| repo.fetch_remote(&name), "Fetched selected remote")?;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn remote_branches_action(&mut self, action: Action) -> bool {
        if action != Action::SwitchBranch {
            return false;
        }
        if let Some(reference) = self.remote_branches.get(self.remote_branch_selection) {
            self.start_prompt(
                PromptKind::CheckoutRemote(reference.clone()),
                vec!["New local tracking branch name"],
            );
        }
        true
    }

    fn tags_action(&mut self, action: Action) -> bool {
        if action == Action::New {
            self.start_prompt(
                PromptKind::CreateTag,
                vec![
                    "Tag name",
                    "Target commit (empty = HEAD)",
                    "Annotation (empty = lightweight)",
                ],
            );
            return true;
        }
        let Some(tag) = self.tags.get(self.tag_selection).cloned() else {
            return matches!(
                action,
                Action::Remove | Action::PublishBranch | Action::DeleteRemoteTag
            );
        };
        let selected = format!("Selected tag: {} ({})", tag.name, tag.sha);
        match action {
            Action::Remove => self.start_selected_prompt(
                selected,
                PromptKind::DeleteTag(tag),
                vec!["Type delete to remove local tag"],
            ),
            Action::PublishBranch => self.start_selected_prompt(
                selected,
                PromptKind::PublishTag(tag),
                vec!["Remote to publish selected tag"],
            ),
            Action::DeleteRemoteTag => self.start_selected_prompt(
                selected,
                PromptKind::DeleteRemoteTag(tag),
                vec![
                    "Remote to delete selected tag from",
                    "Type delete to confirm remote deletion",
                ],
            ),
            _ => return false,
        }
        true
    }

    fn hunks_action(&mut self, action: Action) -> Result<bool> {
        match action {
            Action::ToggleComparison => {
                self.staged_hunks = !self.staged_hunks;
                self.hunk_selection = 0;
                self.load_view_data(true)?;
                self.refresh_detail();
            }
            Action::Stage | Action::Unstage if !self.upstream_comparison => {
                // Staging applies to unstaged hunks; unstaging to staged ones. Mismatches are no-ops.
                if (action == Action::Unstage) == self.staged_hunks
                    && let Some(hunk) = self.hunks.get(self.hunk_selection).cloned()
                {
                    self.run_action(
                        |repo| repo.apply_hunk(&hunk),
                        "Updated selected hunk in index",
                    )?;
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn reflog_action(&mut self, action: Action) -> Result<bool> {
        match action {
            Action::New => {
                if let Some(entry) = self.reflog.get(self.reflog_selection) {
                    let (sha, subject) = (entry.sha.clone(), entry.subject.clone());
                    self.start_selected_prompt(
                        format!("Recover {sha}: {subject}"),
                        PromptKind::RecoverCommit(sha),
                        vec!["New recovery branch name (checkout unchanged)"],
                    );
                }
            }
            Action::LoadHistory => {
                self.reflog_limit = self.reflog_limit.saturating_add(100);
                self.load_view_data(true)?;
                self.refresh_detail();
                self.notify(format!("Loaded {} reflog entries", self.reflog.len()));
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn pr_files_action(&mut self, action: Action) -> bool {
        if action != Action::CommentPr {
            return false;
        }
        if let (Some(pr), Some(file)) = (
            self.review_pr.clone(),
            self.pr_files.get(self.pr_file_selection).cloned(),
        ) {
            self.start_selected_prompt(
                format!(
                    "Comment PR #{} {} at reviewed head {}",
                    pr.number,
                    file.filename.escape_debug(),
                    pr.head_ref_oid
                ),
                PromptKind::InlinePr(pr, file),
                vec![
                    "Diff side: LEFT or RIGHT",
                    "Displayed old/new line number",
                    "Inline comment body",
                ],
            );
        }
        true
    }

    fn recent_repositories_action(&mut self, action: Action) -> Result<bool> {
        match action {
            Action::New => self.start_prefilled_prompt(
                PromptKind::InitializeRepository,
                vec!["New repository path", "Initial branch"],
                vec![String::new(), "main".into()],
            ),
            Action::CommentPr => self.start_prompt(
                PromptKind::CloneGitRepository,
                vec![
                    "Git URL or local source path",
                    "Clone destination (empty or new directory)",
                ],
            ),
            Action::SwitchBranch => {
                if let Some(settings) = self.navigation.repositories.get(self.recent_selection) {
                    let path = settings.root.path()?;
                    self.open_repository(&path)?;
                }
            }
            Action::Remove => {
                if self.recent_selection < self.navigation.repositories.len() {
                    self.navigation.repositories.remove(self.recent_selection);
                    super::clamp(
                        &mut self.recent_selection,
                        self.navigation.repositories.len(),
                    );
                    self.refresh_detail();
                    self.notify("Forgot recent entry; working tree remains on disk");
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}
