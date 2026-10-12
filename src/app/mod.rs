//! Application state: the repository snapshot, one cursor per view, and the detail pane.
//!
//! Key handling, actions, prompts and detail rendering live in the submodules; this file
//! owns the state itself and the refresh cycle every mutation ends with.
mod actions;
mod detail;
mod keys;
mod prompts;

pub use actions::Action;
pub use prompts::{
    BranchPrompt, GitHubPrompt, HistoryPrompt, Prompt, PromptKind, RemotePrompt, RepositoryPrompt,
    StashPrompt, TagPrompt, WorkingTreePrompt,
};

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;

use crate::git::Repository;
use crate::model::{FileEntry, RepoState, ViewMode};

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
    pub reflog: Listed<crate::model::ReflogEntry>,
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
    pub workspaces: Listed<crate::model::Workspace>,
    pub pull_requests: Listed<crate::github::PullRequest>,
    pub pr_filter: crate::github::PrFilter,
    pub review_pr: Option<crate::github::PullRequest>,
    pub pr_files: Listed<crate::pr_review::PrFile>,
    pub prompt: Option<Prompt>,
    pub palette_selection: usize,
    pub github_repositories: Listed<crate::github::GitHubRepo>,
    pub stashes: Listed<crate::model::StashEntry>,
    pub remotes: Listed<crate::model::RemoteEntry>,
    pub remote_branches: Listed<String>,
    pub tracking: crate::model::Tracking,
    pub tags: Listed<crate::model::TagEntry>,
    pub hunks: Listed<crate::patch::Hunk>,
    pub staged_hunks: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileDetail {
    #[default]
    Diff,
    History,
    Blame,
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
            history_limit: 20,
            graph_visible: false,
            history_graph: String::new(),
            reflog: Listed::default(),
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
            workspaces: Listed::default(),
            pull_requests: Listed::default(),
            pr_filter: crate::github::PrFilter::default(),
            review_pr: None,
            pr_files: Listed::default(),
            prompt: None,
            palette_selection: 0,
            github_repositories: Listed::default(),
            stashes: Listed::default(),
            remotes: Listed::default(),
            remote_branches: Listed::default(),
            tracking: crate::model::Tracking::default(),
            tags: Listed::default(),
            hunks: Listed::default(),
            staged_hunks: false,
        };
        app.tracking = app.load_tracking();
        app.refresh_detail();
        let mut navigation = std::mem::take(&mut app.navigation);
        navigation.remember(&app, true);
        app.navigation = navigation;
        Ok(app)
    }

    pub fn refresh(&mut self) -> Result<()> {
        // Keep the previous snapshot intact if reading the repository fails.
        self.state = self
            .repository
            .load_state_with_history(self.history_limit)?;
        self.tracking = self.load_tracking();
        // One upstream diff serves both the comparison list and the Seen-mark check.
        let upstream_files = if self.upstream_comparison {
            self.review_files = self.repository.upstream_files()?;
            self.review_files.clone()
        } else if self.seen.keys().any(|(upstream, _)| *upstream) {
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
            if let Some(file) = files.iter().find(|file| &file.path == path) {
                let detail = if *upstream {
                    self.repository.upstream_detail(file)
                } else {
                    self.repository.file_detail(file)
                };
                if let Ok(detail) = detail
                    && &detail == previous
                {
                    valid.insert((*upstream, path.clone()), detail);
                }
            }
        }
        self.seen = valid;
        if self.browse_tracked {
            self.tracked_files = self.load_tracked_files()?;
        }
        let displayed = self.displayed_files().len();
        clamp_to(&mut self.file_selection, displayed);
        clamp_to(&mut self.history_selection, self.state.commits.len());
        clamp_to(&mut self.branch_selection, self.state.branches.len());
        clamp_to(
            &mut self.conflict_selection,
            self.state.merge_state.conflicts.len(),
        );
        self.load_view(true)?;
        self.refresh_detail();
        Ok(())
    }

    /// Load the list behind the current view, keeping its cursor inside the new bounds.
    pub(super) fn load_extra(&mut self) -> Result<()> {
        self.load_view(false)
    }

    /// `state_is_fresh` skips data `refresh` has just read, so a refresh runs each
    /// Git command once.
    fn load_view(&mut self, state_is_fresh: bool) -> Result<()> {
        match self.view {
            ViewMode::Reflog => self.reflog.items = self.repository.reflog(self.reflog_limit)?,
            ViewMode::History => {
                if !state_is_fresh {
                    self.state.commits = self.repository.history(self.history_limit)?;
                }
                if self.graph_visible {
                    self.history_graph = self.repository.history_graph(self.history_limit)?;
                }
            }
            ViewMode::Hunks => {
                self.hunks.items = if self.browse_tracked || self.upstream_comparison {
                    Vec::new()
                } else if let Some(file) = self.state.files.get(self.file_selection) {
                    self.repository.hunks(file, self.staged_hunks)?
                } else {
                    Vec::new()
                };
            }
            ViewMode::Tags => self.tags.items = self.repository.tags()?,
            ViewMode::Remotes => self.remotes.items = self.repository.remotes()?,
            ViewMode::RemoteBranches => {
                self.remote_branches.items = self.repository.remote_branches()?
            }
            ViewMode::Stashes => self.stashes.items = self.repository.stashes()?,
            ViewMode::GitHubRepositories => {
                self.github_repositories.items =
                    crate::github::repositories(self.repository.root())?;
            }
            ViewMode::Workspaces => self.workspaces.items = self.repository.workspaces()?,
            ViewMode::PrFiles => {
                self.pr_files.items.clear();
                if let Some(pr) = &self.review_pr {
                    self.pr_files.items =
                        crate::pr_review::reviewed_files(self.repository.root(), pr)?;
                }
            }
            ViewMode::PullRequests => {
                let selected = self
                    .pull_requests
                    .items
                    .get(self.pull_requests.selection)
                    .map(|pr| pr.number);
                self.pull_requests.items =
                    crate::github::list_filtered(self.repository.root(), &self.pr_filter)?;
                if let Some(index) = selected
                    .and_then(|n| self.pull_requests.items.iter().position(|p| p.number == n))
                {
                    self.pull_requests.selection = index;
                }
            }
            ViewMode::Files | ViewMode::Branches | ViewMode::Conflicts => {}
            ViewMode::RecentRepositories => {}
        }
        let (selection, len) = self.cursor();
        clamp_to(selection, len);
        Ok(())
    }

    pub fn selection(&self) -> usize {
        match self.view {
            ViewMode::RecentRepositories => self.recent_selection,
            ViewMode::PrFiles => self.pr_files.selection,
            ViewMode::Reflog => self.reflog.selection,
            ViewMode::Hunks => self.hunks.selection,
            ViewMode::Tags => self.tags.selection,
            ViewMode::Remotes => self.remotes.selection,
            ViewMode::RemoteBranches => self.remote_branches.selection,
            ViewMode::Stashes => self.stashes.selection,
            ViewMode::Files => self.file_selection,
            ViewMode::History => self.history_selection,
            ViewMode::Branches => self.branch_selection,
            ViewMode::Conflicts => self.conflict_selection,
            ViewMode::GitHubRepositories => self.github_repositories.selection,
            ViewMode::Workspaces => self.workspaces.selection,
            ViewMode::PullRequests => self.pull_requests.selection,
        }
    }

    pub(super) fn move_selection(&mut self, delta: isize) {
        let (selection, len) = self.cursor();
        *selection = selection.saturating_add_signed(delta);
        clamp_to(selection, len);
        self.refresh_detail();
    }

    /// The current view's cursor and the length of the list it points into.
    fn cursor(&mut self) -> (&mut usize, usize) {
        let file_len = self.displayed_files().len();
        match self.view {
            ViewMode::RecentRepositories => (
                &mut self.recent_selection,
                self.navigation.repositories.len(),
            ),
            ViewMode::Files => (&mut self.file_selection, file_len),
            ViewMode::History => (&mut self.history_selection, self.state.commits.len()),
            ViewMode::Branches => (&mut self.branch_selection, self.state.branches.len()),
            ViewMode::Conflicts => (
                &mut self.conflict_selection,
                self.state.merge_state.conflicts.len(),
            ),
            ViewMode::PrFiles => self.pr_files.cursor(),
            ViewMode::Reflog => self.reflog.cursor(),
            ViewMode::Hunks => self.hunks.cursor(),
            ViewMode::Tags => self.tags.cursor(),
            ViewMode::Remotes => self.remotes.cursor(),
            ViewMode::RemoteBranches => self.remote_branches.cursor(),
            ViewMode::Stashes => self.stashes.cursor(),
            ViewMode::GitHubRepositories => self.github_repositories.cursor(),
            ViewMode::Workspaces => self.workspaces.cursor(),
            ViewMode::PullRequests => self.pull_requests.cursor(),
        }
    }

    pub(super) fn refresh_detail(&mut self) {
        self.detail_scroll = 0;
        self.detail_column = 0;
        self.detail_text = self
            .selected_detail()
            .unwrap_or_else(|error| format!("Cannot load details:\n{error:#}"));
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

    pub(super) fn load_tracked_files(&self) -> Result<Vec<FileEntry>> {
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

    pub(super) fn open_repository(&mut self, path: &std::path::Path) -> Result<()> {
        // Construct first: a failed open leaves the existing repository intact.
        let mut replacement = Self::new(Repository::open_with(path, self.repository.runner())?)?;
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

    /// Run a repository operation and report its success in the action line.
    pub(super) fn run_action<F>(&mut self, action: F, success: impl Into<String>) -> Result<()>
    where
        F: FnOnce(&Repository) -> Result<()>,
    {
        let success = success.into();
        match self.reload_after(|app| action(&app.repository)) {
            Ok(()) => {
                self.notify(success);
                Ok(())
            }
            Err(Failure::Refresh(error)) => {
                Err(error.context(format!("{success}, but refreshing the repository failed")))
            }
            Err(Failure::Operation(error)) => Err(error),
        }
    }

    /// Run an operation, then reload: a failed pull, rebase or hook can still have
    /// changed files or the index, so the snapshot is refreshed either way.
    pub(super) fn refresh_after<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        self.reload_after(operation).map_err(anyhow::Error::from)
    }

    fn reload_after<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T>,
    ) -> std::result::Result<T, Failure> {
        let result = operation(self);
        let refreshed = self.refresh();
        match (result, refreshed) {
            (Ok(value), Ok(())) => Ok(value),
            (Ok(_), Err(error)) => Err(Failure::Refresh(error)),
            (Err(error), Ok(())) => Err(Failure::Operation(error)),
            (Err(error), Err(refresh_error)) => Err(Failure::Operation(
                error.context(format!("Repository refresh also failed: {refresh_error:#}")),
            )),
        }
    }

    /// Show a successful outcome in the action line.
    pub(super) fn notify(&mut self, message: impl Into<String>) {
        self.message = message.into();
        self.message_is_error = false;
    }

    /// Git and GitHub failures are recoverable: show them instead of leaving raw mode.
    pub(super) fn report_error(&mut self, error: anyhow::Error) {
        self.detail_scroll = 0;
        self.detail_column = 0;
        self.message = format!("{error:#}");
        self.detail_text = format!("Action failed\n\n{}", self.message);
        self.message_is_error = true;
    }

    /// Tracking problems are shown in the header rather than failing the whole refresh.
    fn load_tracking(&self) -> crate::model::Tracking {
        self.repository
            .tracking()
            .unwrap_or_else(|error| crate::model::Tracking {
                error: Some(format!("{error:#}")),
                ..Default::default()
            })
    }
}

/// Which half of an operation-then-reload sequence failed.
enum Failure {
    Operation(anyhow::Error),
    Refresh(anyhow::Error),
}

impl From<Failure> for anyhow::Error {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::Operation(error) | Failure::Refresh(error) => error,
        }
    }
}

/// A loaded list and the cursor into it, kept together so they cannot drift apart.
#[derive(Debug, Clone)]
pub struct Listed<T> {
    pub items: Vec<T>,
    pub selection: usize,
}

impl<T> Default for Listed<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            selection: 0,
        }
    }
}

impl<T> Listed<T> {
    /// The entry under the cursor, if the list is not empty.
    pub fn selected(&self) -> Option<&T> {
        self.items.get(self.selection)
    }

    /// The cursor and the length it must stay below.
    fn cursor(&mut self) -> (&mut usize, usize) {
        (&mut self.selection, self.items.len())
    }
}

/// Keep a list cursor inside the list after it was reloaded or shortened.
pub(super) fn clamp_to(selection: &mut usize, len: usize) {
    *selection = (*selection).min(len.saturating_sub(1));
}
