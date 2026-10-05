use crate::model::FileEntry;
use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::git::{ConflictSide, Repository};
use crate::model::{RepoState, ViewMode, display_path};

/// UI-independent commands; terminal key bindings live in `input`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
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

#[derive(Debug, Clone)]
pub enum PromptKind {
    ApplyCommit(String, bool),
    RestoreFile(crate::git::RestoreRequest),
    CreateTag,
    DeleteTag(crate::model::TagEntry),
    PublishTag(crate::model::TagEntry),
    DeleteRemoteTag(crate::model::TagEntry),
    AddRemote,
    EditPushUrl(String),
    EditRemote(String),
    RemoveRemote(String),
    CheckoutRemote(String),
    SetUpstream,
    PublishBranch,
    PullStrategy,
    SaveStash,
    ApplyStash(crate::model::StashEntry, bool),
    DropStash(crate::model::StashEntry),
    Commit(bool),
    Branch,
    RenameBranch(String),
    DeleteBranch(String),
    IntegrateBranch(String, bool),
    CloneRepository(String),
    Workspace,
    OpenRepository,
    Remove(crate::model::Workspace),
    CreatePr,
    Merge(crate::github::PullRequest),
    Review(u64, &'static str),
    Comment(u64),
}

#[derive(Debug)]
pub struct Prompt {
    pub kind: PromptKind,
    pub labels: Vec<&'static str>,
    pub values: Vec<String>,
    pub text: String,
}

#[derive(Debug)]
pub struct App {
    pub repository: Repository,
    pub state: RepoState,
    pub view: ViewMode,
    pub file_selection: usize,
    pub history_selection: usize,
    pub history_limit: usize,
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
    pub workspaces: Vec<crate::model::Workspace>,
    pub pull_requests: Vec<crate::github::PullRequest>,
    pub workspace_selection: usize,
    pub pr_selection: usize,
    pub prompt: Option<Prompt>,
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

impl App {
    pub fn new(repository: Repository) -> Result<Self> {
        let state = repository.load_state()?;
        let mut app = Self {
            repository,
            state,
            view: ViewMode::Files,
            file_selection: 0,
            history_selection: 0,
            history_limit: 20,
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
            workspaces: Vec::new(),
            pull_requests: Vec::new(),
            workspace_selection: 0,
            pr_selection: 0,
            prompt: None,
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
        app.tracking = app
            .repository
            .tracking()
            .unwrap_or_else(|error| crate::model::Tracking {
                error: Some(format!("{error:#}")),
                ..Default::default()
            });
        app.refresh_detail();
        Ok(app)
    }

    pub fn refresh(&mut self) -> Result<()> {
        // Keep the previous snapshot intact if reading the repository fails.
        self.state = self.repository.load_state()?;
        if self.history_limit > 20 {
            self.state.commits = self.repository.history(self.history_limit)?;
        }
        self.tracking = self
            .repository
            .tracking()
            .unwrap_or_else(|error| crate::model::Tracking {
                error: Some(format!("{error:#}")),
                ..Default::default()
            });
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
        self.load_extra()?;
        self.refresh_detail();
        Ok(())
    }

    fn load_extra(&mut self) -> Result<()> {
        match self.view {
            ViewMode::History => {
                self.state.commits = self.repository.history(self.history_limit)?;
                self.history_selection = self
                    .history_selection
                    .min(self.state.commits.len().saturating_sub(1));
            }
            ViewMode::Hunks => {
                self.hunks = if self.upstream_comparison {
                    Vec::new()
                } else if let Some(file) = self.state.files.get(self.file_selection) {
                    self.repository.hunks(file, self.staged_hunks)?
                } else {
                    Vec::new()
                };
                self.hunk_selection = self.hunk_selection.min(self.hunks.len().saturating_sub(1));
            }
            ViewMode::Tags => {
                self.tags = self.repository.tags()?;
                self.tag_selection = self.tag_selection.min(self.tags.len().saturating_sub(1));
            }
            ViewMode::Remotes => {
                self.remotes = self.repository.remotes()?;
                self.remote_selection = self
                    .remote_selection
                    .min(self.remotes.len().saturating_sub(1));
            }
            ViewMode::RemoteBranches => {
                self.remote_branches = self.repository.remote_branches()?;
                self.remote_branch_selection = self
                    .remote_branch_selection
                    .min(self.remote_branches.len().saturating_sub(1));
            }
            ViewMode::Stashes => {
                self.stashes = self.repository.stashes()?;
                self.stash_selection = self
                    .stash_selection
                    .min(self.stashes.len().saturating_sub(1));
            }
            ViewMode::GitHubRepositories => {
                self.github_repositories = crate::github::repositories(self.repository.root())?;
                self.github_repo_selection = self
                    .github_repo_selection
                    .min(self.github_repositories.len().saturating_sub(1));
            }
            ViewMode::Workspaces => {
                self.workspaces = self.repository.workspaces()?;
                self.workspace_selection = self
                    .workspace_selection
                    .min(self.workspaces.len().saturating_sub(1));
            }
            ViewMode::PullRequests => {
                self.pull_requests = crate::github::list(self.repository.root())?;
                self.pr_selection = self
                    .pr_selection
                    .min(self.pull_requests.len().saturating_sub(1));
            }
            _ => {}
        }
        Ok(())
    }

    pub fn selection(&self) -> usize {
        match self.view {
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

    fn move_selection(&mut self, delta: isize) {
        let file_len = self.displayed_files().len();
        let (selection, len) = match self.view {
            ViewMode::Hunks => (&mut self.hunk_selection, self.hunks.len()),
            ViewMode::Tags => (&mut self.tag_selection, self.tags.len()),
            ViewMode::Remotes => (&mut self.remote_selection, self.remotes.len()),
            ViewMode::RemoteBranches => (
                &mut self.remote_branch_selection,
                self.remote_branches.len(),
            ),
            ViewMode::Stashes => (&mut self.stash_selection, self.stashes.len()),
            ViewMode::Files => (&mut self.file_selection, file_len),
            ViewMode::History => (&mut self.history_selection, self.state.commits.len()),
            ViewMode::Branches => (&mut self.branch_selection, self.state.branches.len()),
            ViewMode::GitHubRepositories => (
                &mut self.github_repo_selection,
                self.github_repositories.len(),
            ),
            ViewMode::Workspaces => (&mut self.workspace_selection, self.workspaces.len()),
            ViewMode::PullRequests => (&mut self.pr_selection, self.pull_requests.len()),
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
                Some(pr) => crate::github::detail(self.repository.root(), pr.number),
                None => Ok("No open pull requests. N creates a draft PR; authenticate with gh auth login outside the TUI.".into()),
            },
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
        if self.prompt.is_some() {
            if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
                self.running = false;
                return;
            }
            if key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            {
                return;
            }
            if key.code == KeyCode::Esc {
                self.prompt = None;
                return;
            }
            let prompt = self.prompt.as_mut().unwrap();
            match key.code {
                KeyCode::Char(c) => prompt.text.push(c),
                KeyCode::Backspace => {
                    prompt.text.pop();
                }
                KeyCode::Enter => {
                    prompt.values.push(std::mem::take(&mut prompt.text));
                    if prompt.values.len() == prompt.labels.len() {
                        let prompt = self.prompt.take().unwrap();
                        if let Err(error) = self.submit_prompt(prompt) {
                            self.detail_scroll = 0;
                            self.detail_column = 0;
                            self.message = format!("{error:#}");
                            self.detail_text = format!("Action failed\n\n{}", self.message);
                            self.message_is_error = true;
                        }
                    }
                }
                _ => {}
            }
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

    fn start_prompt(&mut self, kind: PromptKind, labels: Vec<&'static str>) {
        self.prompt = Some(Prompt {
            kind,
            labels,
            values: Vec::new(),
            text: String::new(),
        });
    }

    fn open_repository(&mut self, path: &std::path::Path) -> Result<()> {
        // Construct first: a failed open leaves the existing repository intact.
        let mut replacement = Self::new(Repository::open(path)?)?;
        replacement.message = "Opened repository".into();
        *self = replacement;
        Ok(())
    }

    fn submit_prompt(&mut self, prompt: Prompt) -> Result<()> {
        use anyhow::ensure;
        let v = prompt.values;
        let output = match prompt.kind {
            PromptKind::ApplyCommit(sha, revert) => {
                ensure!(
                    v[0] == sha,
                    "Commit operation cancelled: confirm full commit ID"
                );
                let mainline = if v[1].trim().is_empty() {
                    None
                } else {
                    Some(
                        v[1].trim()
                            .parse::<usize>()
                            .context("Mainline must be a parent number")?,
                    )
                };
                self.run_action(
                    |repo| repo.apply_commit(&sha, revert, mainline),
                    if revert {
                        "Reverted commit"
                    } else {
                        "Cherry-picked commit"
                    },
                )?;
                self.message.clone()
            }
            PromptKind::RestoreFile(request) => {
                ensure!(v[0] == "discard", "Discard cancelled");
                let result = self.repository.restore_file(&request);
                let refreshed = self.refresh();
                result?;
                refreshed?;
                "Restored selected file".into()
            }
            PromptKind::CreateTag => {
                self.repository.create_tag(&v[0], &v[1], &v[2])?;
                "Created tag".into()
            }
            PromptKind::DeleteTag(tag) => {
                ensure!(v[0] == "delete", "Tag deletion cancelled");
                self.repository.delete_tag(&tag)?;
                "Deleted selected local tag".into()
            }
            PromptKind::PublishTag(tag) => {
                self.repository.publish_tag(&tag, &v[0])?;
                "Published selected tag".into()
            }
            PromptKind::DeleteRemoteTag(tag) => {
                ensure!(v[1] == "delete", "Remote tag deletion cancelled");
                self.repository.delete_remote_tag(&tag, &v[0])?;
                "Deleted remote tag; local tag retained".into()
            }

            PromptKind::AddRemote => {
                self.repository.add_remote(&v[0], &v[1])?;
                "Added remote".into()
            }
            PromptKind::EditPushUrl(name) => {
                self.repository.edit_push_url(&name, &v[0])?;
                "Updated push URL".into()
            }
            PromptKind::EditRemote(name) => {
                self.repository.edit_remote(&name, &v[0])?;
                "Updated fetch URL".into()
            }
            PromptKind::RemoveRemote(name) => {
                ensure!(v[0] == "remove", "Remote removal cancelled");
                self.repository.remove_remote(&name)?;
                "Removed remote".into()
            }
            PromptKind::CheckoutRemote(reference) => {
                self.repository.checkout_remote(&reference, &v[0])?;
                "Checked out tracking branch".into()
            }
            PromptKind::SetUpstream => {
                self.repository.set_upstream(&v[0])?;
                "Updated upstream".into()
            }
            PromptKind::PublishBranch => {
                let result = self.repository.publish_branch(&v[0]);
                let refreshed = self.refresh();
                result?;
                refreshed?;
                "Published current branch and set upstream".into()
            }
            PromptKind::PullStrategy => {
                let result = self.repository.pull_strategy(&v[0]);
                let refreshed = self.refresh();
                result?;
                refreshed?;
                "Pulled changes".into()
            }

            PromptKind::SaveStash => {
                let result = self.repository.save_stash(&v[0]);
                let refreshed = self.refresh();
                result?;
                refreshed?;
                "Saved stash including untracked files".into()
            }
            PromptKind::ApplyStash(stash, pop) => {
                ensure!(
                    v[0] == if pop { "pop" } else { "apply" },
                    "Stash operation cancelled"
                );
                let result = self.repository.apply_stash(&stash, pop);
                let refreshed = self.refresh();
                result?;
                refreshed?;
                if pop {
                    "Applied and removed stash".into()
                } else {
                    "Applied stash; entry retained".into()
                }
            }
            PromptKind::DropStash(stash) => {
                ensure!(v[0] == "drop", "Drop cancelled: type drop exactly");
                self.repository.drop_stash(&stash)?;
                "Dropped selected stash".into()
            }

            PromptKind::Commit(amend) => {
                // Reload after hooks even when they fail, preserving partial state changes.
                let result = if amend {
                    self.repository.amend(&v[0])
                } else {
                    self.repository.commit(&v[0])
                };
                let refreshed = self.refresh();
                result?;
                refreshed?;
                "Committed changes".into()
            }
            PromptKind::Branch => {
                self.repository.switch_or_create_branch(&v[0])?;
                "Switched branch".into()
            }
            PromptKind::RenameBranch(name) => {
                self.repository.rename_branch(&name, &v[0])?;
                "Renamed branch".into()
            }
            PromptKind::DeleteBranch(name) => {
                ensure!(v[0] == "delete", "Deletion cancelled: type delete exactly");
                self.repository.delete_branch(&name)?;
                "Deleted merged branch".into()
            }
            PromptKind::IntegrateBranch(name, rebase) => {
                ensure!(
                    v[0] == if rebase { "rebase" } else { "merge" },
                    "Operation cancelled: confirmation does not match"
                );
                let result = self.repository.integrate_branch(&name, rebase);
                let refreshed = self.refresh();
                result?;
                refreshed?;
                "Branch integrated".into()
            }

            PromptKind::CloneRepository(name) => {
                let path = PathBuf::from(&v[0]);
                let path = if path.is_absolute() {
                    path
                } else {
                    self.repository.root().join(path)
                };
                crate::github::clone_repository(self.repository.root(), &name, &path)?;
                return self.open_repository(&path);
            }
            PromptKind::Workspace => {
                self.repository
                    .create_workspace(std::path::Path::new(&v[0]), &v[1])?;
                "Workspace created".into()
            }
            PromptKind::OpenRepository => {
                let path = PathBuf::from(&v[0]);
                let path = if path.is_absolute() {
                    path
                } else {
                    self.repository.root().join(path)
                };
                return self.open_repository(&path);
            }
            PromptKind::Remove(w) => {
                ensure!(v[0] == "remove", "Removal cancelled: type remove exactly");
                self.repository.remove_workspace(&w)?;
                "Workspace removed; branch retained".into()
            }
            PromptKind::CreatePr => {
                self.repository.ensure_clean()?;
                crate::github::create(self.repository.root(), &v[0], &v[1], &v[2], &v[3])?
            }
            PromptKind::Merge(pr) => {
                ensure!(
                    v[0] == pr.number.to_string(),
                    "Merge cancelled: enter the selected PR number"
                );
                crate::github::merge(self.repository.root(), &pr)?
            }
            PromptKind::Review(number, verdict) => {
                crate::github::review(self.repository.root(), number, verdict, &v[0])?
            }
            PromptKind::Comment(number) => {
                crate::github::comment(self.repository.root(), number, &v[0])?
            }
        };
        // Preserve the operation result even if a network refresh then fails.
        let refreshed = self.refresh();
        self.message = if let Err(error) = refreshed {
            format!("{output}\nRefresh failed: {error:#}")
        } else {
            output
        };
        self.message_is_error = false;
        Ok(())
    }

    fn find_match(&mut self, next: bool) {
        if self.search.is_empty() {
            return;
        }
        if self.view == ViewMode::History {
            let len = self.state.commits.len();
            let start = if next {
                self.history_selection.saturating_add(1)
            } else {
                0
            };
            let found = (start..len).chain(0..start.min(len)).find(|index| {
                let c = &self.state.commits[*index];
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
            Action::LoadHistory if self.view == ViewMode::History => {
                self.history_limit = self.history_limit.saturating_add(100);
                self.load_extra()?;
                self.message = format!(
                    "Loaded {} commits across branches",
                    self.state.commits.len()
                );
                self.message_is_error = false;
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
                        PromptKind::ApplyCommit(sha, revert),
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
                        PromptKind::RestoreFile(request),
                        vec!["Type discard to confirm selected file"],
                    );
                }
            }

            Action::OpenHunks if self.view == ViewMode::Files && !self.upstream_comparison => {
                self.view = ViewMode::Hunks;
                self.hunk_selection = 0;
                self.load_extra()?;
                self.refresh_detail();
            }
            Action::ToggleComparison if self.view == ViewMode::Hunks => {
                self.staged_hunks = !self.staged_hunks;
                self.hunk_selection = 0;
                self.load_extra()?;
                self.refresh_detail();
            }
            Action::Stage | Action::Unstage
                if self.view == ViewMode::Hunks && !self.upstream_comparison =>
            {
                if (action == Action::Unstage) == self.staged_hunks
                    && let Some(hunk) = self.hunks.get(self.hunk_selection).cloned()
                {
                    self.run_action(
                        |repo| repo.apply_hunk(&hunk),
                        "Updated selected hunk in index",
                    )?;
                }
            }

            Action::New if self.view == ViewMode::Tags => self.start_prompt(
                PromptKind::CreateTag,
                vec![
                    "Tag name",
                    "Target commit (empty = HEAD)",
                    "Annotation (empty = lightweight)",
                ],
            ),
            Action::Remove | Action::PublishBranch | Action::DeleteRemoteTag
                if self.view == ViewMode::Tags =>
            {
                if let Some(tag) = self.tags.get(self.tag_selection).cloned() {
                    self.message = format!("Selected tag: {} ({})", tag.name, tag.sha);
                    match action {
                        Action::Remove => self.start_prompt(
                            PromptKind::DeleteTag(tag),
                            vec!["Type delete to remove local tag"],
                        ),
                        Action::PublishBranch => self.start_prompt(
                            PromptKind::PublishTag(tag),
                            vec!["Remote to publish selected tag"],
                        ),
                        _ => self.start_prompt(
                            PromptKind::DeleteRemoteTag(tag),
                            vec![
                                "Remote to delete selected tag from",
                                "Type delete to confirm remote deletion",
                            ],
                        ),
                    }
                }
            }

            Action::New if self.view == ViewMode::Remotes => {
                self.start_prompt(PromptKind::AddRemote, vec!["Remote name", "Remote URL"])
            }
            Action::EditRemote | Action::EditPushUrl | Action::Remove
                if self.view == ViewMode::Remotes =>
            {
                if let Some(remote) = self.remotes.get(self.remote_selection) {
                    let name = remote.name.clone();
                    self.message = format!("Selected remote: {name}");
                    if action == Action::EditPushUrl {
                        self.start_prompt(PromptKind::EditPushUrl(name), vec!["New push URL"]);
                    } else if action == Action::EditRemote {
                        self.start_prompt(PromptKind::EditRemote(name), vec!["New fetch URL"]);
                    } else {
                        self.start_prompt(
                            PromptKind::RemoveRemote(name),
                            vec!["Type remove to remove remote and tracking refs"],
                        );
                    }
                }
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
            Action::SwitchBranch if self.view == ViewMode::RemoteBranches => {
                if let Some(reference) = self.remote_branches.get(self.remote_branch_selection) {
                    self.start_prompt(
                        PromptKind::CheckoutRemote(reference.clone()),
                        vec!["New local tracking branch name"],
                    );
                }
            }

            Action::SaveStash => self.start_prompt(
                PromptKind::SaveStash,
                vec!["Stash message (includes untracked files)"],
            ),
            Action::ApplyStash | Action::PopStash | Action::Remove
                if self.view == ViewMode::Stashes =>
            {
                if let Some(stash) = self.stashes.get(self.stash_selection).cloned() {
                    self.message = format!("Selected {}: {}", stash.selector, stash.subject);
                    match action {
                        Action::ApplyStash => self.start_prompt(
                            PromptKind::ApplyStash(stash, false),
                            vec!["Type apply to restore stash and index"],
                        ),
                        Action::PopStash => self.start_prompt(
                            PromptKind::ApplyStash(stash, true),
                            vec!["Type pop to restore and remove on success"],
                        ),
                        _ => self.start_prompt(
                            PromptKind::DropStash(stash),
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
                if let Some(repo) = self.github_repositories.get(self.github_repo_selection) {
                    self.start_prompt(
                        PromptKind::CloneRepository(repo.name_with_owner.clone()),
                        vec!["Clone destination (new directory)"],
                    );
                }
            }
            Action::OpenRepository => {
                self.start_prompt(PromptKind::OpenRepository, vec!["Repository path"])
            }
            Action::New if self.view == ViewMode::Workspaces => self.start_prompt(
                PromptKind::Workspace,
                vec![
                    "Workspace path (relative to repo or absolute)",
                    "New branch name",
                ],
            ),
            Action::New if self.view == ViewMode::PullRequests => self.start_prompt(
                PromptKind::CreatePr,
                vec![
                    "Draft PR title",
                    "PR body",
                    "Base branch",
                    "Published head branch (or owner:branch)",
                ],
            ),
            Action::Remove if self.view == ViewMode::Workspaces => {
                if let Some(w) = self.workspaces.get(self.workspace_selection).cloned() {
                    self.message = format!("Remove {}? Branch will remain.", display_path(&w.path));
                    self.start_prompt(PromptKind::Remove(w), vec!["Type remove to confirm"]);
                }
            }
            Action::MergePr | Action::ApprovePr | Action::RequestChanges | Action::CommentPr
                if self.view == ViewMode::PullRequests =>
            {
                if let Some(pr) = self.pull_requests.get(self.pr_selection).cloned() {
                    self.message = format!("Selected PR #{}: {} ({})", pr.number, pr.title, pr.url);
                    match action {
                        Action::MergePr => self.start_prompt(
                            PromptKind::Merge(pr),
                            vec!["Confirm squash merge: enter PR number"],
                        ),
                        Action::ApprovePr => self.start_prompt(
                            PromptKind::Review(pr.number, "--approve"),
                            vec!["Approve review body"],
                        ),
                        Action::RequestChanges => self.start_prompt(
                            PromptKind::Review(pr.number, "--request-changes"),
                            vec!["Requested changes"],
                        ),
                        _ => self.start_prompt(PromptKind::Comment(pr.number), vec!["PR comment"]),
                    }
                }
            }
            Action::PrDiff if self.view == ViewMode::PullRequests => {
                if let Some(pr) = self.pull_requests.get(self.pr_selection) {
                    self.detail_text = crate::github::run(
                        self.repository.root(),
                        &["pr", "diff", &pr.number.to_string(), "--color", "never"],
                    )?;
                    self.detail_scroll = 0;
                    self.detail_column = 0;
                }
            }
            Action::SwitchBranch if self.view == ViewMode::Workspaces => {
                if let Some(w) = self.workspaces.get(self.workspace_selection) {
                    let path = w.path.clone();
                    self.open_repository(&path)?;
                }
            }
            Action::SwitchBranch if self.view == ViewMode::PullRequests => {
                if let Some(pr) = self.pull_requests.get(self.pr_selection) {
                    self.repository.ensure_clean()?;
                    let number = pr.number;
                    let result = crate::github::checkout(self.repository.root(), number);
                    let refreshed = self.refresh();
                    result?;
                    refreshed?;
                    self.message = format!("Checked out PR #{number}");
                    self.message_is_error = false;
                }
            }
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
            Action::Commit => self.start_prompt(PromptKind::Commit(false), vec!["Commit message"]),
            Action::AmendCommit => {
                self.message =
                    "Amend rewrites the latest commit; enter its replacement message".into();
                self.start_prompt(PromptKind::Commit(true), vec!["Amend commit message"]);
            }
            Action::Branch => {
                self.start_prompt(PromptKind::Branch, vec!["Create or switch branch"])
            }
            Action::RenameBranch | Action::MergeBranch | Action::RebaseBranch | Action::Remove
                if self.view == ViewMode::Branches =>
            {
                if let Some(branch) = self.state.branches.get(self.branch_selection) {
                    let name = branch.name.clone();
                    self.message = format!("Selected branch: {name}");
                    match action {
                        Action::RenameBranch => self
                            .start_prompt(PromptKind::RenameBranch(name), vec!["New branch name"]),
                        Action::MergeBranch => self.start_prompt(
                            PromptKind::IntegrateBranch(name, false),
                            vec!["Type merge to merge into the current branch"],
                        ),
                        Action::RebaseBranch => self.start_prompt(
                            PromptKind::IntegrateBranch(name, true),
                            vec!["Type rebase to rebase the current branch onto selection"],
                        ),
                        _ => self.start_prompt(
                            PromptKind::DeleteBranch(name),
                            vec!["Type delete to remove a merged branch"],
                        ),
                    }
                }
            }
            Action::Fetch if self.view == ViewMode::Remotes => {
                if let Some(remote) = self.remotes.get(self.remote_selection) {
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
