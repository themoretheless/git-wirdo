# Git Wirdo

A lightweight Git client written in Rust with a terminal UI for the core checkout workflow: status overview, file staging, branch switching, commit, fetch, pull, push, commit history, and merge/rebase conflict handling.

## Run

```bash
cd /Users/themoretheless/Documents/Sources/git-wirdo
cargo run -- --repo /path/to/repo
```

## Keys

- `tab` switch between files, history, branches, and conflicts
- `j`/`k` or arrow keys move the selection
- `enter` checkout the selected branch in branch view
- `s` stage the selected file
- `u` unstage the selected file
- `c` create a commit with the default message `wirdo update`
- `b` create or switch to the `feature/wirdo` branch
- `f` fetch remotes
- `p` pull from upstream
- `P` push to upstream
- conflict view: `o` take ours, `t` take theirs, `a` mark resolved, `e` continue, `k` skip rebase step, `x` abort
- `q` quit
