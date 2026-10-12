//! Commits: history, mail patches, blame, reflog, reset and in-progress operations.
use super::*;

impl Repository {
    pub fn history_graph(&self, limit: usize) -> Result<String> {
        if !self.has_head()? {
            return Ok(String::new());
        }
        self.text(&[
            "log",
            "--no-color",
            "--no-show-signature",
            "--all",
            "--topo-order",
            "--graph",
            "-n",
            &limit.to_string(),
            "--decorate=short",
            "--format=%H %s %d",
        ])
    }

    pub fn history(&self, limit: usize) -> Result<Vec<CommitEntry>> {
        let limit = limit.to_string();
        if !self.has_head()? {
            return Ok(Vec::new());
        }
        let bytes = self.git([
            "log",
            "--no-color",
            "--no-show-signature",
            "-z",
            "-n",
            &limit,
            "--all",
            "--topo-order",
            "--date=short",
            "--format=%H%x00%h%x00%ad%x00%an%x00%s",
        ])?;
        let text = String::from_utf8_lossy(&bytes);
        let fields: Vec<_> = text.split_terminator('\0').collect();
        let (records, remainder) = fields.as_chunks::<5>();
        ensure!(remainder.is_empty(), "Invalid commit record from Git");
        Ok(records
            .iter()
            .map(|parts| CommitEntry {
                sha: parts[0].to_owned(),
                short_sha: parts[1].to_owned(),
                date: parts[2].to_owned(),
                author: parts[3].to_owned(),
                subject: parts[4].to_owned(),
            })
            .collect())
    }

    pub fn import_mail_patch(&self, path: &Path) -> Result<()> {
        self.ensure_clean()?;
        ensure!(!path.as_os_str().is_empty(), "Select a mail patch file");
        let path = if path.is_absolute() {
            path.to_owned()
        } else {
            self.root.join(path)
        };
        ensure!(
            fs::metadata(&path)
                .with_context(|| format!("Cannot read patch {}", display_path(&path)))?
                .is_file(),
            "Patch input must be a regular file"
        );
        self.git([
            OsStr::new("am"),
            OsStr::new("--3way"),
            OsStr::new("--no-rerere-autoupdate"),
            OsStr::new("--"),
            path.as_os_str(),
        ])?;
        Ok(())
    }

    pub fn current_mail_patch(&self) -> Result<String> {
        self.text(&["am", "--show-current-patch"])
    }

    /// A self-contained mail patch for one non-merge commit; never overwrite a file.
    pub fn export_commit(&self, revision: &str, destination: &Path) -> Result<PathBuf> {
        ensure!(!revision.is_empty(), "Select a commit to export");
        let target = format!("{revision}^{{commit}}");
        let sha = self.text(&["rev-parse", "--verify", "--end-of-options", &target])?;
        let sha = sha.trim();
        let parents = self.text(&["rev-list", "--parents", "--max-count=1", sha])?;
        ensure!(
            parents.split_whitespace().count() <= 2,
            "Merge commits cannot be exported as a single mail patch; select an ordinary commit"
        );
        let bytes = self.git([
            "format-patch",
            "--stdout",
            "--root",
            "--no-signature",
            "--no-numbered",
            "--no-cover-letter",
            "--no-notes",
            "--no-ext-diff",
            "--no-textconv",
            "--binary",
            "--full-index",
            "-1",
            sha,
            "--",
        ])?;
        ensure!(!bytes.is_empty(), "Git did not produce a mail patch");
        ensure!(
            !destination.as_os_str().is_empty(),
            "Enter a patch destination"
        );
        let path = if destination.is_absolute() {
            destination.to_owned()
        } else {
            self.root.join(destination)
        };
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .with_context(|| {
                format!(
                    "Cannot create patch {}; existing files are never overwritten",
                    display_path(&path)
                )
            })?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .with_context(|| {
                format!(
                    "Cannot finish patch {}; inspect the partial file",
                    display_path(&path)
                )
            })?;
        Ok(path)
    }

    /// Committed history only; Git follows the selected literal path across renames.
    pub fn file_history(&self, path: &Path, limit: usize) -> Result<String> {
        validate_inspection_path(path)?;
        ensure!(
            (1..=10_000).contains(&limit),
            "File history limit must be 1..=10000"
        );
        self.path_text(
            &[
                "log",
                "--follow",
                "--no-color",
                "--no-show-signature",
                "--no-ext-diff",
                "--no-textconv",
                "--format=fuller",
                "--patch",
                "--stat",
                &format!("--max-count={limit}"),
                "HEAD",
            ],
            &[path],
        )
    }

    /// Inspect HEAD, leaving pending edits and the index untouched.
    pub fn file_blame(&self, path: &Path) -> Result<String> {
        validate_inspection_path(path)?;
        self.path_text(
            &[
                "blame",
                "--no-textconv",
                "--root",
                "--date=short",
                "--abbrev=12",
                "HEAD",
            ],
            &[path],
        )
    }

    pub fn commit_detail(&self, commit: &CommitEntry) -> Result<String> {
        self.text(&[
            "show",
            "--no-color",
            "--no-show-signature",
            "--no-ext-diff",
            "--no-textconv",
            "--stat",
            "--patch",
            "--decorate=short",
            "--format=fuller",
            &commit.sha,
            "--",
        ])
    }

    pub fn commit(&self, message: &str) -> Result<()> {
        ensure!(!message.trim().is_empty(), "Commit message cannot be empty");
        self.git(["commit", "-m", message])?;
        Ok(())
    }

    pub fn amend(&self, message: &str) -> Result<()> {
        ensure!(!message.trim().is_empty(), "Commit message cannot be empty");
        self.git(["commit", "--amend", "-m", message])?;
        Ok(())
    }

    pub(super) fn verify_commit_id(&self, sha: &str) -> Result<()> {
        ensure!(
            matches!(sha.len(), 40 | 64) && sha.bytes().all(|b| b.is_ascii_hexdigit()),
            "Select a full commit ID"
        );
        let target = format!("{sha}^{{commit}}");
        ensure!(
            self.text(&["rev-parse", "--verify", "--end-of-options", &target])?
                .trim()
                == sha,
            "Selected commit changed"
        );
        Ok(())
    }

    pub fn reflog(&self, limit: usize) -> Result<Vec<crate::model::ReflogEntry>> {
        let bytes = if self.has_head()? {
            self.git([
                "reflog",
                "show",
                "--no-color",
                "--no-show-signature",
                "-z",
                "-n",
                &limit.to_string(),
                "--format=%H%x00%gD%x00%gs",
                "HEAD",
                "--",
            ])?
        } else {
            Vec::new()
        };
        let text = String::from_utf8_lossy(&bytes);
        let fields: Vec<_> = text.split_terminator('\0').collect();
        let (records, remainder) = fields.as_chunks::<3>();
        ensure!(remainder.is_empty(), "Invalid reflog record from Git");
        let mut entries: Vec<_> = records
            .iter()
            .map(|p| crate::model::ReflogEntry {
                sha: p[0].into(),
                selector: p[1].into(),
                subject: p[2].into(),
            })
            .collect();
        // Persistent pre-reset references remain discoverable even after HEAD reflog expires.
        let backups = self.text(&[
            "for-each-ref",
            "--sort=-refname",
            &format!("--count={limit}"),
            "--format=%(objectname)%00%(refname)%00%(subject)",
            "refs/git-wirdo/recovery/",
        ])?;
        for line in backups.lines() {
            let fields: Vec<_> = line.split('\0').collect();
            ensure!(
                fields.len() == 3,
                "Invalid recovery reference record from Git"
            );
            entries.push(crate::model::ReflogEntry {
                sha: fields[0].into(),
                selector: fields[1].into(),
                subject: fields[2].into(),
            });
        }
        Ok(entries)
    }

    pub fn recover_commit(&self, sha: &str, branch: &str) -> Result<()> {
        self.verify_commit_id(sha)?;
        ensure!(
            !branch.trim().is_empty(),
            "Enter a new recovery branch name"
        );
        self.git(["check-ref-format", "--branch", branch])?;
        self.git(["branch", "--", branch, sha])?;
        Ok(())
    }

    pub(super) fn reset_snapshot(&self) -> Result<ResetSnapshot> {
        let state = self.load_state()?;
        ensure!(
            !state.merge_state.in_progress() && state.merge_state.conflicts.is_empty(),
            "Finish the current Git operation first"
        );
        ensure!(self.has_head()?, "HEAD does not exist");
        let head_ref = self.git_output(["symbolic-ref", "--quiet", "HEAD"])?;
        ensure!(
            head_ref.status.success() || head_ref.status.code() == Some(1),
            "Cannot inspect HEAD reference"
        );
        let flags = self.git(["ls-files", "-v", "-z"])?;
        let mut tracked = state
            .files
            .into_iter()
            .filter(|file| file.status != "??")
            .collect::<Vec<_>>();
        // Status deliberately hides assume-unchanged and skip-worktree paths; include their
        // content in confirmation checks as reset --hard may overwrite hidden changes.
        for record in flags.split(|b| *b == 0).filter(|r| r.len() > 2) {
            if record[0].is_ascii_lowercase() || record[0] == b'S' {
                let path = path_from_bytes(&record[2..])?;
                if !tracked.iter().any(|f| f.path == path) {
                    tracked.push(FileEntry {
                        path,
                        original_path: None,
                        status: String::new(),
                        staged: false,
                        unstaged: false,
                        conflicted: false,
                    });
                }
            }
        }
        let files = tracked
            .iter()
            .filter(|file| !self.root.join(&file.path).is_dir())
            .map(|file| self.file_snapshot(file))
            .collect::<Result<Vec<_>>>()?;
        Ok(ResetSnapshot {
            head: self.text(&["rev-parse", "HEAD"])?.trim().into(),
            head_ref: head_ref.stdout,
            status: self.git(["status", "--porcelain=v1", "-z", "--untracked-files=all"])?,
            index: self.git(["ls-files", "--stage", "-z"])?,
            flags,
            files,
        })
    }

    pub fn prepare_reset(&self, sha: &str) -> Result<ResetRequest> {
        self.verify_commit_id(sha)?;
        Ok(ResetRequest {
            target: sha.into(),
            snapshot: self.reset_snapshot()?,
        })
    }

    pub fn reset_commit(&self, request: &ResetRequest, mode: ResetMode) -> Result<String> {
        ensure!(
            self.reset_snapshot()? == request.snapshot,
            "Repository changed since reset preview; refresh and confirm again"
        );
        self.verify_commit_id(&request.target)?;
        if mode == ResetMode::Hard {
            let target = self.git(["ls-tree", "-r", "-z", "--name-only", &request.target])?;
            // Include ignored files: reset --hard can remove untracked paths that obstruct its tree.
            let others = self.git(["ls-files", "--others", "-z"])?;
            let normalize = |path: PathBuf| {
                #[cfg(any(target_os = "macos", windows))]
                {
                    PathBuf::from(path.to_string_lossy().to_lowercase())
                }
                #[cfg(not(any(target_os = "macos", windows)))]
                {
                    path
                }
            };
            let targets = target
                .split(|b| *b == 0)
                .filter(|p| !p.is_empty())
                .map(|raw| path_from_bytes(raw).map(normalize))
                .collect::<Result<std::collections::HashSet<_>>>()?;
            let ancestors = targets
                .iter()
                .flat_map(|path| path.ancestors().skip(1).map(Path::to_owned))
                .collect::<std::collections::HashSet<_>>();
            for raw in others.split(|b| *b == 0).filter(|p| !p.is_empty()) {
                let other = path_from_bytes(raw)?;
                let comparable = normalize(other.clone());
                let overlap = ancestors.contains(&comparable)
                    || comparable.ancestors().any(|p| targets.contains(p));
                ensure!(
                    !overlap,
                    "Hard reset would remove untracked/ignored path {}; move it first",
                    display_path(&other)
                );
            }
        }
        ensure!(
            self.reset_snapshot()? == request.snapshot,
            "Repository changed during reset preparation; refresh and confirm again"
        );
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let recovery = format!("refs/git-wirdo/recovery/{stamp}-{}", std::process::id());
        let zero = "0".repeat(request.snapshot.head.len());
        self.git([
            "update-ref",
            "--create-reflog",
            "-m",
            "git-wirdo before reset",
            &recovery,
            &request.snapshot.head,
            &zero,
        ])?;
        self.git(["reset", mode.flag(), &request.target, "--"])?;
        Ok(recovery)
    }

    /// Apply or undo exactly one immutable commit, preserving Git's conflict state.
    pub fn apply_commit(&self, sha: &str, revert: bool, mainline: Option<usize>) -> Result<()> {
        self.ensure_clean()?;
        ensure!(
            matches!(sha.len(), 40 | 64) && sha.bytes().all(|b| b.is_ascii_hexdigit()),
            "Select a full commit ID"
        );
        let target = format!("{sha}^{{commit}}");
        let resolved = self.text(&["rev-parse", "--verify", "--end-of-options", &target])?;
        ensure!(resolved.trim() == sha, "Selected commit changed");
        let parents = self.text(&["rev-list", "--parents", "-n", "1", sha])?;
        let count = parents.split_whitespace().count().saturating_sub(1);
        if count > 1 {
            ensure!(
                mainline.is_some_and(|n| n > 0 && n <= count),
                "Merge commit requires a mainline parent from 1 to {count}"
            );
        } else {
            ensure!(
                mainline.is_none(),
                "Mainline is only valid for merge commits"
            );
        }
        let mut args = vec![
            if revert {
                "revert".to_owned()
            } else {
                "cherry-pick".to_owned()
            },
            "--no-edit".into(),
        ];
        if let Some(n) = mainline {
            args.extend(["--mainline".into(), n.to_string()]);
        }
        args.extend(["--".into(), sha.to_owned()]);
        self.git(args)?;
        Ok(())
    }

    pub fn continue_operation(&self) -> Result<()> {
        let state = self.load_state()?.merge_state;
        ensure!(
            state.conflicts.is_empty(),
            "Resolve and stage all conflicts before continuing"
        );
        if state.am_in_progress {
            self.git(["am", "--continue"])?;
        } else if state.rebase_in_progress {
            self.git(["rebase", "--continue"])?;
        } else if state.cherry_pick_in_progress {
            self.git(["cherry-pick", "--continue"])?;
        } else if state.revert_in_progress {
            self.git(["revert", "--continue"])?;
        } else if state.merge_in_progress {
            self.git(["commit", "--no-edit"])?;
        } else {
            bail!("No Git operation in progress");
        }
        Ok(())
    }

    pub fn skip_rebase(&self) -> Result<()> {
        ensure!(
            self.merge_state(&[])?.rebase_in_progress,
            "No rebase in progress"
        );
        self.git(["rebase", "--skip"])?;
        Ok(())
    }

    pub fn abort_operation(&self) -> Result<()> {
        let state = self.merge_state(&[])?;
        if state.am_in_progress {
            self.git(["am", "--abort"])?;
        } else if state.rebase_in_progress {
            self.git(["rebase", "--abort"])?;
        } else if state.cherry_pick_in_progress {
            self.git(["cherry-pick", "--abort"])?;
        } else if state.revert_in_progress {
            self.git(["revert", "--abort"])?;
        } else if state.merge_in_progress {
            self.git(["merge", "--abort"])?;
        } else {
            bail!("No Git operation in progress");
        }
        Ok(())
    }
}
