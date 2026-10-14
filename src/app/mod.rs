//! UI-independent application state. Key bindings live in `input`, rendering in `ui`.
mod actions;
mod detail;
mod prompt;
mod selection;

pub use prompt::{Prompt, PromptKind};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::git::{HISTORY_PAGE, Repository};
use crate::model::{FileEntry, RepoState, ViewMode};

/// UI-independent commands; terminal key bindings live in `input`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileDetail {
    #[default]
    Diff,
    History,
    Blame,
}

/// Largest accepted size of one typed or pasted input field.
const INPUT_LIMIT: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct App {
    pub navigation: crate::settings::Navigation,
    pub recent_selection: usize,
    pub persistence_error: Option<String>,
    pub repository: Repository,
    pub state: RepoState,
    pub view: ViewMode,
    pub file_selection: usize,
    pub history_selection: usize,
    pub history_limit: usize,
    pub graph_visible: bool,
    pub history_graph: String,
    pub reflog: Vec<crate::model::ReflogEntry>,
    pub reflog_selection: usize,
    pub reflog_limit: usize,
    pub branch_selection: usize,
    pub conflict_selection: usize,
    pub browse_tracked: bool,
    pub tracked_files: Vec<FileEntry>,
    pub file_detail: FileDetail,
    pub detail_text: String,
    pub message: String,
    pub message_is_error: bool,
    pub running: bool,
    pub detail_scroll: u16,
    pub detail_column: u16,
    pub upstream_comparison: bool,
    pub review_files: Vec<FileEntry>,
    pub seen: HashMap<(bool, PathBuf), String>,
    pub search: String,
    pub searching: bool,
    pub workspaces: Vec<crate::model::Workspace>,
    pub pull_requests: Vec<crate::github::PullRequest>,
    pub workspace_selection: usize,
    pub pr_selection: usize,
    pub pr_filter: crate::github::PrFilter,
    pub review_pr: Option<crate::github::PullRequest>,
    pub pr_files: Vec<crate::pr_review::PrFile>,
    pub pr_file_selection: usize,
    pub prompt: Option<Prompt>,
    pub palette_selection: usize,
    pub github_repositories: Vec<crate::github::GitHubRepo>,
    pub github_repo_selection: usize,
    pub stashes: Vec<crate::model::StashEntry>,
    pub stash_selection: usize,
    pub remotes: Vec<crate::model::RemoteEntry>,
    pub remote_branches: Vec<String>,
    pub remote_selection: usize,
    pub remote_branch_selection: usize,
    pub tracking: crate::model::Tracking,
    pub tags: Vec<crate::model::TagEntry>,
    pub tag_selection: usize,
    pub hunks: Vec<crate::patch::Hunk>,
    pub hunk_selection: usize,
    pub staged_hunks: bool,
}

/// Keep a list cursor inside the list after its contents changed.
fn clamp(selection: &mut usize, len: usize) {
    *selection = (*selection).min(len.saturating_sub(1));
}

impl App {
    pub fn capture_navigation(&mut self) {
        if self.view == ViewMode::RecentRepositories {
            return;
        }
        let mut navigation = std::mem::take(&mut self.navigation);
        navigation.remember(self, false);
        self.navigation = navigation;
    }

    pub fn restore_navigation(&mut self, mut navigation: crate::settings::Navigation) {
        navigation.restore(self);
        navigation.remember(self, true);
        self.navigation = navigation;
    }

    pub fn new(repository: Repository) -> Result<Self> {
        let state = repository.load_state()?;
        let mut app = Self {
            navigation: crate::settings::Navigation::default(),
            recent_selection: 0,
            persistence_error: None,
            repository,
            state,
            view: ViewMode::Files,
            file_selection: 0,
            history_selection: 0,
            history_limit: HISTORY_PAGE,
            graph_visible: false,
            history_graph: String::new(),
            reflog: Vec::new(),
            reflog_selection: 0,
            reflog_limit: 100,
            branch_selection: 0,
            conflict_selection: 0,
            browse_tracked: false,
            tracked_files: Vec::new(),
            file_detail: FileDetail::Diff,
            detail_text: String::new(),
            message: "Ready".to_owned(),
            message_is_error: false,
            running: true,
            detail_scroll: 0,
            detail_column: 0,
            upstream_comparison: false,
            review_files: Vec::new(),
            seen: HashMap::new(),
            search: String::new(),
            searching: false,
            workspaces: Vec::new(),
            pull_requests: Vec::new(),
            workspace_selection: 0,
            pr_selection: 0,
            pr_filter: crate::github::PrFilter::default(),
            review_pr: None,
            pr_files: Vec::new(),
            pr_file_selection: 0,
            prompt: None,
            palette_selection: 0,
            github_repositories: Vec::new(),
            github_repo_selection: 0,
            stashes: Vec::new(),
            stash_selection: 0,
            remotes: Vec::new(),
            remote_branches: Vec::new(),
            remote_selection: 0,
            remote_branch_selection: 0,
            tracking: crate::model::Tracking::default(),
            tags: Vec::new(),
            tag_selection: 0,
            hunks: Vec::new(),
            hunk_selection: 0,
            staged_hunks: false,
        };
        app.load_tracking();
        app.refresh_detail();
        let mut navigation = std::mem::take(&mut app.navigation);
        navigation.remember(&app, true);
        app.navigation = navigation;
        Ok(app)
    }

    /// A broken upstream is a visible header message, never a reason to hide the repository.
    fn load_tracking(&mut self) {
        self.tracking = self
            .repository
            .tracking()
            .unwrap_or_else(|error| crate::model::Tracking {
                error: Some(format!("{error:#}")),
                ..Default::default()
            });
    }

    pub fn refresh(&mut self) -> Result<()> {
        // Publish one complete snapshot; failure must not expose partially refreshed lists.
        let mut updated = self.clone();
        updated.refresh_snapshot()?;
        *self = updated;
        Ok(())
    }

    fn refresh_snapshot(&mut self) -> Result<()> {
        let selected = self.selection_key(self.view, self.selection());
        let file = self.selection_key(ViewMode::Files, self.file_selection);
        let history = self.selection_key(ViewMode::History, self.history_selection);
        let branch = self.selection_key(ViewMode::Branches, self.branch_selection);
        let viewport = (self.detail_scroll, self.detail_column);
        let previous_detail = self.detail_text.clone();
        self.state = self
            .repository
            .load_state_with_history(self.history_limit)?;
        self.load_tracking();
        if self.upstream_comparison {
            self.review_files = self.repository.upstream_files()?;
        }
        self.revalidate_seen();
        if self.browse_tracked {
            self.tracked_files = self.load_tracked_files()?;
        }
        self.restore_selection(ViewMode::Files, file.as_ref());
        self.restore_selection(ViewMode::History, history.as_ref());
        self.restore_selection(ViewMode::Branches, branch.as_ref());
        let files = self.displayed_files().len();
        clamp(&mut self.file_selection, files);
        clamp(&mut self.branch_selection, self.state.branches.len());
        clamp(
            &mut self.conflict_selection,
            self.state.merge_state.conflicts.len(),
        );
        self.load_view_data(false)?;
        self.restore_selection(self.view, selected.as_ref());
        self.clamp_selection();
        self.refresh_detail();
        if selected.is_some()
            && self.selection_key(self.view, self.selection()) == selected
            && self.detail_text == previous_detail
        {
            (self.detail_scroll, self.detail_column) = viewport;
        }
        Ok(())
    }

    /// Seen marks are content snapshots: a file whose diff changed is no longer seen.
    fn revalidate_seen(&mut self) {
        let upstream_files = if self.seen.keys().any(|(upstream, _)| *upstream) {
            self.repository.upstream_files().unwrap_or_default()
        } else {
            Vec::new()
        };
        let mut valid = HashMap::new();
        for ((upstream, path), previous) in &self.seen {
            let files = if *upstream {
                &upstream_files
            } else {
                &self.state.files
            };
            let Some(file) = files.iter().find(|file| &file.path == path) else {
                continue;
            };
            let detail = if *upstream {
                self.repository.upstream_detail(file)
            } else {
                self.repository.file_detail(file)
            };
            if detail.as_ref().is_ok_and(|detail| detail == previous) {
                valid.insert((*upstream, path.clone()), previous.clone());
            }
        }
        self.seen = valid;
    }

    /// Load data only the current view needs. `refresh` already holds fresh history.
    fn load_view_data(&mut self, reload_history: bool) -> Result<()> {
        let selected = self.selection_key(self.view, self.selection());
        match self.view {
            ViewMode::Reflog => self.reflog = self.repository.reflog(self.reflog_limit)?,
            ViewMode::History => {
                if reload_history {
                    self.state.commits = self.repository.history(self.history_limit)?;
                }
                if self.graph_visible {
                    self.history_graph = self.repository.history_graph(self.history_limit)?;
                }
            }
            ViewMode::Hunks => {
                self.hunks = if self.browse_tracked || self.upstream_comparison {
                    Vec::new()
                } else if let Some(file) = self.state.files.get(self.file_selection) {
                    self.repository.hunks(file, self.staged_hunks)?
                } else {
                    Vec::new()
                };
            }
            ViewMode::Tags => self.tags = self.repository.tags()?,
            ViewMode::Remotes => self.remotes = self.repository.remotes()?,
            ViewMode::RemoteBranches => self.remote_branches = self.repository.remote_branches()?,
            ViewMode::Stashes => self.stashes = self.repository.stashes()?,
            ViewMode::GitHubRepositories => {
                self.github_repositories = crate::github::repositories(self.repository.root())?;
            }
            ViewMode::Workspaces => self.workspaces = self.repository.workspaces()?,
            ViewMode::PrFiles => {
                self.pr_files.clear();
                if let Some(pr) = &self.review_pr {
                    self.pr_files = crate::pr_review::reviewed_files(self.repository.root(), pr)?;
                }
            }
            ViewMode::PullRequests => {
                let selected = self
                    .pull_requests
                    .get(self.pr_selection)
                    .map(|pr| pr.number);
                self.pull_requests =
                    crate::github::list_filtered(self.repository.root(), &self.pr_filter)?;
                if let Some(index) =
                    selected.and_then(|n| self.pull_requests.iter().position(|p| p.number == n))
                {
                    self.pr_selection = index;
                }
            }
            ViewMode::Files | ViewMode::Branches | ViewMode::Conflicts => {}
            ViewMode::RecentRepositories => {}
        }
        self.restore_selection(self.view, selected.as_ref());
        self.clamp_selection();
        Ok(())
    }

    /// Switch views and load what the new view shows.
    fn enter_view(&mut self, view: ViewMode) -> Result<()> {
        self.view = view;
        self.load_view_data(true)?;
        self.refresh_detail();
        Ok(())
    }

    pub fn selection(&self) -> usize {
        match self.view {
            ViewMode::RecentRepositories => self.recent_selection,
            ViewMode::PrFiles => self.pr_file_selection,
            ViewMode::Reflog => self.reflog_selection,
            ViewMode::Hunks => self.hunk_selection,
            ViewMode::Tags => self.tag_selection,
            ViewMode::Remotes => self.remote_selection,
            ViewMode::RemoteBranches => self.remote_branch_selection,
            ViewMode::Stashes => self.stash_selection,
            ViewMode::Files => self.file_selection,
            ViewMode::History => self.history_selection,
            ViewMode::Branches => self.branch_selection,
            ViewMode::Conflicts => self.conflict_selection,
            ViewMode::GitHubRepositories => self.github_repo_selection,
            ViewMode::Workspaces => self.workspace_selection,
            ViewMode::PullRequests => self.pr_selection,
        }
    }

    fn selection_mut(&mut self) -> &mut usize {
        self.selection_mut_for(self.view)
    }

    fn selection_mut_for(&mut self, view: ViewMode) -> &mut usize {
        match view {
            ViewMode::RecentRepositories => &mut self.recent_selection,
            ViewMode::PrFiles => &mut self.pr_file_selection,
            ViewMode::Reflog => &mut self.reflog_selection,
            ViewMode::Hunks => &mut self.hunk_selection,
            ViewMode::Tags => &mut self.tag_selection,
            ViewMode::Remotes => &mut self.remote_selection,
            ViewMode::RemoteBranches => &mut self.remote_branch_selection,
            ViewMode::Stashes => &mut self.stash_selection,
            ViewMode::Files => &mut self.file_selection,
            ViewMode::History => &mut self.history_selection,
            ViewMode::Branches => &mut self.branch_selection,
            ViewMode::Conflicts => &mut self.conflict_selection,
            ViewMode::GitHubRepositories => &mut self.github_repo_selection,
            ViewMode::Workspaces => &mut self.workspace_selection,
            ViewMode::PullRequests => &mut self.pr_selection,
        }
    }

    /// Number of entries in the current view's list.
    pub fn list_len(&self) -> usize {
        match self.view {
            ViewMode::RecentRepositories => self.navigation.repositories.len(),
            ViewMode::PrFiles => self.pr_files.len(),
            ViewMode::Reflog => self.reflog.len(),
            ViewMode::Hunks => self.hunks.len(),
            ViewMode::Tags => self.tags.len(),
            ViewMode::Remotes => self.remotes.len(),
            ViewMode::RemoteBranches => self.remote_branches.len(),
            ViewMode::Stashes => self.stashes.len(),
            ViewMode::Files => self.displayed_files().len(),
            ViewMode::History => self.state.commits.len(),
            ViewMode::Branches => self.state.branches.len(),
            ViewMode::Conflicts => self.state.merge_state.conflicts.len(),
            ViewMode::GitHubRepositories => self.github_repositories.len(),
            ViewMode::Workspaces => self.workspaces.len(),
            ViewMode::PullRequests => self.pull_requests.len(),
        }
    }

    fn clamp_selection(&mut self) {
        let len = self.list_len();
        clamp(self.selection_mut(), len);
    }

    fn move_selection(&mut self, delta: isize) {
        let selection = self.selection_mut();
        *selection = selection.saturating_add_signed(delta);
        self.clamp_selection();
        self.refresh_detail();
    }

    /// Replace the detail pane and start reading it from the top-left corner.
    fn set_detail(&mut self, text: String) {
        self.detail_text = text;
        self.detail_scroll = 0;
        self.detail_column = 0;
    }

    fn refresh_detail(&mut self) {
        let detail = self
            .selected_detail()
            .unwrap_or_else(|error| format!("Cannot load details:\n{error:#}"));
        self.set_detail(detail);
        if self.view == ViewMode::History
            && self.graph_visible
            && let Some(line) = self
                .detail_text
                .lines()
                .position(|line| line.starts_with("> "))
        {
            self.detail_scroll = line.saturating_sub(3).min(u16::MAX as usize) as u16;
        }
    }

    fn load_tracked_files(&self) -> Result<Vec<FileEntry>> {
        Ok(self
            .repository
            .tracked_files()?
            .into_iter()
            .map(|file| {
                self.state
                    .files
                    .iter()
                    .find(|changed| changed.path == file.path)
                    .cloned()
                    .unwrap_or(file)
            })
            .collect())
    }

    pub fn displayed_files(&self) -> &[FileEntry] {
        if self.browse_tracked {
            &self.tracked_files
        } else if self.upstream_comparison {
            &self.review_files
        } else {
            &self.state.files
        }
    }

    pub fn file_seen(&self, file: &FileEntry) -> bool {
        self.seen
            .contains_key(&(self.upstream_comparison, file.path.clone()))
    }

    /// Record a successful outcome in the action line.
    fn notify(&mut self, message: impl Into<String>) {
        self.message = message.into();
        self.message_is_error = false;
    }

    /// Git failures are recoverable UI messages, not reasons to leave the terminal in raw mode.
    fn fail(&mut self, error: anyhow::Error) {
        self.message = format!("{error:#}");
        self.set_detail(format!("Action failed\n\n{}", self.message));
        self.message_is_error = true;
    }

    pub fn handle_paste(&mut self, text: &str) {
        self.palette_selection = 0;
        let target = if let Some(prompt) = &mut self.prompt {
            Some(&mut prompt.text)
        } else if self.searching {
            Some(&mut self.search)
        } else {
            None
        };
        if let Some(target) = target {
            if target.len().saturating_add(text.len()) > INPUT_LIMIT {
                self.message = "Input exceeds 64 KiB; clear or shorten the field".into();
                self.message_is_error = true;
                return;
            }
            target.push_str(text);
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        if self.prompt.is_some() {
            self.handle_prompt_key(key);
        } else if self.searching
            && !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            match key.code {
                KeyCode::Esc => self.searching = false,
                KeyCode::Enter => {
                    self.searching = false;
                    self.find_match(false);
                }
                KeyCode::Backspace => {
                    self.search.pop();
                }
                KeyCode::Char(c) => self.search.push(c),
                _ => {}
            }
        } else if let Some(action) = crate::input::action_for_key(key) {
            self.handle(action);
        }
    }

    /// Ordinary shortcut letters are text while a form is open; only control keys act.
    fn handle_prompt_key(&mut self, key: KeyEvent) {
        if self
            .prompt
            .as_ref()
            .is_some_and(|p| matches!(p.kind, PromptKind::CommandPalette))
        {
            let count =
                crate::commands::filtered(self.view, &self.prompt.as_ref().unwrap().text).len();
            match key.code {
                KeyCode::Up => {
                    self.palette_selection = self.palette_selection.saturating_sub(1);
                    return;
                }
                KeyCode::Down => {
                    self.palette_selection = self
                        .palette_selection
                        .saturating_add(1)
                        .min(count.saturating_sub(1));
                    return;
                }
                KeyCode::Char(_) | KeyCode::Backspace => self.palette_selection = 0,
                _ => {}
            }
        }
        let control = key.modifiers == KeyModifiers::CONTROL;
        let prompt = self.prompt.as_mut().unwrap();
        match key.code {
            KeyCode::Char('c') if control => self.running = false,
            KeyCode::Char('u') if control => prompt.text.clear(),
            KeyCode::Enter if key.modifiers == KeyModifiers::ALT => prompt.text.push('\n'),
            _ if key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {}
            KeyCode::Esc => self.prompt = None,
            KeyCode::Char(c) => prompt.text.push(c),
            KeyCode::Backspace => {
                prompt.text.pop();
            }
            KeyCode::Enter if prompt.advance() => {
                let prompt = self.prompt.take().unwrap();
                if let Err(error) = self.submit_prompt(prompt) {
                    self.fail(error);
                }
            }
            _ => {}
        }
    }

    fn open_repository(&mut self, path: &Path) -> Result<()> {
        // Construct first: a failed open leaves the existing repository intact.
        let mut replacement = Self::new(Repository::open(path)?)?;
        let mut navigation = self.navigation.clone();
        navigation.remember(self, false);
        navigation.restore(&mut replacement);
        navigation.remember(&replacement, true);
        replacement.navigation = navigation;
        replacement.persistence_error = self.persistence_error.clone();
        replacement.handle(Action::Refresh);
        if !replacement.message_is_error {
            replacement.message = "Opened repository".into();
        }
        *self = replacement;
        Ok(())
    }

    pub fn handle(&mut self, action: Action) {
        if let Err(error) = self.apply(action) {
            self.fail(error);
        }
    }

    /// Run a mutation, then reload unconditionally: a failed pull, rebase or hook can still
    /// change files, the index or the operation state before returning an error.
    fn run_action<F>(&mut self, action: F, success: impl Into<String>) -> Result<()>
    where
        F: FnOnce(&Repository) -> Result<()>,
    {
        let result = action(&self.repository).map(|()| success.into());
        self.finish(result)
    }

    /// Reload after an operation and report it. The operation result is preserved even when
    /// the reload fails, so a successful push is never reported as a failure of the push.
    fn finish(&mut self, result: Result<String>) -> Result<()> {
        let refreshed = self.refresh();
        match (result, refreshed) {
            (Ok(success), Ok(())) => {
                self.notify(success);
                Ok(())
            }
            (Ok(success), Err(refresh_error)) => {
                Err(refresh_error
                    .context(format!("{success}, but refreshing the repository failed")))
            }
            (Err(error), Ok(())) => Err(error),
            (Err(error), Err(refresh_error)) => {
                Err(error.context(format!("Repository refresh also failed: {refresh_error:#}")))
            }
        }
    }
}
