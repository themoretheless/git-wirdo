use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub path: PathBuf,
    pub original_path: Option<PathBuf>,
    pub status: String,
    pub staged: bool,
    pub unstaged: bool,
    pub conflicted: bool,
}

impl FileEntry {
    pub fn label(&self) -> String {
        match &self.original_path {
            Some(original) => format!("{} -> {}", display_path(original), display_path(&self.path)),
            None => display_path(&self.path),
        }
    }

    pub(crate) fn paths(&self) -> Vec<&Path> {
        let mut paths = Vec::new();
        // Resetting a copy must not unstage independent changes in its source file.
        if self.status.contains('R')
            && let Some(original) = &self.original_path
        {
            paths.push(original.as_path());
        }
        paths.push(self.path.as_path());
        paths
    }
}

#[derive(Debug, Clone)]
pub struct BranchEntry {
    pub name: String,
    pub current: bool,
}

#[derive(Debug, Clone)]
pub struct CommitEntry {
    pub sha: String,
    pub short_sha: String,
    pub date: String,
    pub author: String,
    pub subject: String,
}

#[derive(Debug, Clone, Default)]
pub struct MergeState {
    pub merge_in_progress: bool,
    pub rebase_in_progress: bool,
    pub am_in_progress: bool,
    pub cherry_pick_in_progress: bool,
    pub revert_in_progress: bool,
    pub conflicts: Vec<PathBuf>,
}

/// A multi-step Git operation that can be continued or aborted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    MailImport,
    Rebase,
    CherryPick,
    Revert,
    Merge,
}

impl Operation {
    /// The Git subcommand that owns `--continue` / `--abort` for this operation.
    pub fn command(self) -> &'static str {
        match self {
            Self::MailImport => "am",
            Self::Rebase => "rebase",
            Self::CherryPick => "cherry-pick",
            Self::Revert => "revert",
            Self::Merge => "merge",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::MailImport => "Mail patch import",
            Self::Rebase => "Rebase",
            Self::CherryPick => "Cherry-pick",
            Self::Revert => "Revert",
            Self::Merge => "Merge",
        }
    }
}

impl MergeState {
    pub fn in_progress(&self) -> bool {
        self.operation().is_some()
    }

    /// Mail import shares Git's rebase-apply directory, so it is reported first.
    pub fn operation(&self) -> Option<Operation> {
        [
            (self.am_in_progress, Operation::MailImport),
            (self.rebase_in_progress, Operation::Rebase),
            (self.cherry_pick_in_progress, Operation::CherryPick),
            (self.revert_in_progress, Operation::Revert),
            (self.merge_in_progress, Operation::Merge),
        ]
        .into_iter()
        .find_map(|(active, operation)| active.then_some(operation))
    }

    pub fn summary(&self) -> String {
        let operation = match self.operation() {
            Some(operation) => format!("{} in progress", operation.label()),
            None => "No Git operation in progress".to_owned(),
        };
        format!("{operation}\nConflicts: {}", self.conflicts.len())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ViewMode {
    Files,
    History,
    Branches,
    Conflicts,
    #[serde(rename = "Workspaces", alias = "Worktrees")]
    Worktrees,
    PullRequests,
    GitHubRepositories,
    Stashes,
    Remotes,
    RemoteBranches,
    Tags,
    Hunks,
    Reflog,
    PrFiles,
    RecentRepositories,
}

impl ViewMode {
    /// Every view in Tab order; list rendering and cycling derive from this single list.
    pub const ALL: [Self; 15] = [
        Self::Files,
        Self::History,
        Self::Branches,
        Self::Conflicts,
        Self::Worktrees,
        Self::PullRequests,
        Self::GitHubRepositories,
        Self::Stashes,
        Self::Remotes,
        Self::RemoteBranches,
        Self::Tags,
        Self::Hunks,
        Self::Reflog,
        Self::PrFiles,
        Self::RecentRepositories,
    ];

    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|view| *view == self)
            .expect("every view is listed in ViewMode::ALL")
    }

    pub fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }
}

#[derive(Debug, Clone)]
pub struct RepoState {
    pub branch: String,
    pub files: Vec<FileEntry>,
    pub branches: Vec<BranchEntry>,
    pub commits: Vec<CommitEntry>,
    pub merge_state: MergeState,
}

impl RepoState {
    pub fn summary(&self) -> String {
        if self.files.is_empty() {
            "clean".to_owned()
        } else {
            self.files
                .iter()
                .map(|file| format!("{} {}", file.status, file.label()))
                .collect::<Vec<_>>()
                .join("\n")
        }
    }
}

/// Escape control characters for single-line labels, without changing the path used by Git.
pub fn display_path(path: &Path) -> String {
    path.to_string_lossy().escape_debug().to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    pub path: PathBuf,
    pub branch: String,
    pub locked: bool,
    pub bare: bool,
}

#[derive(Debug, Clone)]
pub struct StashEntry {
    pub selector: String,
    pub sha: String,
    pub subject: String,
}

#[derive(Debug, Clone)]
pub struct RemoteEntry {
    pub name: String,
    pub fetch_urls: String,
    pub push_urls: String,
}

#[derive(Debug, Clone, Default)]
pub struct Tracking {
    pub error: Option<String>,
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
}

#[derive(Debug, Clone)]
pub struct TagEntry {
    pub name: String,
    pub sha: String,
    pub kind: String,
    pub subject: String,
}

#[derive(Debug, Clone)]
pub struct ReflogEntry {
    pub sha: String,
    pub selector: String,
    pub subject: String,
}
