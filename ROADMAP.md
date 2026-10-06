# Full Git client acceptance

The goal is a usable daily Git client within the existing Rust TUI. Passing tests
for one milestone does not complete this goal. All entries require user-visible
operations, recoverable failures, and verification using temporary repositories.

Implemented foundations:

- Repository status, file staging, unstaging, previews and literal paths.
- Editable commits and branch names; amend, rename and guarded deletion.
- Merge/rebase start, conflict resolution, continuation and abort.
- Worktree creation, opening, guarded removal and opening other repositories.
- GitHub authentication, repository listing/cloning and basic PR lifecycle.
- Upstream comparison, diff scrolling/search and session review progress.
- Stash list/save/inspect/apply/pop/confirmed drop, including index and untracked
  restoration, conflict retention and stale-selector checks.

- Remote configuration, separate fetch/push URLs, branch publication/upstream,
  remote tracking checkout, ahead/behind state and explicit pull strategies.

- Lightweight/annotated tags: list, inspect, create, publish and guarded local
  and remote deletion, including compare-and-swap/lease protection.

- Selective tracked-text hunk staging/unstaging with exact-diff validation, and
  confirmed index/HEAD restoration with file/index snapshots and symlink handling.

- Expandable cross-branch history and metadata search; confirmed cherry-pick and
  revert (including explicit merge mainline), conflict continuation/abort.

- Git branch/merge graph, explicit soft/mixed/hard reset with stale-confirmation
  and untracked obstruction checks, persistent pre-reset HEAD references, and
  workspace HEAD reflog inspection/recovery into a new branch.

- Serialized background Git/GitHub tasks, streamed diagnostics/elapsed time,
  mutation exclusion, cancellable recovery refresh, and graceful/forced process
  cleanup on Unix / Windows Job Objects. Unix hooks and inherited pipes verified.

- Prefilled/multiline PR editing, guarded draft/ready transitions, PR query/state
  filtering and incremental lists, selectable merge method, head-pinned reviews,
  paginated file/inline-comment inspection and validated inline comment publication.
  Remote mutation requests are fixture-tested; no live review/comment was posted.

- Persistent recent repository/worktree navigation and per-root views, history
  graph/size and PR filters; explicit resume and persistence opt-out. Atomic,
  locked state merging preserves other clients' roots and damaged settings.
  Restart and damaged-state behavior verified through a real Unix PTY.

- Primary Unix terminal workflows verified using the actual binary, a reconstructed
  VT screen, and independent Git index/files/refs: staging/commits/branches, hunks
  and restore, stash, merge/rebase/cherry-pick/revert conflict resolution and abort,
  remotes/tracking/tags, history/patch/graph/search, reset/reflog, and worktrees.
  Offline GitHub CLI acceptance covers PR forms, paginated diffs/comments,
  head/line guards, creation/checkout, repository listing and local clone.

- Repository lifecycle: initialize while retaining ordinary files, clone arbitrary
  Git URLs/local repositories, and choose/open/create/clone outside a checkout.
  Recent-root operations and the startup screen are covered through a real Unix
  terminal, including recoverable errors, cancellation and terminal restoration.

- Linux aarch64 runtime acceptance (Rust 1.98.0, Git 2.39.5): strict all-target
  Clippy and all 183 tests passed, including real PTY workflows, background hooks,
  lifecycle cancellation and non-UTF8 filesystem paths. macOS terminal workflows
  passed again after fixing the platform-specific PTY argument type.

Verified acceptance:

- [Hosted platform matrix](https://github.com/themoretheless/git-wirdo/actions/runs/37524510965)
  passed formatting, strict all-target Clippy and all tests: Linux 183, macOS 182,
  Windows 154. Native Windows Job Object fixtures verify cancellation and cleanup
  after normal parent exit with inherited pipes. Native ConPTY fixtures verify
  Unicode literal staging, commit/branch controls, resize, clone/worktree navigation,
  Git-hook cancellation and alternate-screen restoration.
- Rust nightly 1.101.0 (282215592 2026-10-04) passed formatting, strict Clippy,
  build and all 182 macOS tests.

Acceptance evidence by capability:

| Capability | Independent repository/transport checks | Terminal workflow checks |
| --- | --- | --- |
| Status, literal paths, staging, commits and branches | `git_workflows`, `app_behavior`, `branch_actions` | `terminal_workflows`, `windows_terminal` |
| Hunks and guarded file restoration | `hunks`, `restore` | `terminal_workflows` |
| Merge/rebase/cherry-pick/revert and conflict recovery | `git_workflows`, `branch_actions`, `history_actions` | `terminal_workflows` |
| Remotes, upstream, pull strategies and tags | `remotes`, `tags`, `review` | `terminal_workflows` |
| Stashes with index/untracked restoration | `stashes` | `terminal_workflows` |
| History, graph, reset backups and reflog recovery | `history_actions`, `reset_reflog` | `terminal_workflows` |
| Worktrees, open/init/clone and recent-root settings | `workspaces`, `lifecycle`, `settings`, `cli` | `terminal_lifecycle`, `terminal_background`, `windows_terminal` |
| GitHub repository/PR lifecycle, editing and inline review | GitHub transport/patch tests and strict offline CLI fixture | `terminal_github` |
| Background serialization, cancellation and process cleanup | `background`, `windows_process` | `terminal_background`, `terminal_lifecycle`, `windows_terminal` |

Known current limits remain documented in README. GitHub mutations are verified
with an offline CLI fixture, not live review/comment publication. This is a Git
client; Delta's agent threads, models and cloud collaboration are not implemented.
