# Git Wirdo

A lightweight Git client written in Rust with a terminal UI for status, staging,
branch switching, commits, fetch/pull/push, history, and merge/rebase conflicts.

## Run

Install a current stable Rust toolchain and Git 2.36 or newer, then from this checkout:

```bash
cargo run --locked -- --repo /path/to/repo
```

`--repo` defaults to the current directory. A subdirectory of a working tree is
also accepted; commands operate relative to the repository root. Linked worktrees
and repositories with no commits are supported. Bare repositories are not.

Starting in a directory outside a checkout opens repository selection. `--start`
opens that screen explicitly: `O` opens a local path, `N` initializes a repository
with a chosen initial branch (default `main`), and `C` clones an arbitrary Git URL
or local repository into a new or empty directory. Enter opens a saved recent
root. Relative paths resolve from the startup directory. Operations run in the
background; Esc cancels, and `q` cancels and waits for process cleanup before exit.
Errors retain the selection screen and allow retrying or opening another path.

The same operations are available after opening a checkout: press `I`, then `N`
or `C`; relative paths resolve from that working tree's root. Initialization keeps
ordinary files and refuses existing Git metadata, bare repositories, metadata
subdirectories, files and destination symlinks. Clone refuses nonempty directories.
The client never deletes a destination after a failed/cancelled operation; Git may
clean up its own incomplete clone. Inspect the reported destination before retrying.
Credentials use Git's configured mechanisms; Git password prompts are disabled.

Explicit lifecycle commands also work outside a checkout:

```bash
git-wirdo --init ./new-project --initial-branch main --headless
git-wirdo --clone https://example.org/team/project.git --destination ./project --headless
```

Omit `--headless` to open the resulting working tree in the UI. Explicit `--repo`
errors and noninteractive inspection errors never enter raw terminal mode.

For scripts, redirected output, or a quick status check without a terminal:

```bash
cargo run --locked -- --repo /path/to/repo --headless
```

To install the executable locally:

```bash
cargo install --locked --path .
git-wirdo --repo /path/to/repo
```

## Keys

| Key | View | Action |
| --- | --- | --- |
| `Tab` | Any | Cycle views (including workspaces, PRs, stashes and remotes) |
| `j` / `k`, `↓` / `↑` | Any | Move the selection; the list follows it |
| `r` | Any | Refresh repository state and the detail pane |
| `PageUp` / `PageDown` | Any | Scroll details by ten lines |
| `←` / `→` | Any | Scroll long detail lines horizontally |
| `/`, then text and `Enter` | Any | Search loaded commits in History, otherwise selected details; `Esc` cancels input |
| `n` | Any | Find the next match, wrapping at the end |
| `d` | Files | Toggle working changes / upstream merge-base comparison |
| `v` | Files | Toggle the selected file's Seen mark |
| `q`, `Ctrl-C` | Any | Quit and restore the terminal |
| `s` / `u` | Files | Stage / unstage the selected file |
| `c` | Any | Enter a message and commit staged changes |
| `b` | Any | Enter a branch name to create or switch |
| `Enter` | Branches | Switch to the selected local branch |
| `f` | Any | Fetch all remotes |
| `p` | Any | Pull from upstream, **fast-forward only** |
| `P` | Any | Push using the configured upstream |
| `o` / `t` | Conflicts | Take ours / theirs and stage the result |
| `a` | Conflicts | Stage a file after resolving it manually |
| `e` | Conflicts | Continue merge, rebase, cherry-pick or revert |
| `K` | Conflicts | Skip the current rebase commit |
| `x` | Conflicts | Abort merge, rebase, cherry-pick or revert |

**Binding change:** lowercase `k` always moves up. Only uppercase `K` skips a
rebase commit. Staging shortcuts never act on a hidden file selection in another
view. Key releases/repeats and unrelated Ctrl/Alt shortcuts do not trigger Git actions.

Taking a conflict side replaces that file's working copy. If the chosen side
removed the file, the deletion is staged. During a **rebase**, Git's “ours” means
the branch being rebased **onto**, and “theirs” means the commit being replayed.
`K` discards the current rebase step; `x` aborts the operation. These are explicit
Git actions, not undo commands.

## Reliability and behavior

- Status uses NUL-delimited porcelain output, not human-formatted Git output.
  Spaces, quotes, newlines, renames, Unicode, and non-UTF-8 filename bytes (on compatible Unix filesystems) are preserved. Control characters are escaped for display only.
- File arguments are literal pathspecs. A file named `[abc].txt` or `*.rs` cannot
  accidentally stage other matching files. Unstaging a rename handles both names.
- Unstaging before the first commit removes the index entry, **not** the working file.
- Merge/rebase state is read from the actual per-worktree Git directory.
- Git action failures appear in the action line and detail pane instead of exiting
  the app. State is reloaded even after a failed command, since a rebase may have
  advanced to another conflict before returning an error. Press `r` to retry a
  refresh, or keep navigating.
- Terminal cleanup runs on ordinary exits, setup/event/render errors, and unwinding
  panics. Non-interactive use must specify `--headless`.
- Git pagers, color, external diff/textconv helpers, and interactive Git editors are
  disabled for the UI's commands. Git hooks still run normally.
- Untracked text previews are capped at 256 KiB. Binary files and special files get
  a descriptive placeholder; symlink previews show the target without reading it.

Configure remotes, upstream branches, credentials, signing, and an SSH agent
before launching the UI. Git credential prompts and interactive Git editors are
not available inside the TUI. Git/network commands and hooks run in a serialized background task. The UI
shows elapsed time and command diagnostics; Esc cancels, q cancels and exits.

Commit and branch names are entered in the TUI. `E` amends the latest commit
with a replacement message and staged changes. In Branches, `B` renames the
selected branch; `D` deletes it after typing `delete`. Deletion refuses the
current branch, release/hotfix branches, branches checked out in another worktree
and branches containing commits not merged into the current HEAD.

`m` merges the selected branch into the current branch after typing `merge`.
`z` rebases the current branch onto the selected branch after typing `rebase`.
Both require a clean working tree. Conflicts remain visible in Conflicts for
resolution, continuation or abort; they are never silently resolved.
History begins with 20 commits across local/remote branches in topological order.
`+` loads another 100 (and keeps the larger window on refresh). `/` searches loaded
commit IDs, subjects, authors and dates; `n` moves to the next matching commit.
Commit details show authorship/commit dates, the complete message, statistics and
the patch. Use PageUp/PageDown for long patches. `g` toggles the branch/merge graph
in the detail pane; j/k highlights the selected commit.

In History, `Y` cherry-picks the selected commit and `Z` reverts it by creating a
new commit. Confirm the full selected commit ID; leave the mainline field blank
for ordinary commits, or enter the parent number for a merge commit. Both actions
require a clean checkout and no unfinished operation. Conflicts are retained in
Conflicts: resolve and stage, then `e` continues or `x` aborts. `K` remains specific
to rebase. `F` opens reset; modes and recovery are described below.
Background tasks keep the terminal responsive while Git runs.

## Code layout

- `src/patch.rs` — exact-byte hunk separation for selective staging
- `src/git.rs` — repository discovery, machine-readable Git output, and Git operations
- `src/model.rs` — repository data and display-safe path labels
- `src/app.rs` — UI-independent actions, selections, refresh, and error handling
- `src/input.rs` — terminal key events mapped to application actions
- `src/ui.rs` — Ratatui rendering and list scroll state
- `src/terminal.rs` — terminal lifecycle and event loop
- `src/main.rs` — CLI and headless output

## Development

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
```

Tests create and clean up isolated temporary repositories, with local Git identity
and hooks configuration. They do **not** switch branches, edit Git configuration,
create commits, or push in this project's checkout. Remote tests use local bare
repositories and require no network access after Rust dependencies are installed.

Coverage includes porcelain parsing, unusual paths, initial commits, staging and
renames, branch-switch failures, detached HEAD, worktrees, modify/delete conflicts,
merge/rebase continuation and abort, state refresh after partial failures, key
bindings, terminal rendering, and the CLI. CI runs formatting, Clippy, and tests on
Linux, macOS, and Windows.

## Reviewing changes

Inspired by Delta's review workflow, `d` compares tracked working-tree files with
`merge-base HEAD @{upstream}`. This includes committed branch changes and pending
tracked edits, while excluding changes made only on the upstream branch after
it diverged. Configure the upstream with `git branch --set-upstream-to=<ref>`.
Comparison uses locally available refs; press `f` to fetch before reviewing.
Untracked files remain in the working-changes view. Stage/unstage shortcuts act
only in that view, so reviewing a committed change cannot modify the index.

`v` marks a file Seen for the current comparison during this session. The header
shows review progress. Refresh retains marks only while the file's displayed
contents remain identical; edits or removed files clear their marks. Marks are
independent between working and upstream comparisons and are not saved to disk.
Search is case-sensitive: loaded commit metadata in History, selected details in other views. While entering a query, ordinary shortcut letters are text.

## Workspaces and GitHub

Tab cycles Files → History → Branches → Conflicts → Workspaces → PullRequests →
GitHubRepositories → Stashes → Remotes → RemoteBranches → Tags → Hunks → Reflog → PrFiles → RecentRepositories. `O` opens another local repository by path; the previous
checkout and its files remain on disk. Relative paths are resolved from the
current repository root. Workspaces are Git linked worktrees, not cloud sessions. Listing uses the
[NUL-delimited porcelain format](https://git-scm.com/docs/git-worktree/2.36.0),
so Git 2.36 or newer is required.

In Workspaces, `Enter` opens the selected checkout. `N` asks for a new directory
and new branch name, then creates a worktree from the current HEAD. `D` asks you
to type `remove` before removing the selected worktree. The current checkout,
primary checkout, dirty and locked worktrees cannot be removed. Branches are
retained, including their unique commits; removal never uses force.

`I` opens RecentRepositories: up to 50 recently opened repository/worktree roots,
most recent first. Enter opens the selected root, retaining unsaved work in both
checkouts. `D` forgets an entry without removing files or Git worktrees. Missing
or moved roots produce an error and retain the current checkout. Each working
tree keeps its own last view, graph toggle, history size and PR query/state/limit.
File/hunk and PR-review targets, drafts, search text, Seen marks and credentials
are never restored. A hunk/PR-file view resumes in Files with fresh data.

The TUI writes state after settled changes and on normal quit to
`$XDG_CONFIG_HOME/git-wirdo/state.json` (or `$HOME/.config/git-wirdo/state.json`)
on Unix, and `%APPDATA%\git-wirdo\state.json` on Windows. Override it with
`--state-file PATH` or `GIT_WIRDO_STATE_FILE`; `--no-state` disables persistence.
Writes use an OS lock and atomic replacement; concurrently opened clients retain
each other's repository entries. Changes to the same repository use the last
writer's settings. Damaged, unsupported or unreadable state is preserved and
disables saving for that session; save errors remain visible and can be retried.

The current directory (or explicit `--repo`) determines the checkout unless an
explicit lifecycle operation or the selection screen chooses a different root.
`--resume` explicitly opens the last saved root, including from outside a repo;
it reports missing paths rather than selecting a different checkout.
`--list-recent` lists saved roots without a TUI or Git mutations. Headless Git
inspection leaves state untouched. Saved history/PR limits are bounded to 10,000.

GitHub support uses the GitHub CLI (`gh`), including its stored authentication
and configured repository selection. Install it and authenticate with:

```bash
git-wirdo --github-login
# Or: gh auth login
```

Login runs outside the TUI so browser/device prompts remain usable. `G` shows
GitHub authentication status. Without gh or authentication, local Git views still
work and GitHub errors appear in the detail pane. `r` retries loading the current
view. With multiple remotes, configure `gh repo set-default` outside the TUI.
Inherited GH_REPO and repository-local Git environment variables are ignored so
commands address the checkout opened in Git Wirdo.

PullRequests starts with 100 open PRs. `+` requests another 100; `U` filters
open/closed/merged/all states and accepts a GitHub search query (such as
`author:@me label:bug`). Search/API result limits still apply. Selection follows
PR number across refresh. Details include checks, review decisions, mergeability,
reviews, conversation and inline comments, including outdated locations.

| Key | PR action |
| --- | --- |
| `Enter` | Checkout the PR; refuses dirty files or an unfinished merge/rebase |
| `V` | Show its patch; existing scrolling and search work here |
| `N` | Create a draft PR with title, body, base and published head branch |
| `A` | Submit an approval with the entered review body |
| `R` | Submit a request-changes review with the entered body |
| `C` | Post a conversation comment with the entered body |
| `M` | Confirm PR number, then choose merge / squash / rebase |
| `L` | Edit prefilled title, multiline body and base branch |
| `T` | Confirm draft ↔ ready transition with the PR number |
| `i` | Open files with old/new line numbers and inline commenting |

Each input uses Enter to advance and Escape to cancel; ordinary shortcut letters
are text while a prompt is open. `Ctrl-U` clears the current field; `Alt-Enter`
inserts a newline. Bracketed paste is accepted as text without executing shortcuts
(64 KiB per field). Edit fields are prefilled; an empty body explicitly clears it. Creation
requires a clean checkout and a head branch already published to GitHub; push it
first with your upstream configured. Creation does not implicitly push or fork.
Merge requests match the head SHA shown by the loaded PR list, so changes pushed
since refresh require refreshing before retrying. GitHub's checks, permissions
and protection rules apply; no admin bypass or branch deletion is requested.
Approvals and request-changes reviews use the selected immutable head commit and
reject a changed head before publishing. Edit forms reject changed metadata;
draft transitions recheck head and draft state. Reviews and comments are published
when their input is submitted.

In PrFiles, `j/k` selects a changed file; `C` asks for LEFT/RIGHT, a displayed old/new
line number and body. Files and inline comments follow all API pages. The chosen
head and exact file patch are rechecked before sending structured JSON; comments
are attached to that reviewed commit, including renamed/control-character paths.
Binary, missing, malformed or truncated patches cannot provide line targets.
GitHub's own file/patch limits apply; no line location is fabricated.

GitHubRepositories lists up to 100 repositories owned by the signed-in user.
Enter asks for a new clone directory, clones that repository and opens it. This
list does not include every organization or repository shared with the user.
GitHub requests run in the same cancellable background task as Git operations.
Repository creation is not implemented. CLI `--pr-files NUMBER` inspects the
line-numbered patches; `--list-prs` accepts `--pr-state`, `--pr-search`, and
`--pr-limit`. CLI `--pr NUMBER` includes paginated inline review comments.

Read-only CLI output is also available without an interactive terminal:

```bash
git-wirdo --list-workspaces
git-wirdo --list-prs
git-wirdo --pr 123
git-wirdo --list-github-repos
```

## Stashes

`S` opens a message prompt and saves tracked, staged and untracked changes;
ignored files are not included. Saving refuses an unfinished merge/rebase or
unresolved conflicts. Escape cancels without changing files.

In Stashes, selecting an entry shows its patch including saved untracked files.
`y` applies it after typing `apply`; `T` pops it after typing `pop`. Both restore
the saved index with `--index` and require a clean checkout. Apply keeps the
entry. Pop removes it only on successful restoration; conflicts retain it and
refresh the file/conflict views. Resolve stash conflicts and stage manually;
stash conflicts do not have a merge/rebase continuation operation.

`D` asks you to type `drop` to remove the selected entry. The selected stash SHA
and reflog selector are checked before mutations; if another process has changed
the list, refresh and reselect. No stash clear operation is exposed.
`git-wirdo --list-stashes` prints the list without a terminal.

## Remotes and tracking

In Remotes, `N` adds a name and URL. `L` edits the fetch URL; `H` edits the push
URL separately. Details show all configured fetch and push URLs. `D` requires
`remove` before removing the remote and its tracking refs; local branches and
commits remain. `f` fetches the selected remote in this view, and all remotes
elsewhere. Git credentials and SSH signing remain handled by your Git setup.

`W` asks which remote to publish the current branch to, pushes that branch
without force and sets its upstream. `U` sets the current branch's upstream;
submit an empty value to unset it. In RemoteBranches, Enter creates and switches
to a named local branch tracking the selected ref. Symbolic remote HEAD entries
are omitted. Failed switches retain existing work under Git's normal protections.

The header shows upstream and ahead/behind counts from locally fetched refs.
A missing/broken upstream produces an explicit tracking error instead of zero
counts. Fetch to refresh your knowledge of the remote before comparing.

`p` retains fast-forward-only pull. `l` asks for `ff-only`, `merge` or `rebase`;
these operations require a clean checkout and explicitly choose the strategy,
including when pull.ff is configured to only. Merge/rebase conflicts refresh the
existing Conflicts view for resolution, continuation or abort.

## Tags

In Tags, `N` asks for name, target commit (empty means HEAD), and annotation.
An empty annotation creates an unsigned lightweight tag; a nonempty annotation
creates an annotated tag using your Git signing configuration. Configure signing
credentials beforehand. Existing tags are never overwritten. The detail pane
shows the selected object and annotation/commit details.

`D` requires `delete` before deleting the local tag. Deletion compares its
selected object ID atomically, refusing a tag replaced since selection.
`W` publishes only the selected tag to an entered remote without force.
`X` asks for a remote and `delete` before removing the remote tag; its expected
object ID is protected by an explicit force-with-lease. A changed remote tag
is not deleted. Remote deletion retains the local tag, and local deletion never
contacts a remote. Working files and branches are unchanged by these actions.
`git-wirdo --list-tags` prints local tags without starting the TUI.

## Hunks and restoring files

In Files, `i` opens the selected file's Hunks view. `j`/`k` selects a hunk, `s`
stages an unstaged hunk, and `u` unstages a staged hunk. `d` switches the index
and working-tree comparisons; Tab returns to Files. The patch shown is the exact
Git patch used for the index operation. Worktree contents are unchanged.
If the selected diff has changed since it was loaded, refresh before retrying.
Context and no-final-newline markers are preserved, as are unusual path bytes.

Hunks are available for ordinary tracked text modifications, staged additions
and deletions. Binary files, untracked files, renames/copies and mode changes
use full-file stage/unstage instead. The upstream review comparison does not
expose staging or discard shortcuts.

In Files, `w` restores the working file from the index, retaining staged edits.
`D` restores both index and working file from the selected HEAD snapshot. For
an untracked file either action deletes only that selected file/symlink, never
its target or sibling files. Both require typing `discard`; Escape cancels.
The prompt shows the selected path and exact scope. File content, permissions,
index and HEAD changes since opening the prompt invalidate confirmation. Restore
refuses conflicted files; use the Conflicts view for those. Before the initial
commit, unstage a newly staged file instead of requesting a nonexistent HEAD.


## Reset and recovery

In History, `F` resets the current branch (or detached HEAD) to the selected commit.
Choose `soft`, `mixed` or `hard`, then type the full target commit ID:

- `soft` moves HEAD, retaining staged and working changes.
- `mixed` moves HEAD and resets the index to the target, retaining working files.
- `hard` also replaces tracked working files, discarding their changes.

Reset is refused during an unfinished operation or when HEAD, its branch, the
index, file status or affected content changes after opening the prompt. Hard
reset refuses untracked/ignored paths that obstruct the target tree. Move these
paths first; unrelated untracked/ignored files remain. Submodule working trees
are not recursively reset.

Before every reset, the previous HEAD is saved in a persistent local
`refs/git-wirdo/recovery/...` reference, shown in the result message. This protects
committed history from garbage collection; it does **not** save discarded
uncommitted changes. No force push is performed.

Reflog shows the current workspace's HEAD movements and persistent pre-reset references, including commits no longer
on a branch. `+` loads older entries. `N` creates a new named branch at the
selected immutable commit ID without switching checkout or touching local edits.
Existing branches cannot be overwritten. `--list-reflog` lists the first 100 HEAD entries plus up to 100 recovery references without starting the terminal UI. Linked workspaces have separate HEAD
reflogs; Git's configured reflog expiration still applies.


## Background tasks and cancellation

The terminal keeps rendering during Git, GitHub and hook execution. One task at
a time owns repository operations; additional mutations are rejected rather than
queued. Diff scrolling remains available while a task is running. The footer
shows elapsed time and the latest command diagnostic (including Git transfer
progress when available). Modal text entry remains immediate.

`Esc` requests cancellation. The process group on Unix, or assigned Job Object
on Windows, covers Git/GitHub and their ordinary descendants. A graceful stop is
attempted before forced termination. Commands are waited for, and output pipes
are drained before another mutation is accepted. `q`/`Ctrl-C` while busy cancel
and wait for cleanup before restoring the terminal and exiting.

Cancellation cannot undo a commit or remote write that already completed. After
cancellation, repository state is reloaded so partial changes and unfinished
operations can be inspected. Recovery refresh is also cancellable. A command
forcibly terminated while holding a Git lock can leave a lock file; investigate
its owner before removing it. No lock is removed automatically.

Repository validation and initial status loading occur before entering the TUI.
CLI inspection options remain synchronous. Windows code is checked by cross
compilation; native Windows cancellation still requires its CI/runtime checks.

## Terminal acceptance checks

On Unix, `cargo test --locked --test terminal_workflows --test terminal_github`
drives the real binary through a PTY, reconstructs its displayed screen, and
checks independent Git refs/index/working files after each workflow. It covers
staging, commits/amend, branches, selective hunks, restore, stash, merge/rebase,
cherry-pick/revert conflicts, remotes/tracking, tags, history/graph, reset/reflog
and linked worktree navigation/removal. Tests also check terminal-mode restoration.

GitHub acceptance uses a strict offline `gh` executable fixture and temporary
repositories. It checks CLI arguments, JSON stdin, multiline forms, pagination,
head/line guards, PR lifecycle and real local checkout/clone; it publishes no live
reviews/comments and does not prove server permissions or policy acceptance.
`terminal_background` separately checks hook cancellation and saved-state restart.
