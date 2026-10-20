# Reliability, performance and terminal UX audit — 2026-10-14

This iteration implements the agreed priority: reliability and speed, then terminal
UX, then a separate feature backlog. It is a targeted audit, not a claim that all
possible defects have been found. No numeric change quota is used.

## Confirmed problems and acceptance

| ID | Reproduction before the change | Result | Regression evidence |
| --- | --- | --- | --- |
| R1 | Insert an alphabetically earlier file or a newer commit outside the client; refresh selects a different object by row number. | Refresh follows file paths, commit SHA and list identities. A removed entry falls back to a clamped row. | `refresh_identity`; Unix PTY and native Windows ConPTY independently verify that staging affects only the original file. |
| R2 | Refresh loads new status, then fails to load the upstream comparison; the visible state mixes two snapshots. | Publish the updated app only when every required list loads successfully. Git failure retains the previous complete snapshot. | `failed_refresh_preserves_the_entire_visible_snapshot`, index independently unchanged. |
| R3 | Scroll the selected file, then refresh its unchanged detail. | Keep both scroll axes only when object identity and detail text remain equal; changed/missing detail resets the viewport. | `refresh_identity`, PTY/ConPTY external insertion scenarios. |
| R4 | A worker panics; despite the recovery message a subsequent staging operation starts. | Block commands until successful refresh, retaining quit and viewport controls. Close obsolete prompts. | Injected worker panic; index remains unstaged until recovery succeeds. |
| R5 | Cancel a slow commit after its Git directory becomes temporarily unavailable; reload fails, then unstage is still accepted. | Failed cancellation recovery also blocks commands until successful refresh. | `background`: unchanged HEAD, index and working file, stopped hook descendant, explicit refresh then successful unstage. |
| R6 | Horizontal scrolling uses code-point count for wide CJK and combining marks. | Bound scrolling by rendered terminal cells using the same control escaping as rendering. | `horizontal_bounds_use_terminal_cells_for_wide_and_combining_characters`. |
| R7 | Two files have identical untracked previews; scrolling during background navigation is applied to the other file. | Transfer busy viewport changes only when repository root and selected object identity also match. | Deterministic worker-completion regressions for identical previews and different repositories in `session`. |

Selection identities retain native path bytes; Linux verifies a non-UTF8 path.
Stash/reflog selection uses commit identity; identical repeated reflog entries
cannot be distinguished by the current model. A failed refresh is transactional
inside the app, not a filesystem-wide atomic snapshot of concurrent Git changes.
Existing mutation-specific stale-state checks remain necessary.

## Measured changes

- Paging stops counting lines once the requested viewport is reached.
- Transactional refresh deliberately retains a second app snapshot until success;
  this is a correctness cost, not a memory optimization.

Paired measurements and environment are recorded in
[performance-2026-10-14.json](performance-2026-10-14.json), comparing `23ab607`
with accepted code `09d168d` using immutable executable copies.

| Fixture / probe | Before median / p95, ms | After median / p95, ms |
| --- | --- | --- |
| small / page_down_100 | 444.72 / 681.26 | 1.15 / 15.61 |
| small / session_scroll_100 | 1030.85 / 1689.17 | 1.21 / 38.48 |
| history / page_down_100 | 1196.92 / 1676.15 | 1.57 / 3.33 |
| history / session_scroll_100 | 731.78 / 2025.02 | 2.42 / 6.90 |
| files / page_down_100 | 625.57 / 1692.16 | 7.47 / 203.28 |
| files / session_scroll_100 | 538.85 / 1014.45 | 1.50 / 52.03 |

| Fixture | Before maximum RSS, MiB | After maximum RSS, MiB |
| --- | --- | --- |
| small | 27.44 | 30.38 |
| history | 30.02 | 29.42 |
| files | 16.17 | 16.91 |

These show a large paging improvement, including at p95. Other latency measurements
are noisy and sometimes worse; no overall startup/refresh/search improvement is
claimed. Maximum RSS increased on the small fixture and varied on others; no
memory saving is claimed. The retained complete snapshot is a correctness cost.

Build the baseline and changed revision with separate Cargo target directories
using `cargo build --locked --release --example performance`, preserve the old probe
binary, then `python3 scripts/measure.py OUTPUT_DIRECTORY OLD_PROBE_BINARY NEW_PROBE_BINARY`.
The script rejects identical executable hashes and records host load. It creates
disposable, identical repositories: 20 commits/10 files,
3000 commits/10 files, and 20 commits/3000 files, plus one large tracked file in each case. Normal
files have both staged and unstaged content. Startup uses the default 20-commit
window; refresh probes explicitly request up to 3000 commits. Probes use one
warmup and 15 samples; p95 is the largest of these 15 samples, not a tight
confidence estimate. Before/after run sequentially on each fixture, reversing
order for the history case; host load can still affect results.

Startup times `App::new` after locating the repository. Paging and search use a real diff of a tracked Unicode file with 30,000
changed lines (about 60,000 displayed diff lines). Session paging includes cloning the app. Cancel
timing includes a deliberate 25 ms delay before cancellation of a sleeping
helper; it is total operation time, not pure cancel-to-exit latency. On macOS
`/usr/bin/time -l` records resource usage separately for each probe process.
The `open_diff` probe navigates successive entries, clamping at the last one;
small fixtures therefore mix ordinary and large diff reloads.
Paging probes time App/Session handlers, excluding terminal drawing. The render
probe uses the initially selected ordinary file with warmed rendering caches;
large-diff frame latency is not measured by that probe.
No timing threshold is added to CI.

Rejected experiments: adaptive initial process polling and allocation-free detail
search did not show consistent latency improvements on the loaded host. They were
reverted; existing polling, cancellation and search algorithms are retained.
Startup/refresh/diff latency is reported without claiming an improvement.

## Validation and remaining limits

Formatting and strict all-target/all-feature Clippy passed. The full local suite
passed 219 tests. [Hosted acceptance](https://github.com/themoretheless/git-wirdo/actions/runs/38598225585)
for accepted code `09d168d` passed Linux 221, macOS 219 and Windows 189 tests.
The public API adds a read-only `App::detail_column()` getter; CLI flags and the
persisted settings format retain compatibility. Existing acceptance covers
palette text versus shortcuts, multiline paste, narrow terminals, long lists,
search wrap, conflict recovery, inherited pipes and graceful/forced cancellation.
The import terminal driver now waits for the completed refresh message rather than
a header already present before the input; Unix step deadlines match the Windows
20-second allowance for loaded hosts. Git-state assertions are unchanged.
The new terminal regression covers external insertion, unchanged scrolling and
independent Git index verification. Remote GitHub mutations were not repeated.

## Selected feature stage — implemented

The user selected all three candidates. The follow-up implements:

1. Filter local file and branch lists by name; retain existing detail/history search.
2. Stage or unstage selected lines within a text hunk, with exact-diff guards.
3. A guarded interactive rebase editor for reorder, squash and fixup, including recovery.

No cloud agents, second-account GitHub operations, stash clearing or recursive
submodule reset are included in this iteration.


| ID | Previous limitation | Change | Regression evidence |
| --- | --- | --- | --- |
| F1 | Files/branches only had detail search. | Backslash opens a name filter, visible rows retain original identities; hidden selections cannot be mutated. | `editing`, Unix PTY and Windows ConPTY check index against the selected filename. |
| F2 | Only complete text hunks could be staged. | `=` selects numbered changed lines; rebuild counts and context, preserve byte content/no-newline markers; exact original diff is rechecked. | `editing` tests replacements, insertions, deletions, independent unstaging, missing newline and stale snapshots; PTY/ConPTY independently inspect index. |
| F3 | Rebase could only integrate a branch. | `~` edits a bounded linear todo after selected base, supports reorder/squash/fixup, validates immutable IDs and HEAD/branch, retains recovery ref. | `editing` covers fixup/reorder, invalid todos, dirty/detached refusal, conflict abort/continue; PTY/ConPTY drive the actual editor. |

Limits: one active name filter, replaced when filtering another list; filters are
not persisted. Line selection excludes structural file creation/deletion,
rename/mode/binary patches. The rebase editor handles 1–200 commits after a
base (the root commit itself is not rewritten), requires an attached clean branch, and excludes merge commits,
drop/edit/exec. It runs the existing cancellable Git process and preserves Git's
in-progress state on conflicts. Squash uses Git's combined messages without
opening an external editor. No new performance improvement is claimed here.

Windows follow-up: the CI wrapper now emits UTF-8 even when redirected Python
stdout defaults to cp1252, preserving Unicode terminal failure diagnostics and
the command's exit code. The todo editor also accepts `;` separators for native
Windows input, whose crossterm backend does not emit Unix bracketed-paste events.
The ConPTY scenario uses this native input path; Unix PTY still checks multiline
bracketed paste.


## Visual editing and selected stash follow-up

| ID | Reproduction / gap | Change | Verification |
| --- | --- | --- | --- |
| V1 | Line staging required manually typing ordinals. | Space opens a cursor/range editor, with selected side-specific s/u and exact-diff guards. Structural patches retain whole-hunk operations. | `visual_editing`, 20,000-change narrow-window rendering, Unix PTY and Windows ConPTY independently inspect index and working files. |
| V2 | Rebase required rewriting raw full-ID todo text. | Visual reorder, pick/squash/fixup controls, abbreviated IDs/subjects, group preview and typed confirmation; raw palette remains available. | Guarded repository tests plus visual cancellation/confirmation/reorder tests and native terminal scenarios. |
| V3 | Filtering another list replaced the previous filter and required Enter. | Independent Files/Branches filters update matches while typing without Git reads; Enter/Esc loads detail; Esc outside input clears active filter. | Independent live-filter/state and shortcut-input tests, PTY/ConPTY. Filters remain transient per repository. |
| V4 | Stash saved everything; native pathspec stash also captures unrelated staged changes in its trees. | A private index builds selected-only index/working/untracked stash trees, stores recovery copy before guarded selected cleanup. | Real Git tree/index/file oracles cover mixed staged/working/untracked, renames, deletions, added/ignored files, stale requests and injected pre/post-store failure. Linux also tests native non-UTF8 paths. |

No latency improvement is claimed. The line viewport builds only visible rows;
existing detail/list rendering is skipped behind editors. Rebase preview describes
commit groups rather than predicting conflicts or resulting SHAs. Selected stash
requires HEAD and regular files/symlinks, refuses intent-to-add and submodules,
and does not promise atomic cleanup against concurrent external Git/filesystem
changes. Cancellation or cleanup failure after store retains a recoverable stash.
Worker-panic/failed-reload recovery closes editors so they cannot intercept refresh.
Earlier F1/F3 limitations above describe the previous accepted package; this
follow-up replaces its single filter and raw-only todo interface.

The concurrent auto-reload change from PR #6 is included. Polling explicitly waits
while either visual editor is open, then resumes after close; captured selections
still use exact-diff/HEAD checks at mutation. The integration regression checks
both editors, retained drafts and the subsequent external-file reload.

Validation note: local checks use installed Rust 1.98.1 because default nightly
Cargo became unavailable during the run. Parallel local PTY runs reached the
unchanged 20-second deadline while Git was still running; all 20 terminal workflows
passed sequentially. The large Unicode snapshot exposed a test-reader defect:
continuation cells of wide graphemes were interpreted as spaces. The reader now
advances by terminal cell width, retaining exact-text assertions and adding an
independent wide/combining/emoji/real-space decoding check.

The ConPTY flow returns from Hunks through the existing View files palette entry;
Tab cycles through Reflog and other views instead of jumping directly to Files.
The Hunks footer now describes that navigation accurately. Git/index/stash oracles
and terminal deadlines are unchanged.


## Reliability restart — 2026-10-21

The accepted 2026-10-14/15 work is retained. Workspaces terminology is now
Worktrees throughout the UI, with the old CLI flag and persisted view spelling
kept compatible. The following are reproduced defects, not hypothetical features.

| ID | Reproduction before fix | Change | Evidence |
| --- | --- | --- | --- |
| R8 | Open removal confirmation, switch the linked tree to another branch externally, then confirm. The replacement checkout was deleted. | Capture branch, HEAD and canonical Git directory before confirmation; revalidate on submit. Preparation runs in the cancellable worker. | `removal_confirmation_rejects_a_different_branch_at_the_selected_path`; separate HEAD-movement test; Unix PTY repeats external branch change. |
| R9 | Create an ignored directory containing personal notes in an otherwise clean linked tree; remove it. Native Git deletes ignored contents even without force. | Inspect ignored and untracked paths before deletion and refuse nonempty status. | `ignored_files_are_retained_when_removing_a_worktree`; Unix PTY verifies ignored contents remain. |
| R10 | Mark a tracked file assume-unchanged, edit it, remove the tree. Native Git deletes the hidden edits. | Refuse removal with assume-unchanged or skip-worktree index entries. Real index flags are not modified. | `assume_unchanged_edits_are_not_deleted_with_a_worktree`. |
| R11 | Retain CHERRY_PICK_HEAD while files/index are clean, then remove the tree. Native Git discards the operation state. | Check per-worktree merge/rebase/am/cherry-pick/revert state before removal. | `unfinished_operation_is_not_discarded_with_a_clean_worktree` verifies marker and files retained. |

The four reproductions failed before their respective fixes. Additional tests
cover a new lock or untracked file after confirmation and preservation of branch
refs/unique commits. This is not filesystem-wide exclusion: an external process
can still change files after the final check. No force removal is used, and no
existing user branch or worktree is deleted by development work.

### Fresh baseline

[Raw probes and environment](performance-2026-10-21.json) record release Rust
1.98.1 on macOS arm64, one warmup and 15 samples per probe. The original runner's
`rustc` field reports the default shell toolchain; `probe_build_rustc` records the
actual compiler used to build the executable. The same existing probe and
fixtures were used. The host was concurrently running validation and other work;
load is recorded. These are baseline measurements, not an optimization result.

| Fixture | Startup median / p95 ms | Refresh median / p95 ms | Open diff median / p95 ms | Search median / p95 ms | Cancel total median / p95 ms | Max RSS MiB |
| --- | --- | --- | --- | --- | --- | --- |
| 20 commits / 10 files | 136.23 / 154.28 | 138.29 / 152.42 | 27.98 / 41.64 | 1.44 / 1.52 | 61.96 / 62.95 | 22.69 |
| 3000 commits / 10 files | 159.54 / 255.80 | 142.95 / 178.99 | 29.14 / 44.77 | 1.45 / 1.70 | 58.99 / 62.58 | 24.06 |
| 20 commits / 3000 files | 420.67 / 682.40 | 681.91 / 957.69 | 54.89 / 70.12 | 2.75 / 4.63 | 57.46 / 64.16 | 16.97 |

Cancel includes the intentional 25 ms wait. The ordinary render probe does not
measure large-diff terminal drawing; open_diff advances/clamps through file rows.
15-sample p95 is the largest observation. Startup times App construction after
repository discovery. No startup, refresh or memory improvement is claimed.

### Remaining audit / feature queue

This register is not 2000 completed improvements. Remaining investigations include
fault injection for disk-full/permission errors and process exit at every selected
stash cleanup boundary; reproducible large-diff drawing/input latency; and broader
cross-platform filesystem races. Existing injected stash failures, cancellation,
panic recovery, stale index/HEAD, snapshot refresh and terminal tests remain
required gates. Feature delivery follows reliability gates: worktree branch/base
selection and lock/move, then rebase reword/edit/drop and conflict-view design.


## Worktree management follow-up — 2026-10-21

| ID | Previous gap | Change | Evidence |
| --- | --- | --- | --- |
| W1 | Creation always made a new branch at HEAD. | Palette forms create at an explicit resolved commit/ref or check out an existing local branch. No force checkout. | Real Git tests inspect both HEADs, source index/files, occupied branch refusal and invalid-base refusal; Unix PTY drives both forms. |
| W2 | An occupied branch failed checkout without showing its location. | Branch details show the linked checkout path. | Worktree branch detail regression. |
| W3 | Worktree locks required a separate terminal. | Palette lock with reason and typed unlock confirmation, revalidating selected path/branch/lock state. | Git porcelain oracle, stale-lock refusal, Unix PTY and native Windows ConPTY flow. |
| W4 | Moving a linked checkout required a separate terminal. | Palette destination form and checked target snapshot, normal Git move without force. | Independent staged/unstaged/untracked content and Git registration checks; stale move refusal; real Unix PTY. |

Current/primary/locked trees and trees with unfinished Git operations cannot be moved. Moving the active checkout is not
supported. Old recent-root settings are retained at the old path until explicitly
forgotten; navigation can open the destination and save its own settings. Native
Git controls submodule and destination restrictions. No new shortcut or configuration
format is added. Lock reasons are stored by Git; the current compact model displays
lock state but not the reason. No performance improvement is claimed.

## Visual rebase actions — 2026-10-21

| ID | Previous gap | Change | Evidence |
| --- | --- | --- | --- |
| E1 | Rewording required an external editor. | `r` opens the complete original message; replacement text preserves Unicode, multiline bodies and literal shell characters. | Real Git message/body/tree/recovery oracles and visual/Unix PTY/native ConPTY scenarios. |
| E2 | No edit stop in the visual todo. | `e` marks edit; successful pause opens Conflicts with an explicit paused message, then existing amend/continue/abort commands apply. | Independent rebase-state checks and continuation in a fresh Git process. |
| E3 | Every commit had to be retained as content. | `d` explicitly marks drop; preview labels dropped commits. All prepared IDs still appear exactly once; at least one retained commit and a valid first retained action are required. | Missing/duplicate/all-drop rejection before ref mutation; file/tree/count/recovery tests. |
| R12 | A child can emit LeaveAlternateScreen after the PTY driver's last drain but before try_wait reports exit. macOS CI reproduced an incorrect alternate-screen assertion. | Drain trailing output after observing child exit, then check terminal restoration without weakening assertions. | Original macOS CI failure and subsequent terminal suites. |

Reword compiles into a generated pick/amend exec step with octal-encoded bytes;
user shell commands remain forbidden. The generated command lives in Git's todo,
so a later application restart/continue keeps replacement messages. No user text
or path is interpolated as executable source. Plans are bounded to 24 KiB for
portable environment transport. This is a Git amend operation and may execute
configured Git commit hooks. Existing clean/attached/linear/1–200-commit guards,
HEAD/branch checks and persistent recovery refs remain in place. No support for
arbitrary exec, merge-preserving rebase, root rewriting or dropping every commit
is claimed. Conflict prediction remains outside the preview.

## Next conflict-view design (specification, not implemented)

The current conflict detail is a combined Git diff. The next read-only preview
should load exact stage 1/2/3 entries with literal NUL-delimited paths, then read
blobs by their object IDs. Label them Base, Ours and Theirs, including the active
Git operation so rebase's side meanings are clear. Missing stages mean deletion;
binary contents and symlinks require explicit metadata views. Start with a
switchable single pane for narrow terminals, then optionally a wide layout.
Keep working-file editing and staging explicit. Any future block-resolution
mutation must revalidate index stages and the working-file snapshot and have
independent Git/index/file tests for modify/modify, add/add, modify/delete,
rename conflicts, stale selection and abort. A preview is not a merge solver.

## Special-repository audit matrix

| Scenario | Current inspected implementation | Remaining validation / work |
| --- | --- | --- |
| Submodules | Native Git status; selected-only stash rejects directories; reset avoids recursive submodule reset. | Dedicated submodule views, nested state, dirty/untracked child preservation and explicit update policies are not implemented. |
| Sparse checkout | Native Git behavior; worktree removal now refuses skip-worktree entries. | No sparse-pattern editor or sparse-index acceptance matrix. Test absent paths, expansion, staging and checkout before enabling an editor. |
| Shallow / partial clone | Generic clone/open relies on Git. | No dedicated shallow-depth/filter controls or missing-object/offline/automatic-fetch acceptance matrix. |
| Git LFS | Git filters/hooks may participate in native operations. | No LFS transfer UI, pointer/working-content distinction or offline/missing-object/error acceptance matrix. |

These rows describe inspected code, not successful end-to-end support. They are
candidates for the separately scoped next feature packages; no account/cloud
agent requirement is introduced.
