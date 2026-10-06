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

Remaining acceptance work:

- Complete native Windows runtime verification of background process-tree
  cancellation and cleanup after parent exit. Native process-handle fixtures are
  prepared and cross-compile; cross compilation alone does not prove behavior.

- Successful hosted CI checks for the platform matrix remain a regression gate;
  current Unix runtime evidence is local macOS and Linux execution. Native Windows
  filename/terminal behavior remains unverified.

Known current limits are documented in README. Keep this list honest as features
are implemented; do not mark an item complete based only on a method or key binding.
