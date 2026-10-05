use crate::model::FileEntry;
use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::git::{ConflictSide, Repository};
use crate::model::{RepoState, ViewMode, display_path};

// Keep the original workflow defaults in this reliability-focused refactor.
const DEFAULT_COMMIT_MESSAGE: &str = "wirdo update";
const DEFAULT_BRANCH_NAME: &str = "feature/wirdo";

/// UI-independent commands; terminal key bindings live in `input`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
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

#[derive(Debug)]
pub struct App {
    pub repository: Repository,
    pub state: RepoState,
    pub view: ViewMode,
    pub file_selection: usize,
    pub history_selection: usize,
    pub branch_selection: usize,
    pub conflict_selection: usize,
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
}

impl App {
    pub fn new(repository: Repository) -> Result<Self> {
        let state = repository.load_state()?;
        let mut app = Self {
            repository,
            state,
            view: ViewMode::Files,
            file_selection: 0,
            history_selection: 0,
            branch_selection: 0,
            conflict_selection: 0,
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
        };
        app.refresh_detail();
        Ok(app)
    }

    pub fn refresh(&mut self) -> Result<()> {
        // Keep the previous snapshot intact if reading the repository fails.
        self.state = self.repository.load_state()?;
        if self.upstream_comparison {
            self.review_files = self.repository.upstream_files()?;
        }
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
        self.file_selection = self
            .file_selection
            .min(self.displayed_files().len().saturating_sub(1));
        self.history_selection = self
            .history_selection
            .min(self.state.commits.len().saturating_sub(1));
        self.branch_selection = self
            .branch_selection
            .min(self.state.branches.len().saturating_sub(1));
        self.conflict_selection = self
            .conflict_selection
            .min(self.state.merge_state.conflicts.len().saturating_sub(1));
        self.refresh_detail();
        Ok(())
    }

    pub fn selection(&self) -> usize {
        match self.view {
            ViewMode::Files => self.file_selection,
            ViewMode::History => self.history_selection,
            ViewMode::Branches => self.branch_selection,
            ViewMode::Conflicts => self.conflict_selection,
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let file_len = self.displayed_files().len();
        let (selection, len) = match self.view {
            ViewMode::Files => (&mut self.file_selection, file_len),
            ViewMode::History => (&mut self.history_selection, self.state.commits.len()),
            ViewMode::Branches => (&mut self.branch_selection, self.state.branches.len()),
            ViewMode::Conflicts => (
                &mut self.conflict_selection,
                self.state.merge_state.conflicts.len(),
            ),
        };
        *selection = selection
            .saturating_add_signed(delta)
            .min(len.saturating_sub(1));
        self.refresh_detail();
    }

    fn refresh_detail(&mut self) {
        self.detail_scroll = 0;
        self.detail_column = 0;
        self.detail_text = self
            .selected_detail()
            .unwrap_or_else(|error| format!("Cannot load details:\n{error:#}"));
    }

    fn selected_detail(&self) -> Result<String> {
        match self.view {
            ViewMode::Files => match self.displayed_files().get(self.file_selection) {
                Some(file) if self.upstream_comparison => self.repository.upstream_detail(file),
                Some(file) => self.repository.file_detail(file),
                None => Ok("No files selected".to_owned()),
            },
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
                        "{header}\n\nNo conflicted file selected.\n\n{help}"
                    )),
                }
            }
        }
    }

    pub fn displayed_files(&self) -> &[FileEntry] {
        if self.upstream_comparison {
            &self.review_files
        } else {
            &self.state.files
        }
    }

    pub fn file_seen(&self, file: &FileEntry) -> bool {
        self.seen
            .contains_key(&(self.upstream_comparison, file.path.clone()))
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        if self.searching
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

    fn find_match(&mut self, next: bool) {
        if self.search.is_empty() {
            return;
        }
        let start = if next {
            usize::from(self.detail_scroll) + 1
        } else {
            0
        };
        let lines: Vec<_> = self.detail_text.lines().collect();
        let found = (start..lines.len())
            .chain(0..start.min(lines.len()))
            .find(|index| lines[*index].contains(&self.search));
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

    /// Git failures are recoverable UI messages, not reasons to leave the terminal in raw mode.
    pub fn handle(&mut self, action: Action) {
        if let Err(error) = self.apply(action) {
            self.detail_scroll = 0;
            self.message = format!("{error:#}");
            self.detail_text = format!("Action failed\n\n{}", self.message);
            self.message_is_error = true;
        }
    }

    fn apply(&mut self, action: Action) -> Result<()> {
        match action {
            Action::ScrollLeft => self.detail_column = self.detail_column.saturating_sub(10),
            Action::ScrollRight => {
                self.detail_column = self.detail_column.saturating_add(10).min(
                    self.detail_text
                        .lines()
                        .map(|line| line.chars().count())
                        .max()
                        .unwrap_or(0)
                        .min(u16::MAX as usize) as u16,
                )
            }
            Action::PageDown => {
                self.detail_scroll = self.detail_scroll.saturating_add(10).min(
                    self.detail_text
                        .lines()
                        .count()
                        .saturating_sub(1)
                        .min(u16::MAX as usize) as u16,
                )
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
                self.upstream_comparison = !self.upstream_comparison;
                self.file_selection = 0;
                self.refresh_detail();
                self.message = if self.upstream_comparison {
                    "Compared with upstream merge base"
                } else {
                    "Working changes"
                }
                .into();
                self.message_is_error = false;
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
                self.message = "Refreshed".to_owned();
                self.message_is_error = false;
            }
            Action::NextView => {
                self.view = self.view.next();
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
            Action::Commit => self.run_action(
                |repo| repo.commit(DEFAULT_COMMIT_MESSAGE),
                "Committed changes",
            )?,
            Action::Branch => self.run_action(
                |repo| repo.switch_or_create_branch(DEFAULT_BRANCH_NAME),
                format!("Switched to {DEFAULT_BRANCH_NAME}"),
            )?,
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
                self.run_action(Repository::continue_operation, "Continued merge/rebase")?
            }
            Action::SkipRebase if self.view == ViewMode::Conflicts => {
                self.run_action(Repository::skip_rebase, "Skipped current rebase commit")?
            }
            Action::Abort if self.view == ViewMode::Conflicts => {
                self.run_action(Repository::abort_operation, "Aborted merge/rebase")?
            }
            _ => {}
        }
        Ok(())
    }

    fn run_action<F>(&mut self, action: F, success: impl Into<String>) -> Result<()>
    where
        F: FnOnce(&Repository) -> Result<()>,
    {
        let result = action(&self.repository);
        // A failed pull/rebase/hook can still change files or the index. Always reload afterwards.
        let refreshed = self.refresh();
        if let Err(error) = result {
            return match refreshed {
                Ok(()) => Err(error),
                Err(refresh_error) => {
                    Err(error.context(format!("Repository refresh also failed: {refresh_error:#}")))
                }
            };
        }
        let success = success.into();
        refreshed.with_context(|| format!("{success}, but refreshing the repository failed"))?;
        self.message = success;
        self.message_is_error = false;
        Ok(())
    }
}
