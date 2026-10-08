//! Discoverable commands reuse the same guarded application actions as shortcuts.
use crate::{app::Action, model::ViewMode};

#[derive(Debug, Clone)]
pub struct Entry {
    pub title: &'static str,
    pub action: Action,
    pub view: Option<ViewMode>,
}

pub fn filtered(view: ViewMode, query: &str) -> Vec<Entry> {
    use Action::*;
    let mut entries = vec![
        ("Refresh repository", Refresh),
        ("Open local repository", OpenRepository),
        ("Recent repositories", RecentRepositories),
        ("Commit staged changes", Commit),
        ("Amend latest commit", AmendCommit),
        ("Create or switch branch", Branch),
        ("Fetch remotes", Fetch),
        ("Pull fast-forward only", Pull),
        ("Push upstream", Push),
        ("Publish branch and set upstream", PublishBranch),
        ("Set upstream", SetUpstream),
        ("Pull with chosen strategy", PullStrategy),
        ("Save stash", SaveStash),
        ("Import mail patch", ImportPatch),
        ("GitHub authentication status", GitHubStatus),
        ("Search loaded commits or details", Search),
        ("Quit", Quit),
    ];
    let contextual: &[(&str, Action)] = match view {
        ViewMode::Files => &[
            ("Browse tracked files / working changes", BrowseTracked),
            ("File history / diff", FileHistory),
            ("Line authorship / diff", FileBlame),
            ("Stage selected file", Stage),
            ("Unstage selected file", Unstage),
            ("Inspect editable hunks", OpenHunks),
            ("Restore selected file from index", RestoreFromIndex),
            ("Discard selected file from HEAD", Remove),
            ("Compare upstream / working changes", ToggleComparison),
            ("Mark file seen", ToggleSeen),
        ],
        ViewMode::History => &[
            ("Export selected commit as mail patch", ExportCommit),
            ("Load older history", LoadHistory),
            ("Toggle history graph", ToggleGraph),
            ("Cherry-pick selected commit", CherryPick),
            ("Revert selected commit", RevertCommit),
            ("Reset to selected commit", ResetCommit),
        ],
        ViewMode::Branches => &[
            ("Checkout selected branch", SwitchBranch),
            ("Rename selected branch", RenameBranch),
            ("Delete selected branch", Remove),
            ("Merge selected branch", MergeBranch),
            ("Rebase onto selected branch", RebaseBranch),
        ],
        ViewMode::Conflicts => &[
            ("Resolve selected conflict with ours", TakeOurs),
            ("Resolve selected conflict with theirs", TakeTheirs),
            ("Stage manually resolved conflict", MarkResolved),
            ("Continue Git operation", Continue),
            ("Abort Git operation", Abort),
            ("Skip current rebase commit", SkipRebase),
        ],
        ViewMode::Workspaces => &[
            ("Create worktree", New),
            ("Open selected worktree", SwitchBranch),
            ("Remove selected worktree", Remove),
        ],
        ViewMode::PullRequests => &[
            ("Create draft pull request", New),
            ("Checkout selected pull request", SwitchBranch),
            ("Show pull request patch", PrDiff),
            ("Merge selected pull request", MergePr),
            ("Approve selected pull request", ApprovePr),
            ("Request changes on pull request", RequestChanges),
            ("Comment on pull request", CommentPr),
            ("Edit pull request", EditRemote),
            ("Filter pull requests", SetUpstream),
            ("Toggle pull request draft / ready", PopStash),
            ("Review pull request files", OpenHunks),
            ("Load more pull requests", LoadHistory),
        ],
        ViewMode::GitHubRepositories => &[("Clone selected GitHub repository", SwitchBranch)],
        ViewMode::Stashes => &[
            ("Apply selected stash", ApplyStash),
            ("Pop selected stash", PopStash),
            ("Drop selected stash", Remove),
        ],
        ViewMode::Remotes => &[
            ("Add remote", New),
            ("Edit fetch URL", EditRemote),
            ("Edit push URL", EditPushUrl),
            ("Remove remote", Remove),
        ],
        ViewMode::RemoteBranches => &[("Checkout selected remote branch", SwitchBranch)],
        ViewMode::Tags => &[
            ("Create tag", New),
            ("Delete local tag", Remove),
            ("Publish selected tag", PublishBranch),
            ("Delete remote tag", DeleteRemoteTag),
        ],
        ViewMode::Hunks => &[
            ("Stage selected hunk", Stage),
            ("Unstage selected hunk", Unstage),
            ("Toggle staged / unstaged hunks", ToggleComparison),
        ],
        ViewMode::Reflog => &[
            ("Recover selected commit into new branch", New),
            ("Load older reflog entries", LoadHistory),
        ],
        ViewMode::PrFiles => &[("Comment on selected pull request line", CommentPr)],
        ViewMode::RecentRepositories => &[
            ("Open selected recent repository", SwitchBranch),
            ("Initialize repository", New),
            ("Clone Git URL or local repository", CommentPr),
            ("Forget selected recent repository", Remove),
        ],
    };
    entries.retain(|(_, action)| !contextual.iter().any(|(_, local)| local == action));
    entries.extend_from_slice(contextual);
    let mut result: Vec<_> = entries
        .into_iter()
        .map(|(title, action)| Entry {
            title,
            action,
            view: None,
        })
        .collect();
    for (target, title) in ViewMode::ALL
        .into_iter()
        .filter_map(|target| view_title(target).map(|title| (target, title)))
    {
        result.push(Entry {
            title,
            action: if target == ViewMode::RecentRepositories {
                RecentRepositories
            } else {
                Refresh
            },
            view: if target == ViewMode::RecentRepositories {
                None
            } else {
                Some(target)
            },
        });
    }
    let query = query.to_lowercase();
    result.retain(|entry| {
        query
            .split_whitespace()
            .all(|term| entry.title.to_lowercase().contains(term))
    });
    result
}

/// Palette title for switching to a view; views that need a fresh target have none.
fn view_title(view: ViewMode) -> Option<&'static str> {
    Some(match view {
        ViewMode::Files => "View files",
        ViewMode::History => "View commit history",
        ViewMode::Branches => "View branches",
        ViewMode::Conflicts => "View conflicts and operation recovery",
        ViewMode::Workspaces => "View worktrees",
        ViewMode::PullRequests => "View pull requests",
        ViewMode::GitHubRepositories => "View GitHub repositories",
        ViewMode::Stashes => "View stashes",
        ViewMode::Remotes => "View remotes",
        ViewMode::RemoteBranches => "View remote branches",
        ViewMode::Tags => "View tags",
        ViewMode::Hunks => "View hunks",
        ViewMode::Reflog => "View reflog recovery",
        ViewMode::RecentRepositories => "View recent repositories",
        ViewMode::PrFiles => return None,
    })
}
