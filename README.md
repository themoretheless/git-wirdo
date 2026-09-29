# Git Wirdo

A lightweight Git client written in Rust with a terminal UI for status, staging,
branch switching, commits, fetch/pull/push, history, and merge/rebase conflicts.

## Run

Install a current stable Rust toolchain and Git 2.28 or newer, then from this checkout:

```bash
cargo run --locked -- --repo /path/to/repo
```

`--repo` defaults to the current directory. A subdirectory of a working tree is
also accepted; commands operate relative to the repository root. Linked worktrees
and repositories with no commits are supported. Bare repositories are not.

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
| `Tab` | Any | Cycle files → history → branches → conflicts |
| `j` / `k`, `↓` / `↑` | Any | Move the selection; the list follows it |
| `r` | Any | Refresh repository state and the detail pane |
| `q`, `Ctrl-C` | Any | Quit and restore the terminal |
| `s` / `u` | Files | Stage / unstage the selected file |
| `c` | Any | Commit staged changes using `wirdo update` |
| `b` | Any | Create or switch to `feature/wirdo` |
| `Enter` | Branches | Switch to the selected local branch |
| `f` | Any | Fetch all remotes |
| `p` | Any | Pull from upstream, **fast-forward only** |
| `P` | Any | Push using the configured upstream |
| `o` / `t` | Conflicts | Take ours / theirs and stage the result |
| `a` | Conflicts | Stage a file after resolving it manually |
| `e` | Conflicts | Continue the current merge or rebase |
| `K` | Conflicts | Skip the current rebase commit |
| `x` | Conflicts | Abort the current merge or rebase |

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
  Spaces, quotes, newlines, renames, Unicode, and (on Unix) non-UTF-8 filename
  bytes are preserved. Control characters are escaped for display only.
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
not available inside the TUI. Network operations and hooks are currently
synchronous and may block the interface while they run.

This refactor deliberately retains the existing terminal layout and the default
commit message/branch shortcut. Editable commit/branch dialogs, confirmation
prompts for destructive actions, diff paging, and background Git jobs are separate
follow-up work, not included in this pass. History currently shows the latest 20
commits. For long patches, use `git diff` / `git show` outside the UI.

## Code layout

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
