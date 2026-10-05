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

Remaining acceptance work:

- Complete native Windows runtime verification of background process-tree
  cancellation (cross compilation alone does not prove behavior).
- GitHub PR editing, ready-for-review, inline review comments, pagination/filtering
  and selectable merge strategy, without bypassing checks or protection rules.
- Persist repository/workspace navigation and useful view settings.
- End-to-end TUI verification for each primary workflow; stable CI checks and
  platform-specific filename/terminal behavior remain regression requirements.

Known current limits are documented in README. Keep this list honest as features
are implemented; do not mark an item complete based only on a method or key binding.
