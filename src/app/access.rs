//! The public surface of `App`. Fields are crate-private; other crates (the binary,
//! integration tests) read state through these accessors and change it only through
//! actions, key events and the few commands below.
use super::{App, FileDetail, Listed, Prompt};
use crate::git::Repository;
use crate::github::{PrFilter, PullRequest};
use crate::model::{
    ReflogEntry, RemoteEntry, RepoState, StashEntry, TagEntry, Tracking, ViewMode, Workspace,
};
use crate::patch::Hunk;
use crate::pr_review::PrFile;
use crate::settings::{Navigation, Store};

impl App {
    pub fn view(&self) -> ViewMode {
        self.view
    }

    pub fn repository(&self) -> &Repository {
        &self.repository
    }

    pub fn state(&self) -> &RepoState {
        &self.state
    }

    pub fn tracking(&self) -> &Tracking {
        &self.tracking
    }

    pub fn navigation(&self) -> &Navigation {
        &self.navigation
    }

    /// Hand the remembered navigation to another `App`, for example after a restart.
    pub fn into_navigation(self) -> Navigation {
        self.navigation
    }

    pub fn running(&self) -> bool {
        self.running
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn message_is_error(&self) -> bool {
        self.message_is_error
    }

    pub fn persistence_error(&self) -> Option<&str> {
        self.persistence_error.as_deref()
    }

    pub fn prompt(&self) -> Option<&Prompt> {
        self.prompt.as_ref()
    }

    pub fn detail_text(&self) -> &str {
        &self.detail_text
    }

    /// Horizontal offset in rendered terminal cells.
    pub fn detail_column(&self) -> u16 {
        self.detail_column
    }

    pub fn detail_scroll(&self) -> u16 {
        self.detail_scroll
    }

    pub fn file_selection(&self) -> usize {
        self.file_selection
    }

    pub fn file_detail(&self) -> FileDetail {
        self.file_detail
    }

    pub fn browse_tracked(&self) -> bool {
        self.browse_tracked
    }

    pub fn upstream_comparison(&self) -> bool {
        self.upstream_comparison
    }

    pub fn history_selection(&self) -> usize {
        self.history_selection
    }

    pub fn history_limit(&self) -> usize {
        self.history_limit
    }

    pub fn graph_visible(&self) -> bool {
        self.graph_visible
    }

    pub fn branch_selection(&self) -> usize {
        self.branch_selection
    }

    pub fn recent_selection(&self) -> usize {
        self.recent_selection
    }

    pub fn pr_filter(&self) -> &PrFilter {
        &self.pr_filter
    }

    pub fn review_pr(&self) -> Option<&PullRequest> {
        self.review_pr.as_ref()
    }

    pub fn line_editor_open(&self) -> bool {
        self.line_editor.is_some()
    }
    pub fn selected_line_numbers(&self) -> Vec<usize> {
        self.line_editor
            .as_ref()
            .map(|e| e.selection())
            .unwrap_or_default()
    }
    pub fn rebase_editor_open(&self) -> bool {
        self.rebase_editor.is_some()
    }
    pub fn rebase_todo(&self) -> Option<String> {
        self.rebase_editor.as_ref().map(|e| e.todo())
    }
    pub fn rebase_preview(&self) -> Option<String> {
        self.rebase_editor.as_ref().map(|e| e.result().join("\n"))
    }
    pub fn selected_stash_file_count(&self) -> usize {
        self.stash_files.len()
    }

    pub fn staged_hunks(&self) -> bool {
        self.staged_hunks
    }

    pub fn hunks(&self) -> &Listed<Hunk> {
        &self.hunks
    }

    pub fn stashes(&self) -> &Listed<StashEntry> {
        &self.stashes
    }

    pub fn tags(&self) -> &Listed<TagEntry> {
        &self.tags
    }

    pub fn reflog(&self) -> &Listed<ReflogEntry> {
        &self.reflog
    }

    pub fn remotes(&self) -> &Listed<RemoteEntry> {
        &self.remotes
    }

    pub fn workspaces(&self) -> &Listed<Workspace> {
        &self.workspaces
    }

    pub fn pull_requests(&self) -> &Listed<PullRequest> {
        &self.pull_requests
    }

    pub fn pr_files(&self) -> &Listed<PrFile> {
        &self.pr_files
    }

    /// Switch views without loading; `refresh` or the next action loads the view's data.
    pub fn set_view(&mut self, view: ViewMode) {
        self.view = view;
    }

    /// Move the current view's cursor to `index` (clamped) and show that entry's details.
    pub fn select(&mut self, index: usize) {
        let (selection, len) = self.cursor();
        *selection = index;
        super::clamp_to(selection, len);
        self.refresh_detail();
    }

    /// Close any open prompt or palette without submitting it, as Esc does.
    pub fn cancel_prompt(&mut self) {
        let filtering = self
            .prompt
            .as_ref()
            .is_some_and(|p| matches!(p.kind, super::PromptKind::ListFilter(_)));
        self.prompt = None;
        if filtering {
            self.refresh_detail();
        }
    }

    /// History window and graph toggle, as restored from saved navigation.
    pub fn set_history_window(&mut self, limit: usize, graph_visible: bool) {
        self.history_limit = limit;
        self.graph_visible = graph_visible;
    }

    /// Files marked as reviewed; transient, so never restored from saved navigation.
    pub fn seen_file_count(&self) -> usize {
        self.seen.len()
    }

    pub fn set_pr_filter(&mut self, filter: PrFilter) {
        self.pr_filter = filter;
    }

    /// Choose the pull request whose files the PrFiles view reviews.
    pub fn set_review_pr(&mut self, pr: PullRequest) {
        self.review_pr = Some(pr);
    }

    /// Report that UI settings cannot be saved; shown in the footer until a save succeeds.
    pub fn set_persistence_error(&mut self, error: impl Into<String>) {
        self.persistence_error = Some(error.into());
    }

    /// Record the current navigation and merge it into the settings store.
    pub fn save_navigation(&mut self, store: &mut Store) {
        self.capture_navigation();
        match store.persist(&self.navigation) {
            Ok(navigation) => {
                self.navigation = navigation;
                self.persistence_error = None;
            }
            Err(error) => self.persistence_error = Some(format!("{error:#}")),
        }
    }
}
