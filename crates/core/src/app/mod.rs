//! Application state: the repository snapshot, one cursor per view, and the detail pane.
//!
//! Key handling, actions, prompts and detail rendering live in the submodules; this file
//! owns the state itself and the refresh cycle every mutation ends with.
mod access;
mod actions;
mod detail;
pub mod editors;
#[cfg(test)]
mod github_tests;
mod keys;
mod prompts;
mod selection;

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
    pub(crate) navigation: crate::settings::Navigation,
    pub(crate) recent_selection: usize,
    pub(crate) persistence_error: Option<String>,
    pub(crate) repository: Repository,
    pub(crate) state: RepoState,
    pub(crate) view: ViewMode,
    pub(crate) file_selection: usize,
    pub(crate) history_selection: usize,
    pub(crate) history_limit: usize,
    pub(crate) graph_visible: bool,
    pub(crate) history_graph: String,
    pub(crate) reflog: Listed<crate::model::ReflogEntry>,
    pub(crate) reflog_limit: usize,
    pub(crate) branch_selection: usize,
    pub(crate) conflict_selection: usize,
    pub(crate) browse_tracked: bool,
    pub(crate) tracked_files: Vec<FileEntry>,
    pub(crate) file_detail: FileDetail,
    pub(crate) detail_text: String,
    pub(crate) message: String,
    pub(crate) message_is_error: bool,
    pub(crate) running: bool,
    pub(crate) detail_scroll: u16,
    pub(crate) detail_column: u16,
    pub(crate) upstream_comparison: bool,
    pub(crate) review_files: Vec<FileEntry>,
    pub(crate) seen: HashMap<(bool, PathBuf), String>,
    /// Repository digest taken just before the last reload; see [`App::changed_externally`].
    pub(crate) fingerprint: Option<u64>,
    pub(crate) search: String,
    pub(crate) searching: bool,
    pub(crate) workspaces: Listed<crate::model::Workspace>,
    pub(crate) pull_requests: Listed<crate::github::PullRequest>,
    pub(crate) pr_filter: crate::github::PrFilter,
    pub(crate) review_pr: Option<crate::github::PullRequest>,
    pub(crate) pr_files: Listed<crate::pr_review::PrFile>,
    pub(crate) prompt: Option<Prompt>,
    pub(crate) palette_selection: usize,
    pub(crate) github_repositories: Listed<crate::github::GitHubRepo>,
    pub(crate) stashes: Listed<crate::model::StashEntry>,
    pub(crate) remotes: Listed<crate::model::RemoteEntry>,
    pub(crate) remote_branches: Listed<String>,
    pub(crate) tracking: crate::model::Tracking,
    pub(crate) tags: Listed<crate::model::TagEntry>,
    pub(crate) hunks: Listed<crate::patch::Hunk>,
    pub(crate) staged_hunks: bool,
    pub(crate) file_filter: String,
    pub(crate) branch_filter: String,
    pub(crate) line_editor: Option<editors::LineEditor>,
    pub(crate) rebase_editor: Option<editors::RebaseEditor>,
    pub(crate) stash_files: std::collections::HashSet<PathBuf>,
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
        let current = self.repository_settings();
        self.navigation.remember(current, false);
    }

    pub fn restore_navigation(&mut self, mut navigation: crate::settings::Navigation) {
        self.apply_saved(&navigation);
        navigation.remember(self.repository_settings(), true);
        self.navigation = navigation;
    }

    /// What this client shows for its working tree, in the persisted form.
    fn repository_settings(&self) -> crate::settings::RepositorySettings {
        crate::settings::RepositorySettings::capture(
            self.repository.root(),
            self.view,
            self.history_limit,
            self.graph_visible,
            &self.pr_filter,
        )
    }

    /// Reopen the working tree where the user left it, if it was opened before.
    fn apply_saved(&mut self, navigation: &crate::settings::Navigation) {
        if let Some(saved) = navigation.saved(self.repository.root()) {
            self.view = saved.view;
            self.history_limit = saved.history_limit;
            self.graph_visible = saved.graph_visible;
            self.pr_filter = saved.pr_filter.clone();
            self.detail_text.clear();
        }
    }

    pub fn new(repository: Repository) -> Result<Self> {
        // Taken before loading: a change racing the load is caught by the next poll.
        let fingerprint = repository.fingerprint().ok();
        let state = repository.load_state()?;
        let mut app = Self {
            fingerprint,
            navigation: crate::settings::Navigation::default(),
            recent_selection: 0,
            persistence_error: None,
            repository,
            state,
            view: ViewMode::Files,
            file_selection: 0,
            history_selection: 0,
            history_limit: crate::git::HISTORY_PAGE,
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
            file_filter: String::new(),
            branch_filter: String::new(),
            line_editor: None,
            rebase_editor: None,
            stash_files: Default::default(),
        };
        app.tracking = app.load_tracking();
        app.refresh_detail();
        let current = app.repository_settings();
        app.navigation.remember(current, true);
        Ok(app)
    }

    pub fn refresh(&mut self) -> Result<()> {
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
        self.fingerprint = self.repository.fingerprint().ok();
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
        self.stash_files
            .retain(|path| self.state.files.iter().any(|f| &f.path == path));
        if self.browse_tracked {
            self.tracked_files = self.load_tracked_files()?;
        }
        self.restore_selection(ViewMode::Files, file.as_ref());
        self.restore_selection(ViewMode::History, history.as_ref());
        self.restore_selection(ViewMode::Branches, branch.as_ref());
        let displayed = self.displayed_files().len();
        clamp_to(&mut self.file_selection, displayed);
        clamp_to(&mut self.history_selection, self.state.commits.len());
        clamp_to(&mut self.branch_selection, self.state.branches.len());
        clamp_to(
            &mut self.conflict_selection,
            self.state.merge_state.conflicts.len(),
        );
        self.load_view(true)?;
        self.restore_selection(self.view, selected.as_ref());
        let (selection, len) = self.cursor();
        clamp_to(selection, len);
        if !self.name_filter().is_empty()
            && !self.visible_indices().contains(&self.selection())
            && let Some(index) = self.visible_indices().first().copied()
        {
            self.select(index);
        }
        self.refresh_detail();
        if selected.is_some()
            && self.selection_key(self.view, self.selection()) == selected
            && self.detail_text == previous_detail
        {
            (self.detail_scroll, self.detail_column) = viewport;
        }
        Ok(())
    }

    /// Load the list behind the current view, keeping its cursor inside the new bounds.
    pub(super) fn load_extra(&mut self) -> Result<()> {
        self.load_view(false)
    }

    /// `state_is_fresh` skips data `refresh` has just read, so a refresh runs each
    /// Git command once.
    fn load_view(&mut self, state_is_fresh: bool) -> Result<()> {
        let selected = self.selection_key(self.view, self.selection());
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
            ViewMode::Worktrees => self.workspaces.items = self.repository.workspaces()?,
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
        self.restore_selection(self.view, selected.as_ref());
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
            ViewMode::Worktrees => self.workspaces.selection,
            ViewMode::PullRequests => self.pull_requests.selection,
        }
    }

    pub(super) fn move_selection(&mut self, delta: isize) {
        if !self.name_filter().is_empty() {
            let visible = self.visible_indices();
            let current = visible
                .iter()
                .position(|i| *i == self.selection())
                .unwrap_or(0);
            if let Some(index) = visible.get(
                current
                    .saturating_add_signed(delta)
                    .min(visible.len().saturating_sub(1)),
            ) {
                self.select(*index);
            }
            return;
        }
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
            ViewMode::Worktrees => self.workspaces.cursor(),
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

    pub fn visible_indices(&self) -> Vec<usize> {
        if self.name_filter().is_empty() {
            return match self.view {
                ViewMode::Files => (0..self.displayed_files().len()).collect(),
                ViewMode::Branches => (0..self.state.branches.len()).collect(),
                _ => Vec::new(),
            };
        }
        let matches = |name: &str| name.to_lowercase().contains(self.name_filter());
        match self.view {
            ViewMode::Files => self
                .displayed_files()
                .iter()
                .enumerate()
                .filter(|(_, f)| matches(&f.label()))
                .map(|(i, _)| i)
                .collect(),
            ViewMode::Branches => self
                .state
                .branches
                .iter()
                .enumerate()
                .filter(|(_, b)| matches(&b.name))
                .map(|(i, _)| i)
                .collect(),
            _ => Vec::new(),
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
        navigation.remember(self.repository_settings(), false);
        replacement.apply_saved(&navigation);
        navigation.remember(replacement.repository_settings(), true);
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
