//! Actions are UI-independent commands; `apply` is the one place that decides what
//! each means in the current view.
use anyhow::Result;

use super::prompts::{
    BranchPrompt, GitHubPrompt, HistoryPrompt, RemotePrompt, RepositoryPrompt, StashPrompt,
    TagPrompt, WorkingTreePrompt,
};
use super::{App, FileDetail, PromptKind, clamp_to};
use crate::git::{ConflictSide, Repository};
use crate::model::{ViewMode, display_path};

/// UI-independent commands; terminal key bindings live in `input`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    WorktreeFrom,
    WorktreeExisting,
    LockWorktree,
    UnlockWorktree,
    MoveWorktree,
    EditLines,
    RawInteractiveRebase,
    ToggleFileSelection,
    StashSelected,
    ClearFilter,
    FilterList,
    StageLines,
    InteractiveRebase,
    CommandPalette,
    ImportPatch,
    BrowseTracked,
    ExportCommit,
    FileHistory,
    FileBlame,
    RecentRepositories,
    ResetCommit,
    ToggleGraph,
    LoadHistory,
    CherryPick,
    RevertCommit,
    RestoreFromIndex,
    OpenHunks,
    DeleteRemoteTag,
    EditPushUrl,
    EditRemote,
    SetUpstream,
    PublishBranch,
    PullStrategy,
    SaveStash,
    ApplyStash,
    PopStash,
    AmendCommit,
    RenameBranch,
    MergeBranch,
    RebaseBranch,
    GitHubStatus,
    New,
    Remove,
    OpenRepository,
    PrDiff,
    MergePr,
    ApprovePr,
    RequestChanges,
    CommentPr,
    ScrollLeft,
    ScrollRight,
    PageDown,
    PageUp,
    ToggleComparison,
    ToggleSeen,
    Search,
    NextMatch,
    Quit,
    Refresh,
    NextView,
    NextItem,
    PreviousItem,
    SwitchBranch,
    Stage,
    Unstage,
    Commit,
    Branch,
    Fetch,
    Pull,
    Push,
    TakeOurs,
    TakeTheirs,
    MarkResolved,
    Continue,
    SkipRebase,
    Abort,
}

impl Action {
    /// Actions that only change UI state (open a prompt, scroll the detail pane) run on
    /// the terminal thread; everything that touches Git or GitHub runs in the worker.
    pub fn runs_locally(self, view: ViewMode) -> bool {
        match self {
            Self::WorktreeFrom
            | Self::WorktreeExisting
            | Self::LockWorktree
            | Self::UnlockWorktree
            | Self::EditLines
            | Self::ToggleFileSelection
            | Self::FilterList
            | Self::StageLines
            | Self::Quit
            | Self::CommandPalette
            | Self::ImportPatch
            | Self::ExportCommit
            | Self::Commit
            | Self::AmendCommit
            | Self::Branch
            | Self::New
            | Self::Search
            | Self::OpenRepository
            | Self::SaveStash
            | Self::SetUpstream
            | Self::PublishBranch
            | Self::PullStrategy
            | Self::RenameBranch
            | Self::MergeBranch
            | Self::RebaseBranch
            | Self::MergePr
            | Self::ApprovePr
            | Self::RequestChanges
            | Self::CommentPr
            | Self::ApplyStash
            | Self::PopStash
            | Self::EditPushUrl
            | Self::DeleteRemoteTag
            | Self::PageDown
            | Self::PageUp
            | Self::ScrollLeft
            | Self::ScrollRight => true,
            // Editing a pull request fetches its current text before the prompt opens.
            Self::EditRemote => view != ViewMode::PullRequests,
            // Discarding a file snapshots it before asking for confirmation.
            Self::Remove => !matches!(view, ViewMode::Files | ViewMode::Worktrees),
            _ => false,
        }
    }
}

impl App {
    /// Git failures are recoverable UI messages, not reasons to leave the terminal in raw mode.
    pub fn handle(&mut self, action: Action) {
        if let Err(error) = self.apply(action) {
            self.report_error(error);
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
                    | Action::ToggleFileSelection
                    | Action::StashSelected
            )
        {
            self.message = "File inspection is read-only; return to working changes first".into();
            return Ok(());
        }
        if !self.name_filter().is_empty()
            && matches!(self.view, ViewMode::Files | ViewMode::Branches)
            && !self.visible_indices().contains(&self.selection())
            && matches!(
                action,
                Action::Stage
                    | Action::Unstage
                    | Action::Remove
                    | Action::RestoreFromIndex
                    | Action::ToggleSeen
                    | Action::OpenHunks
                    | Action::FileHistory
                    | Action::FileBlame
                    | Action::SwitchBranch
                    | Action::RenameBranch
                    | Action::MergeBranch
                    | Action::RebaseBranch
                    | Action::ToggleFileSelection
            )
        {
            self.notify("No matching selection; clear the name filter first");
            return Ok(());
        }
        if matches!(action, Action::Stage | Action::Unstage) && self.line_editor.is_some() {
            let editor = self.line_editor.as_ref().unwrap();
            if (action == Action::Unstage) != editor.hunk.staged {
                self.notify("Use s for unstaged lines or u for staged lines");
                return Ok(());
            }
            let selected = editor.selection();
            if selected.is_empty() {
                self.notify("Select lines with Space or v before applying");
                return Ok(());
            }
            let partial = editor.hunk.select_lines(&selected)?;
            self.line_editor = None;
            return self.run_action(
                |repo| repo.apply_hunk(&partial),
                "Updated selected lines in index",
            );
        }
        match action {
            Action::FilterList if matches!(self.view, ViewMode::Files | ViewMode::Branches) => {
                self.start_prefilled_prompt(
                    PromptKind::ListFilter(self.view),
                    vec!["Name filter (blank clears)"],
                    vec![self.name_filter().to_owned()],
                );
            }
            Action::StageLines
                if self.view == ViewMode::Hunks
                    && !self.browse_tracked
                    && !self.upstream_comparison =>
            {
                if let Some(hunk) = self.hunks.selected().cloned() {
                    self.start_prompt(
                        WorkingTreePrompt::Lines(hunk),
                        vec!["Changed-line numbers separated by spaces"],
                    );
                }
            }
            Action::EditLines | Action::ToggleFileSelection
                if self.view == ViewMode::Hunks
                    && !self.browse_tracked
                    && !self.upstream_comparison =>
            {
                if let Some(hunk) = self.hunks.selected().cloned() {
                    anyhow::ensure!(
                        hunk.supports_line_selection(),
                        "Use whole-hunk staging for file creation or deletion"
                    );
                    let editor = super::editors::LineEditor::new(hunk);
                    if editor.count > 0 {
                        self.line_editor = Some(editor);
                    }
                }
            }
            Action::ToggleFileSelection
                if self.view == ViewMode::Files
                    && !self.browse_tracked
                    && !self.upstream_comparison =>
            {
                if let Some(file) = self.displayed_files().get(self.file_selection).cloned() {
                    if !self.stash_files.remove(&file.path) {
                        self.stash_files.insert(file.path.clone());
                    }
                    self.notify(format!(
                        "{} files selected for stash; $ saves selection",
                        self.stash_files.len()
                    ));
                }
            }
            Action::StashSelected
                if self.view == ViewMode::Files
                    && !self.browse_tracked
                    && !self.upstream_comparison =>
            {
                let files: Vec<_> = if self.stash_files.is_empty() {
                    if self.visible_indices().contains(&self.file_selection) {
                        self.displayed_files()
                            .get(self.file_selection)
                            .cloned()
                            .into_iter()
                            .collect()
                    } else {
                        Vec::new()
                    }
                } else {
                    self.state
                        .files
                        .iter()
                        .filter(|f| self.stash_files.contains(&f.path))
                        .cloned()
                        .collect()
                };
                let request = self.repository.prepare_stash_files(&files)?;
                self.detail_text = format!(
                    "Stash selected files (other files remain):\n{}",
                    files
                        .iter()
                        .map(|f| f.label())
                        .collect::<Vec<_>>()
                        .join("\n")
                );
                self.start_prompt(
                    StashPrompt::SaveSelected(request),
                    vec![
                        "Selected stash message",
                        "Type stash to save only these files",
                    ],
                );
            }
            Action::ClearFilter if matches!(self.view, ViewMode::Files | ViewMode::Branches) => {
                self.update_name_filter(self.view, "");
                self.refresh_detail();
            }
            Action::InteractiveRebase if self.view == ViewMode::History => {
                if let Some(commit) = self.state.commits.get(self.history_selection) {
                    let plan = self.repository.prepare_interactive_rebase(&commit.sha)?;
                    self.rebase_editor = Some(super::editors::RebaseEditor::new(plan));
                }
            }
            Action::RawInteractiveRebase if self.view == ViewMode::History => {
                if let Some(commit) = self.state.commits.get(self.history_selection) {
                    let plan = self.repository.prepare_interactive_rebase(&commit.sha)?;
                    let todo = plan.todo.clone();
                    self.start_prefilled_prompt(
                        HistoryPrompt::Interactive(plan),
                        vec![
                            "Rebase todo: pick / squash / fixup / edit / drop; newline or ; separates entries",
                            "Type rebase to rewrite local history",
                        ],
                        vec![todo, String::new()],
                    );
                }
            }
            Action::CommandPalette => {
                self.palette_selection = 0;
                self.start_prompt(PromptKind::CommandPalette, vec!["Command search"]);
            }
            Action::ImportPatch => self.start_prompt(
                HistoryPrompt::ImportPatch,
                vec!["Mail patch file", "Type apply to import commits"],
            ),
            Action::New if self.view == ViewMode::RecentRepositories => self
                .start_prefilled_prompt(
                    RepositoryPrompt::Initialize,
                    vec!["New repository path", "Initial branch"],
                    vec![String::new(), "main".into()],
                ),
            Action::CommentPr if self.view == ViewMode::RecentRepositories => self.start_prompt(
                RepositoryPrompt::CloneGit,
                vec![
                    "Git URL or local source path",
                    "Clone destination (empty or new directory)",
                ],
            ),
            Action::RecentRepositories => {
                self.capture_navigation();
                self.view = ViewMode::RecentRepositories;
                self.recent_selection = 0;
                self.refresh_detail();
            }
            Action::SwitchBranch if self.view == ViewMode::RecentRepositories => {
                if let Some(settings) = self.navigation.repositories.get(self.recent_selection) {
                    let path = settings.root.path()?;
                    self.open_repository(&path)?;
                }
            }
            Action::Remove if self.view == ViewMode::RecentRepositories => {
                if self.recent_selection < self.navigation.repositories.len() {
                    self.navigation.repositories.remove(self.recent_selection);
                    clamp_to(
                        &mut self.recent_selection,
                        self.navigation.repositories.len(),
                    );
                    self.refresh_detail();
                    self.notify("Forgot recent entry; working tree remains on disk");
                }
            }
            Action::LoadHistory if self.view == ViewMode::PullRequests => {
                self.pr_filter.limit = self.pr_filter.limit.saturating_add(100);
                self.load_extra()?;
                self.refresh_detail();
                self.notify(format!(
                    "Loaded {} PRs (limit {}); search may be capped by GitHub, refine U filter",
                    self.pull_requests.items.len(),
                    self.pr_filter.limit
                ));
            }
            Action::SetUpstream if self.view == ViewMode::PullRequests => self
                .start_prefilled_prompt(
                    GitHubPrompt::Filter,
                    vec![
                        "PR state: open, closed, merged, all",
                        "GitHub search query (blank = none)",
                    ],
                    vec![self.pr_filter.state.clone(), self.pr_filter.search.clone()],
                ),
            Action::EditRemote if self.view == ViewMode::PullRequests => {
                if let Some(pr) = self.pull_requests.selected() {
                    let expected = crate::github::editable(self.repository.root(), pr.number)?;
                    self.message = format!("Edit PR #{}: {}", pr.number, pr.url);
                    let defaults = vec![
                        expected.title.clone(),
                        expected.body.clone(),
                        expected.base_ref_name.clone(),
                    ];
                    self.start_prefilled_prompt(
                        GitHubPrompt::Edit(expected),
                        vec![
                            "PR title",
                            "PR body (Ctrl-U clears; Alt-Enter newline; paste supported)",
                            "Base branch",
                        ],
                        defaults,
                    );
                }
            }
            Action::PopStash if self.view == ViewMode::PullRequests => {
                if let Some(pr) = self.pull_requests.selected().cloned() {
                    self.message = format!(
                        "PR #{} {}: {}",
                        pr.number,
                        if pr.is_draft {
                            "draft -> ready"
                        } else {
                            "ready -> draft"
                        },
                        pr.url
                    );
                    self.start_prompt(
                        GitHubPrompt::Ready(pr),
                        vec!["Type selected PR number to confirm draft transition"],
                    );
                }
            }
            Action::OpenHunks if self.view == ViewMode::PullRequests => {
                if let Some(pr) = self.pull_requests.selected().cloned() {
                    self.review_pr = Some(pr);
                    self.pr_files.items.clear();
                    self.pr_files.selection = 0;
                    self.view = ViewMode::PrFiles;
                    self.load_extra()?;
                    self.refresh_detail();
                }
            }
            Action::CommentPr if self.view == ViewMode::PrFiles => {
                if let (Some(pr), Some(file)) = (self.review_pr.as_ref(), self.pr_files.selected())
                {
                    self.message = format!(
                        "Comment PR #{} {} at reviewed head {}",
                        pr.number,
                        file.filename.escape_debug(),
                        pr.head_ref_oid
                    );
                    self.start_prompt(
                        GitHubPrompt::InlineComment(pr.clone(), file.clone()),
                        vec![
                            "Diff side: LEFT or RIGHT",
                            "Displayed old/new line number",
                            "Inline comment body",
                        ],
                    );
                }
            }
            Action::BrowseTracked if self.view == ViewMode::Files => {
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
            Action::ExportCommit if self.view == ViewMode::History => {
                if let Some(commit) = self.state.commits.get(self.history_selection) {
                    self.start_prompt(
                        HistoryPrompt::ExportCommit(commit.sha.clone()),
                        vec!["New mail patch destination (existing files refused)"],
                    );
                }
            }
            Action::FileHistory | Action::FileBlame if self.view == ViewMode::Files => {
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
            Action::ToggleGraph if self.view == ViewMode::History => {
                self.graph_visible = !self.graph_visible;
                self.load_extra()?;
                self.refresh_detail();
            }
            Action::New if self.view == ViewMode::Reflog => {
                if let Some(entry) = self.reflog.selected() {
                    self.message = format!("Recover {}: {}", entry.sha, entry.subject);
                    self.start_prompt(
                        HistoryPrompt::Recover(entry.sha.clone()),
                        vec!["New recovery branch name (checkout unchanged)"],
                    );
                }
            }
            Action::LoadHistory if self.view == ViewMode::Reflog => {
                self.reflog_limit = self.reflog_limit.saturating_add(100);
                self.load_extra()?;
                self.refresh_detail();
                self.notify(format!("Loaded {} reflog entries", self.reflog.items.len()));
            }
            Action::ResetCommit if self.view == ViewMode::History => {
                if let Some(commit) = self.state.commits.get(self.history_selection) {
                    let request = self.repository.prepare_reset(&commit.sha)?;
                    self.message = format!(
                        "Reset to {}: soft keeps index/files; mixed unstages; hard discards tracked changes",
                        request.target
                    );
                    self.start_prompt(
                        HistoryPrompt::Reset(request),
                        vec![
                            "Reset mode: soft, mixed, hard",
                            "Type full target commit ID to confirm",
                        ],
                    );
                }
            }
            Action::LoadHistory if self.view == ViewMode::History => {
                self.history_limit = self.history_limit.saturating_add(100);
                self.load_extra()?;
                self.refresh_detail();
                self.notify(format!(
                    "Loaded {} commits across branches",
                    self.state.commits.len()
                ));
            }
            Action::CherryPick | Action::RevertCommit if self.view == ViewMode::History => {
                if let Some(commit) = self.state.commits.get(self.history_selection) {
                    let sha = commit.sha.clone();
                    let revert = action == Action::RevertCommit;
                    self.message = format!(
                        "{} {}: {}",
                        if revert { "Revert" } else { "Cherry-pick" },
                        commit.short_sha,
                        commit.subject
                    );
                    self.start_prompt(
                        HistoryPrompt::Apply(sha, revert),
                        vec![
                            "Type full selected commit ID to confirm",
                            "Mainline parent for merge commit (blank otherwise)",
                        ],
                    );
                }
            }
            Action::Remove | Action::RestoreFromIndex
                if self.view == ViewMode::Files && !self.upstream_comparison =>
            {
                if let Some(file) = self.state.files.get(self.file_selection) {
                    let request = self
                        .repository
                        .prepare_restore(file, action == Action::Remove)?;
                    self.message = format!(
                        "Discard {}: {}",
                        file.label(),
                        if file.status == "??" {
                            "delete untracked file"
                        } else if request.from_head {
                            "restore HEAD in index and working tree"
                        } else {
                            "restore working file from index; staged changes retained"
                        }
                    );
                    self.start_prompt(
                        WorkingTreePrompt::Restore(request),
                        vec!["Type discard to confirm selected file"],
                    );
                }
            }

            Action::OpenHunks if self.view == ViewMode::Files && !self.upstream_comparison => {
                self.view = ViewMode::Hunks;
                self.hunks.selection = 0;
                self.load_extra()?;
                self.refresh_detail();
            }
            Action::ToggleComparison if self.view == ViewMode::Hunks => {
                self.staged_hunks = !self.staged_hunks;
                self.hunks.selection = 0;
                self.load_extra()?;
                self.refresh_detail();
            }
            Action::Stage | Action::Unstage
                if self.view == ViewMode::Hunks && !self.upstream_comparison =>
            {
                if (action == Action::Unstage) == self.staged_hunks
                    && let Some(hunk) = self.hunks.selected().cloned()
                {
                    self.run_action(
                        |repo| repo.apply_hunk(&hunk),
                        "Updated selected hunk in index",
                    )?;
                }
            }

            Action::New if self.view == ViewMode::Tags => self.start_prompt(
                TagPrompt::Create,
                vec![
                    "Tag name",
                    "Target commit (empty = HEAD)",
                    "Annotation (empty = lightweight)",
                ],
            ),
            Action::Remove | Action::PublishBranch | Action::DeleteRemoteTag
                if self.view == ViewMode::Tags =>
            {
                if let Some(tag) = self.tags.selected().cloned() {
                    self.message = format!("Selected tag: {} ({})", tag.name, tag.sha);
                    match action {
                        Action::Remove => self.start_prompt(
                            TagPrompt::Delete(tag),
                            vec!["Type delete to remove local tag"],
                        ),
                        Action::PublishBranch => self.start_prompt(
                            TagPrompt::Publish(tag),
                            vec!["Remote to publish selected tag"],
                        ),
                        _ => self.start_prompt(
                            TagPrompt::DeleteRemote(tag),
                            vec![
                                "Remote to delete selected tag from",
                                "Type delete to confirm remote deletion",
                            ],
                        ),
                    }
                }
            }

            Action::New if self.view == ViewMode::Remotes => {
                self.start_prompt(RemotePrompt::Add, vec!["Remote name", "Remote URL"])
            }
            Action::EditRemote | Action::EditPushUrl | Action::Remove
                if self.view == ViewMode::Remotes =>
            {
                if let Some(remote) = self.remotes.selected() {
                    let name = remote.name.clone();
                    self.message = format!("Selected remote: {name}");
                    if action == Action::EditPushUrl {
                        self.start_prompt(RemotePrompt::EditPushUrl(name), vec!["New push URL"]);
                    } else if action == Action::EditRemote {
                        self.start_prompt(RemotePrompt::EditFetchUrl(name), vec!["New fetch URL"]);
                    } else {
                        self.start_prompt(
                            RemotePrompt::Remove(name),
                            vec!["Type remove to remove remote and tracking refs"],
                        );
                    }
                }
            }
            Action::SetUpstream => self.start_prompt(
                RemotePrompt::SetUpstream,
                vec!["Upstream ref (origin/main; empty unsets)"],
            ),
            Action::PublishBranch => self.start_prompt(
                RemotePrompt::PublishBranch,
                vec!["Remote to publish current branch"],
            ),
            Action::PullStrategy => self.start_prompt(
                RemotePrompt::PullStrategy,
                vec!["Pull strategy: ff-only, merge or rebase"],
            ),
            Action::SwitchBranch if self.view == ViewMode::RemoteBranches => {
                if let Some(reference) = self.remote_branches.selected() {
                    self.start_prompt(
                        RemotePrompt::CheckoutTracking(reference.clone()),
                        vec!["New local tracking branch name"],
                    );
                }
            }

            Action::SaveStash => self.start_prompt(
                StashPrompt::Save,
                vec!["Stash message (includes untracked files)"],
            ),
            Action::ApplyStash | Action::PopStash | Action::Remove
                if self.view == ViewMode::Stashes =>
            {
                if let Some(stash) = self.stashes.selected().cloned() {
                    self.message = format!("Selected {}: {}", stash.selector, stash.subject);
                    match action {
                        Action::ApplyStash => self.start_prompt(
                            StashPrompt::Apply(stash, false),
                            vec!["Type apply to restore stash and index"],
                        ),
                        Action::PopStash => self.start_prompt(
                            StashPrompt::Apply(stash, true),
                            vec!["Type pop to restore and remove on success"],
                        ),
                        _ => self.start_prompt(
                            StashPrompt::Drop(stash),
                            vec!["Type drop to delete selected stash"],
                        ),
                    }
                }
            }

            Action::GitHubStatus => {
                self.detail_text = crate::github::run(self.repository.root(), &["auth", "status"])?;
                self.detail_scroll = 0;
                self.detail_column = 0;
            }
            Action::SwitchBranch if self.view == ViewMode::GitHubRepositories => {
                if let Some(repo) = self.github_repositories.selected() {
                    self.start_prompt(
                        RepositoryPrompt::CloneGitHub(repo.name_with_owner.clone()),
                        vec!["Clone destination (new directory)"],
                    );
                }
            }
            Action::New if self.view == ViewMode::GitHubRepositories => {
                self.detail_text = "Create a GitHub repository; a remote name links it to this \
                                    checkout without pushing"
                    .into();
                self.detail_scroll = 0;
                self.start_prompt(
                    GitHubPrompt::CreateRepository,
                    vec![
                        "Repository name (name or owner/name)",
                        "Visibility: private, public or internal",
                        "Remote name to add for this checkout (blank = none)",
                        "Type create to confirm",
                    ],
                );
            }
            Action::OpenRepository => {
                self.start_prompt(RepositoryPrompt::Open, vec!["Repository path"])
            }
            Action::WorktreeFrom if self.view == ViewMode::Worktrees => self.start_prompt(
                RepositoryPrompt::CreateWorktreeFrom,
                vec!["Worktree path", "New branch name", "Base commit or ref"],
            ),
            Action::WorktreeExisting if self.view == ViewMode::Worktrees => self.start_prompt(
                RepositoryPrompt::CreateWorktreeExisting,
                vec!["Worktree path", "Existing local branch name"],
            ),
            Action::LockWorktree | Action::UnlockWorktree if self.view == ViewMode::Worktrees => {
                if let Some(workspace) = self.workspaces.selected().cloned() {
                    if action == Action::LockWorktree {
                        self.start_prompt(
                            RepositoryPrompt::LockWorktree(workspace),
                            vec!["Lock reason"],
                        );
                    } else {
                        self.start_prompt(
                            RepositoryPrompt::UnlockWorktree(workspace),
                            vec!["Type unlock to confirm"],
                        );
                    }
                }
            }
            Action::MoveWorktree if self.view == ViewMode::Worktrees => {
                if let Some(workspace) = self.workspaces.selected() {
                    let request = self.repository.prepare_worktree_removal(workspace)?;
                    self.start_prompt(
                        RepositoryPrompt::MoveWorktree(request),
                        vec!["New worktree path"],
                    );
                }
            }
            Action::New if self.view == ViewMode::Worktrees => self.start_prompt(
                RepositoryPrompt::CreateWorktree,
                vec![
                    "Worktree path (relative to repo or absolute)",
                    "New branch name",
                ],
            ),
            Action::New if self.view == ViewMode::PullRequests => self.start_prompt(
                GitHubPrompt::Create,
                vec![
                    "Draft PR title",
                    "PR body",
                    "Base branch",
                    "Published head branch (or owner:branch)",
                ],
            ),
            Action::Remove if self.view == ViewMode::Worktrees => {
                if let Some(w) = self.workspaces.selected().cloned() {
                    self.message = format!("Remove {}? Branch will remain.", display_path(&w.path));
                    self.start_prompt(
                        RepositoryPrompt::RemoveWorktree(
                            self.repository.prepare_worktree_removal(&w)?,
                        ),
                        vec!["Type remove to confirm"],
                    );
                }
            }
            Action::MergePr | Action::ApprovePr | Action::RequestChanges | Action::CommentPr
                if self.view == ViewMode::PullRequests =>
            {
                if let Some(pr) = self.pull_requests.selected().cloned() {
                    self.message = format!("Selected PR #{}: {} ({})", pr.number, pr.title, pr.url);
                    match action {
                        Action::MergePr => self.start_prompt(
                            GitHubPrompt::Merge(pr),
                            vec![
                                "Confirm merge: enter PR number",
                                "Merge method: merge, squash, rebase",
                            ],
                        ),
                        Action::ApprovePr => self.start_prompt(
                            GitHubPrompt::Review(pr, "--approve"),
                            vec!["Approve review body"],
                        ),
                        Action::RequestChanges => self.start_prompt(
                            GitHubPrompt::Review(pr, "--request-changes"),
                            vec!["Requested changes"],
                        ),
                        _ => {
                            self.start_prompt(GitHubPrompt::Comment(pr.number), vec!["PR comment"])
                        }
                    }
                }
            }
            Action::PrDiff if self.view == ViewMode::PullRequests => {
                if let Some(pr) = self.pull_requests.selected() {
                    self.detail_text = crate::github::run(
                        self.repository.root(),
                        &["pr", "diff", &pr.number.to_string(), "--color", "never"],
                    )?;
                    self.detail_scroll = 0;
                    self.detail_column = 0;
                }
            }
            Action::SwitchBranch if self.view == ViewMode::Worktrees => {
                if let Some(w) = self.workspaces.selected() {
                    let path = w.path.clone();
                    self.open_repository(&path)?;
                }
            }
            Action::SwitchBranch if self.view == ViewMode::PullRequests => {
                if let Some(pr) = self.pull_requests.selected() {
                    self.repository.ensure_clean()?;
                    let number = pr.number;
                    self.refresh_after(|app| {
                        crate::github::checkout(app.repository.root(), number)
                    })?;
                    self.notify(format!("Checked out PR #{number}"));
                }
            }
            Action::ScrollLeft => self.detail_column = self.detail_column.saturating_sub(10),
            Action::ScrollRight => {
                self.detail_column = self.detail_column.saturating_add(10).min(
                    self.detail_text
                        .lines()
                        .map(|line| {
                            ratatui::text::Line::from(crate::model::terminal_text(line)).width()
                        })
                        .max()
                        .unwrap_or(0)
                        .min(u16::MAX as usize) as u16,
                )
            }
            Action::PageDown => {
                let target = self.detail_scroll.saturating_add(10);
                let last = self
                    .detail_text
                    .lines()
                    .take(usize::from(target) + 1)
                    .count()
                    .saturating_sub(1);
                self.detail_scroll = target.min(last as u16);
            }
            Action::PageUp => self.detail_scroll = self.detail_scroll.saturating_sub(10),
            Action::Search => {
                self.searching = true;
                self.search.clear();
            }
            Action::NextMatch => self.find_match(true),
            Action::ToggleComparison if self.view == ViewMode::Files => {
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
            Action::ToggleSeen if self.view == ViewMode::Files => {
                if let Some(file) = self.displayed_files().get(self.file_selection) {
                    let key = (self.upstream_comparison, file.path.clone());
                    if self.seen.remove(&key).is_none() {
                        let detail = self.selected_detail()?;
                        self.seen.insert(key, detail);
                    }
                }
            }
            Action::Quit => self.running = false,
            Action::Refresh => {
                self.refresh()?;
                self.notify("Refreshed".to_owned());
            }
            Action::NextView => {
                self.view = self.view.next();
                self.load_extra()?;
                self.refresh_detail();
            }
            Action::NextItem => self.move_selection(1),
            // Navigation must not silently skip a rebase commit in the conflicts view.
            Action::PreviousItem => self.move_selection(-1),
            Action::SwitchBranch if self.view == ViewMode::Branches => {
                if let Some(branch) = self.state.branches.get(self.branch_selection).cloned() {
                    self.run_action(
                        |repo| repo.switch_branch(&branch.name),
                        format!("Switched to {}", branch.name),
                    )?;
                }
            }
            Action::Stage if self.view == ViewMode::Files && !self.upstream_comparison => {
                if let Some(file) = self.state.files.get(self.file_selection).cloned() {
                    self.run_action(|repo| repo.stage(&file), format!("Staged {}", file.label()))?;
                }
            }
            Action::Unstage if self.view == ViewMode::Files && !self.upstream_comparison => {
                if let Some(file) = self.state.files.get(self.file_selection).cloned() {
                    self.run_action(
                        |repo| repo.unstage(&file),
                        format!("Unstaged {}", file.label()),
                    )?;
                }
            }
            Action::Commit => {
                self.start_prompt(WorkingTreePrompt::Commit(false), vec!["Commit message"])
            }
            Action::AmendCommit => {
                self.message =
                    "Amend rewrites the latest commit; enter its replacement message".into();
                self.start_prompt(
                    WorkingTreePrompt::Commit(true),
                    vec!["Amend commit message"],
                );
            }
            Action::Branch => self.start_prompt(
                BranchPrompt::SwitchOrCreate,
                vec!["Create or switch branch"],
            ),
            Action::RenameBranch | Action::MergeBranch | Action::RebaseBranch | Action::Remove
                if self.view == ViewMode::Branches =>
            {
                if let Some(branch) = self.state.branches.get(self.branch_selection) {
                    let name = branch.name.clone();
                    self.message = format!("Selected branch: {name}");
                    match action {
                        Action::RenameBranch => {
                            self.start_prompt(BranchPrompt::Rename(name), vec!["New branch name"])
                        }
                        Action::MergeBranch => self.start_prompt(
                            BranchPrompt::Integrate(name, false),
                            vec!["Type merge to merge into the current branch"],
                        ),
                        Action::RebaseBranch => self.start_prompt(
                            BranchPrompt::Integrate(name, true),
                            vec!["Type rebase to rebase the current branch onto selection"],
                        ),
                        _ => self.start_prompt(
                            BranchPrompt::Delete(name),
                            vec!["Type delete to remove a merged branch"],
                        ),
                    }
                }
            }
            Action::Fetch if self.view == ViewMode::Remotes => {
                if let Some(remote) = self.remotes.selected() {
                    let name = remote.name.clone();
                    self.run_action(|repo| repo.fetch_remote(&name), "Fetched selected remote")?;
                }
            }
            Action::Fetch => self.run_action(Repository::fetch, "Fetched remotes")?,
            Action::Pull => self.run_action(Repository::pull, "Pulled changes")?,
            Action::Push => self.run_action(Repository::push, "Pushed changes")?,
            Action::TakeOurs | Action::TakeTheirs if self.view == ViewMode::Conflicts => {
                if let Some(path) = self
                    .state
                    .merge_state
                    .conflicts
                    .get(self.conflict_selection)
                    .cloned()
                {
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
            Action::MarkResolved if self.view == ViewMode::Conflicts => {
                if let Some(path) = self
                    .state
                    .merge_state
                    .conflicts
                    .get(self.conflict_selection)
                    .cloned()
                {
                    self.run_action(
                        |repo| repo.mark_resolved(&path),
                        format!("Marked {} as resolved", display_path(&path)),
                    )?;
                }
            }
            Action::Continue if self.view == ViewMode::Conflicts => {
                self.run_action(Repository::continue_operation, "Continued Git operation")?
            }
            Action::SkipRebase if self.view == ViewMode::Conflicts => {
                self.run_action(Repository::skip_rebase, "Skipped current rebase commit")?
            }
            Action::Abort if self.view == ViewMode::Conflicts => {
                self.run_action(Repository::abort_operation, "Aborted Git operation")?
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_prompt_and_scroll_actions_stay_on_the_terminal_thread() {
        assert!(Action::Commit.runs_locally(ViewMode::Files));
        assert!(Action::PageDown.runs_locally(ViewMode::History));
        assert!(!Action::SwitchBranch.runs_locally(ViewMode::Branches));
        assert!(!Action::Refresh.runs_locally(ViewMode::Files));
        // These two fetch repository state before their prompt opens.
        assert!(Action::EditRemote.runs_locally(ViewMode::Remotes));
        assert!(!Action::EditRemote.runs_locally(ViewMode::PullRequests));
        assert!(Action::Remove.runs_locally(ViewMode::Branches));
        assert!(!Action::Remove.runs_locally(ViewMode::Files));
    }
}
