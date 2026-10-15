//! The working tree and index: status, diffs, hunks, restore and conflicts.
use super::*;

impl Repository {
    pub fn load_state(&self) -> Result<RepoState> {
        self.load_state_with_history(HISTORY_PAGE)
    }

    /// A cheap digest of HEAD, upstream counts, the index and the working tree, from one
    /// `git status` call. It changes whenever another tool edits files, stages, commits,
    /// switches branches or fetches, so callers know when a full reload is worth its cost.
    pub fn fingerprint(&self) -> Result<u64> {
        use std::hash::{Hash, Hasher};
        let status = self.git([
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=all",
        ])?;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        status.hash(&mut hasher);
        Ok(hasher.finish())
    }

    /// Snapshot status, branches and the first `history_limit` commits in one pass.
    pub fn load_state_with_history(&self, history_limit: usize) -> Result<RepoState> {
        let files = parse_status(&self.git([
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
        ])?)?;
        let merge_state = self.merge_state(&files)?;
        Ok(RepoState {
            branch: self.current_branch()?,
            files,
            branches: self.branches()?,
            commits: self.history(history_limit)?,
            merge_state,
        })
    }

    pub(super) fn merge_state(&self, files: &[FileEntry]) -> Result<MergeState> {
        // A linked worktree has a .git *file*, and its operation state lives in its own git dir.
        Ok(MergeState {
            merge_in_progress: self.git_dir.join("MERGE_HEAD").try_exists()?,
            rebase_in_progress: self.git_dir.join("rebase-merge").try_exists()?
                || (self.git_dir.join("rebase-apply").try_exists()?
                    && !self.git_dir.join("rebase-apply/applying").try_exists()?),
            am_in_progress: self.git_dir.join("rebase-apply/applying").try_exists()?,
            cherry_pick_in_progress: self.git_dir.join("CHERRY_PICK_HEAD").try_exists()?,
            revert_in_progress: self.git_dir.join("REVERT_HEAD").try_exists()?,
            conflicts: files
                .iter()
                .filter(|file| file.conflicted)
                .map(|file| file.path.clone())
                .collect(),
        })
    }

    pub(super) fn file_snapshot(&self, file: &FileEntry) -> Result<FileSnapshot> {
        let mut contents = Vec::new();
        for path in file.paths() {
            let full = self.root.join(path);
            let content = match fs::symlink_metadata(&full) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    SavedContent::Symlink(fs::read_link(&full)?)
                }
                Ok(metadata) if metadata.is_file() => SavedContent::File {
                    content_hash: self.run_paths(&["hash-object", "--no-filters"], &[path])?,
                    readonly: metadata.permissions().readonly(),
                    #[cfg(unix)]
                    mode: {
                        use std::os::unix::fs::PermissionsExt;
                        metadata.permissions().mode()
                    },
                },
                Ok(_) => bail!("Cannot restore directories or special files"),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => SavedContent::Missing,
                Err(error) => return Err(error.into()),
            };
            contents.push((path.to_owned(), content));
        }
        let head = if self.has_head()? {
            Some(self.text(&["rev-parse", "HEAD"])?.trim().into())
        } else {
            None
        };
        Ok(FileSnapshot {
            index: self.run_paths(&["ls-files", "--stage", "-z"], &file.paths())?,
            contents,
            head,
        })
    }

    pub fn prepare_restore(&self, file: &FileEntry, from_head: bool) -> Result<RestoreRequest> {
        ensure!(
            self.load_state()?
                .files
                .iter()
                .any(|current| current == file),
            "File status changed; refresh first"
        );
        ensure!(
            !file.conflicted,
            "Use conflict resolution for conflicted files"
        );
        let snapshot = self.file_snapshot(file)?;
        ensure!(
            !from_head || file.status == "??" || snapshot.head.is_some(),
            "HEAD does not exist; unstage the initial file instead"
        );
        Ok(RestoreRequest {
            file: file.clone(),
            from_head,
            snapshot,
        })
    }

    pub fn restore_file(&self, request: &RestoreRequest) -> Result<()> {
        ensure!(
            self.file_snapshot(&request.file)? == request.snapshot,
            "File or index changed; refresh and confirm again"
        );
        if request.file.status == "??" {
            // Remove only the selected file or symlink. Never follow a symlink or recurse.
            fs::remove_file(self.root.join(&request.file.path))?;
        } else if request.from_head {
            let source = format!(
                "--source={}",
                request
                    .snapshot
                    .head
                    .as_deref()
                    .context("HEAD unavailable")?
            );
            self.run_paths(
                &["restore", &source, "--staged", "--worktree"],
                &request.file.paths(),
            )?;
        } else {
            self.run_paths(&["restore", "--worktree"], &[&request.file.path])?;
        }
        Ok(())
    }

    pub(super) fn hunk_diff(&self, file: &FileEntry, staged: bool) -> Result<Vec<u8>> {
        if file.conflicted || file.original_path.is_some() || file.status == "??" {
            return Ok(Vec::new());
        }
        let mut diff = args(&[
            &["diff"],
            PLAIN,
            &[
                "--no-renames",
                "--src-prefix=a/",
                "--dst-prefix=b/",
                "--unified=3",
                "--inter-hunk-context=0",
            ],
        ]);
        if staged {
            diff.push("--cached");
        }
        self.run_paths(&diff, &[&file.path])
    }

    pub fn hunks(&self, file: &FileEntry, staged: bool) -> Result<Vec<crate::patch::Hunk>> {
        Ok(crate::patch::split(
            file,
            staged,
            self.hunk_diff(file, staged)?,
        ))
    }

    pub fn apply_hunk(&self, hunk: &crate::patch::Hunk) -> Result<()> {
        ensure!(
            self.hunk_diff(&hunk.file, hunk.staged)? == hunk.snapshot,
            "Diff changed; refresh before applying a hunk"
        );
        let mut args = vec!["apply", "--cached", "--whitespace=nowarn"];
        if hunk.staged {
            args.push("--reverse");
        }
        args.push("-");
        checked(self.git_with_input(&args, Some(&hunk.patch))?)?;
        Ok(())
    }

    pub fn file_detail(&self, file: &FileEntry) -> Result<String> {
        if file.status == "??" {
            return self.untracked_preview(&file.path);
        }
        let paths = file.paths();
        let staged = self.path_text(&args(&[&["diff"], PLAIN, &["--cached"]]), &paths)?;
        let unstaged = self.path_text(&args(&[&["diff"], PLAIN]), &paths)?;
        let mut chunks = Vec::new();
        if !staged.is_empty() {
            chunks.push(format!("[STAGED]\n{staged}"));
        }
        if !unstaged.is_empty() {
            chunks.push(format!("[UNSTAGED]\n{unstaged}"));
        }
        Ok(if chunks.is_empty() {
            "No diff for this file".to_owned()
        } else {
            chunks.join("\n")
        })
    }

    /// Compare the working tree to the merge base with its configured upstream.
    pub fn upstream_files(&self) -> Result<Vec<FileEntry>> {
        let base = self.upstream_base()?;
        let bytes = self.git(["diff", "--name-status", "--no-renames", "-z", &base, "--"])?;
        let mut records = bytes.split(|byte| *byte == 0);
        let mut files = Vec::new();
        while let Some(status) = records.next().filter(|status| !status.is_empty()) {
            let path = records.next().context("Missing upstream diff path")?;
            files.push(FileEntry {
                path: path_from_bytes(path)?,
                original_path: None,
                status: utf8_line(status)?,
                staged: false,
                unstaged: false,
                conflicted: false,
            });
        }
        Ok(files)
    }

    pub(super) fn upstream_base(&self) -> Result<String> {
        let upstream = self
            .text(&["rev-parse", "--symbolic-full-name", "@{upstream}"])
            .context("Configure an upstream branch before comparing changes")?;
        Ok(self
            .text(&["merge-base", "HEAD", upstream.trim()])?
            .trim()
            .to_owned())
    }

    pub fn upstream_detail(&self, file: &FileEntry) -> Result<String> {
        let base = self.upstream_base()?;
        self.path_text(
            &args(&[&["diff"], PLAIN, &["--no-renames", &base]]),
            &[&file.path],
        )
    }

    pub(super) fn untracked_preview(&self, path: &Path) -> Result<String> {
        let full_path = self.root.join(path);
        let metadata = fs::symlink_metadata(&full_path)
            .with_context(|| format!("Cannot preview {}", display_path(path)))?;
        if metadata.file_type().is_symlink() {
            return Ok(format!(
                "[UNTRACKED SYMLINK]\n{}",
                display_path(&fs::read_link(full_path)?)
            ));
        }
        if !metadata.is_file() {
            return Ok("Untracked directory or special file; no text preview".to_owned());
        }
        let mut bytes = Vec::new();
        fs::File::open(full_path)?
            .take((PREVIEW_LIMIT + 1) as u64)
            .read_to_end(&mut bytes)?;
        let truncated = bytes.len() > PREVIEW_LIMIT;
        bytes.truncate(PREVIEW_LIMIT);
        let text = match std::str::from_utf8(&bytes) {
            Ok(text) if !bytes.contains(&0) => text,
            // A bounded read can end in the middle of a valid UTF-8 character.
            Err(error) if truncated && error.error_len().is_none() && !bytes.contains(&0) => {
                std::str::from_utf8(&bytes[..error.valid_up_to()])?
            }
            _ => return Ok("[UNTRACKED]\nBinary or non-UTF-8 file; no text preview".to_owned()),
        };
        let mut text = text.to_owned();
        if truncated {
            text.push_str("\n[Preview truncated at 256 KiB]");
        }
        Ok(format!("[UNTRACKED]\n{text}"))
    }

    pub fn tracked_files(&self) -> Result<Vec<FileEntry>> {
        let bytes = self.git(["ls-files", "--cached", "-z"])?;
        let mut paths = bytes
            .split(|b| *b == 0)
            .filter(|p| !p.is_empty())
            .map(path_from_bytes)
            .collect::<Result<Vec<_>>>()?;
        paths.sort();
        paths.dedup();
        Ok(paths
            .into_iter()
            .map(|path| FileEntry {
                path,
                original_path: None,
                status: "tracked".into(),
                staged: false,
                unstaged: false,
                conflicted: false,
            })
            .collect())
    }

    pub fn file_contents(&self, path: &Path) -> Result<String> {
        validate_inspection_path(path)?;
        self.untracked_preview(path)
            .map(|text| text.replacen("[UNTRACKED", "[WORKING FILE", 1))
    }

    pub fn conflict_detail(&self, path: &Path) -> Result<String> {
        self.path_text(&args(&[&["diff"], PLAIN]), &[path])
    }

    pub fn stage(&self, file: &FileEntry) -> Result<()> {
        if !file.unstaged && !file.conflicted {
            return Ok(());
        }
        // An index rename already staged its source deletion. Only the destination needs adding.
        self.run_paths(&["add", "--all"], &[&file.path])?;
        Ok(())
    }

    pub fn unstage(&self, file: &FileEntry) -> Result<()> {
        if !file.staged && !file.conflicted {
            return Ok(());
        }
        if self.has_head()? {
            // Both names are necessary: resetting only the destination leaves the old name deleted.
            self.run_paths(&["reset", "--quiet", "HEAD"], &file.paths())?;
        } else {
            // HEAD does not exist before the first commit. Never remove the working copy.
            self.run_paths(&["rm", "--cached", "--force"], &[&file.path])?;
        }
        Ok(())
    }

    pub fn resolve_side(&self, path: &Path, side: ConflictSide) -> Result<()> {
        let entries = self.run_paths(&["ls-files", "--unmerged", "-z"], &[path])?;
        ensure!(
            !entries.is_empty(),
            "{} is no longer conflicted; refresh first",
            display_path(path)
        );
        let (stage, flag) = match side {
            ConflictSide::Ours => (b'2', "--ours"),
            ConflictSide::Theirs => (b'3', "--theirs"),
        };
        let side_exists = entries.split(|byte| *byte == 0).any(|entry| {
            let metadata = entry
                .split(|byte| *byte == b'\t')
                .next()
                .unwrap_or_default();
            metadata.last() == Some(&stage)
        });
        if side_exists {
            self.run_paths(&["checkout", flag], &[path])?;
            self.mark_resolved(path)?;
        } else {
            // In a modify/delete conflict, a missing stage means the chosen side deleted the file.
            self.run_paths(&["rm", "--force"], &[path])?;
        }
        Ok(())
    }

    pub fn mark_resolved(&self, path: &Path) -> Result<()> {
        self.run_paths(&["add", "--all"], &[path])?;
        Ok(())
    }
}
