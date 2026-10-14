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
| R7 | Two files have identical untracked previews; scrolling during background navigation is applied to the other file. | Transfer busy viewport changes only when selected object identity also matches. | Deterministic worker-completion regression in `session`. |

Selection identities retain native path bytes; Linux verifies a non-UTF8 path.
Stash/reflog selection uses commit identity; identical repeated reflog entries
cannot be distinguished by the current model. A failed refresh is transactional
inside the app, not a filesystem-wide atomic snapshot of concurrent Git changes.
Existing mutation-specific stale-state checks remain necessary.

## Measured changes

- The process runner initially polls after 1, 2, 4, 8 and 16 ms, then caps at the
  existing 20 ms interval. Long commands retain bounded polling and tree cleanup.
- Paging stops counting lines once the requested viewport is reached.
- Detail search scans lines without allocating a vector of every line.
- Transactional refresh deliberately retains a second app snapshot until success;
  this is a correctness cost, not a memory optimization.

Paired measurements and resource results are pending the final release builds;
they will be recorded before this draft is merged.
Build the baseline and changed revision with separate Cargo target directories
using `cargo build --locked --release --example performance`, preserve the old probe
binary, then `python3 scripts/measure.py OUTPUT_DIRECTORY OLD_PROBE_BINARY`.
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
No timing threshold is added to CI.

## Validation and remaining limits

Formatting, strict all-target/all-feature Clippy, the complete tests, and hosted
Linux/macOS/Windows CI are required for publication. Existing acceptance covers
palette text versus shortcuts, multiline paste, narrow terminals, long lists,
search wrap, conflict recovery, inherited pipes and graceful/forced cancellation.
The import terminal driver now waits for the completed refresh message rather than
a header already present before the input; Unix step deadlines match the Windows
20-second allowance for loaded hosts. Git-state assertions are unchanged.
The new terminal regression covers external insertion, unchanged scrolling and
independent Git index verification. Remote GitHub mutations were not repeated.

## Candidate next feature stage (requires separate selection)

These are absent from the inspected implementation and are not implemented here:

1. Filter local file and branch lists by name; retain existing detail/history search.
2. Stage or unstage selected lines within a text hunk, with exact-diff guards.
3. A guarded interactive rebase editor for reorder, squash and fixup, including recovery.

No cloud agents, second-account GitHub operations, stash clearing or recursive
submodule reset are included in this iteration.
