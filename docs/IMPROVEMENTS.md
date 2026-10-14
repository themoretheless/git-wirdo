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
