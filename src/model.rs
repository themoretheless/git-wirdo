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
    pub cherry_pick_in_progress: bool,
    pub revert_in_progress: bool,
    pub conflicts: Vec<PathBuf>,
}

impl MergeState {
    pub fn in_progress(&self) -> bool {
        self.merge_in_progress
            || self.rebase_in_progress
            || self.cherry_pick_in_progress
            || self.revert_in_progress
    }

    pub fn summary(&self) -> String {
        let operation = if self.rebase_in_progress {
            "Rebase in progress"
        } else if self.cherry_pick_in_progress {
            "Cherry-pick in progress"
        } else if self.revert_in_progress {
            "Revert in progress"
        } else if self.merge_in_progress {
            "Merge in progress"
        } else {
            "No Git operation in progress"
        };
        format!("{operation}\nConflicts: {}", self.conflicts.len())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Files,
    History,
    Branches,
    Conflicts,
    Workspaces,
    PullRequests,
    GitHubRepositories,
    Stashes,
    Remotes,
    RemoteBranches,
    Tags,
    Hunks,
}

impl ViewMode {
    pub fn index(self) -> usize {
        match self {
            Self::Files => 0,
            Self::History => 1,
            Self::Branches => 2,
            Self::Conflicts => 3,
            Self::Workspaces => 4,
            Self::PullRequests => 5,
            Self::GitHubRepositories => 6,
            Self::Stashes => 7,
            Self::Remotes => 8,
            Self::RemoteBranches => 9,
            Self::Tags => 10,
            Self::Hunks => 11,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Files => Self::History,
            Self::History => Self::Branches,
            Self::Branches => Self::Conflicts,
            Self::Conflicts => Self::Workspaces,
            Self::Workspaces => Self::PullRequests,
            Self::PullRequests => Self::GitHubRepositories,
            Self::GitHubRepositories => Self::Stashes,
            Self::Stashes => Self::Remotes,
            Self::Remotes => Self::RemoteBranches,
            Self::RemoteBranches => Self::Tags,
            Self::Tags => Self::Hunks,
            Self::Hunks => Self::Files,
        }
    }
}

#[derive(Debug)]
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

#[derive(Debug, Clone)]
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
