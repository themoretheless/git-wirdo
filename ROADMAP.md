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

## Beyond the initial acceptance plan

- Added literal per-file history with patches and rename following, and committed
  HEAD line authorship. Available in the Files detail pane and through CLI for
  clean/deleted historical paths. Inspection preserves the index and working copy;
  mutation/review shortcuts require returning to diff.
- New repository and CLI tests independently check origin commit attribution,
  rename history, commit limits, literal paths, pending work and mode conflicts.
  Real Unix and Windows terminal scenarios exercise inspection and return to diff.
- History/authorship verification passed on all three hosted platforms; earlier
  acceptance numbers above describe the base client before these additions.

- Added the read-only tracked-file browser (`J`) and CLI listing, including clean
  paths, working-copy previews, history and authorship. Pending renames retain
  their original HEAD inspection path, and editing requires returning to changes.
- Added immutable commit export (`!` in History and `--export-patch --output`),
  preserving metadata/message, rename and binary patches. Root commits work;
  merge commits and existing destinations are refused. Independent `git am`
  round trips verify resulting contents and commit messages while retaining the
  source's pending changes and index. Unix and native Windows terminals cover
  browser-to-history/blame navigation, form submission and patch application.
- [Expanded client acceptance](https://github.com/themoretheless/git-wirdo/actions/runs/37620351731)
  passed formatting, strict all-target Clippy and all tests on Linux (192), macOS
  (191), and Windows (163). Native ConPTY tests exercise both extensions and apply
  the exported patch with Git. The full 191-test macOS suite and strict Clippy
  also passed on nightly; all added repository/CLI and terminal scenarios passed.

Expanded acceptance evidence:

| Added capability | Independent evidence | Real terminal evidence |
| --- | --- | --- |
| File history and HEAD authorship | `file_inspection`: rename following, origin commits, commit limits, clean/deleted paths, staged rename, literal names and CLI validation | `file_history_and_blame_are_read_only_and_return_to_working_diff`; `native_windows_file_history_and_blame_keep_pending_work_and_index_unchanged` |
| Clean tracked-file browsing | `extended_client`: contents, original rename path and preserved index/working files; CLI listing | `clean_file_browser_and_commit_export_are_available_through_the_terminal`; `native_windows_browses_clean_files_and_exports_a_patch_usable_by_git_am` |
| Portable commit export | `extended_client`: independent root/rename/binary patch application, commit messages, preserved source state, overwrite and merge refusal | Both browser/export terminal scenarios apply the actual saved patch through `git am` |

## Command palette and mail import

- Added `:` searchable command palette with contextual actions, direct view
  navigation, arrow selection and existing guarded confirmation forms.
- Added `@` / `--import-patch` mail/mbox import through `git am --3way`, preserving
  source authorship and messages. Import failure is distinguished from rebase;
  conflict resolution, continue and abort use the native mail operation. Abort
  restores the pre-import HEAD even after a partly applied series.
- Repository/CLI tests cover dirty refusal, manual resolution, series abort,
  authorship, rebase-apply classification, palette modal input and tracked-browser
  hunk safety. Unix PTY and Windows ConPTY scenarios cover palette-to-form import
  and both conflict recovery paths. Hosted platform verification remains pending
  this change's own CI run.
