//! Stashes, including untracked files, with stale-selector checks.
use super::*;

impl Repository {
    pub fn stashes(&self) -> Result<Vec<crate::model::StashEntry>> {
        let bytes = self.git(["stash", "list", "-z", "--format=%gd%x00%H%x00%s"])?;
        Ok(nul_records::<3>(&bytes, "stash")?
            .into_iter()
            .map(|[selector, sha, subject]| crate::model::StashEntry {
                selector,
                sha,
                subject,
            })
            .collect())
    }

    pub fn stash_detail(&self, stash: &crate::model::StashEntry) -> Result<String> {
        self.text(&args(&[
            &["stash", "show", "--include-untracked", "--patch"],
            PLAIN,
            &[&stash.sha],
        ]))
    }

    pub fn save_stash(&self, message: &str) -> Result<()> {
        let state = self.load_state()?;
        ensure!(
            !state.merge_state.in_progress() && state.merge_state.conflicts.is_empty(),
            "Finish the current operation before saving a stash"
        );
        ensure!(!state.files.is_empty(), "No changes to stash");
        self.git(["stash", "push", "--include-untracked", "--message", message])?;
        Ok(())
    }

    pub(super) fn verify_stash(&self, stash: &crate::model::StashEntry) -> Result<()> {
        ensure!(
            self.stashes()?
                .iter()
                .any(|current| current.selector == stash.selector && current.sha == stash.sha),
            "Stash list changed; refresh and select the entry again"
        );
        Ok(())
    }

    pub fn apply_stash(&self, stash: &crate::model::StashEntry, pop: bool) -> Result<()> {
        self.ensure_clean()?;
        self.verify_stash(stash)?;
        // Native pop retains the reflog entry on conflict. --index restores staged changes.
        self.git([
            "stash",
            if pop { "pop" } else { "apply" },
            "--index",
            &stash.selector,
        ])?;
        Ok(())
    }

    pub fn drop_stash(&self, stash: &crate::model::StashEntry) -> Result<()> {
        self.verify_stash(stash)?;
        self.git(["stash", "drop", &stash.selector])?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct StashFilesRequest {
    files: Vec<FileEntry>,
    snapshots: Vec<FileSnapshot>,
}
impl Repository {
    pub fn prepare_stash_files(&self, files: &[FileEntry]) -> Result<StashFilesRequest> {
        let state = self.load_state()?;
        ensure!(
            !state.merge_state.in_progress() && state.merge_state.conflicts.is_empty(),
            "Finish the current operation before saving a stash"
        );
        ensure!(self.has_head()?, "Create an initial commit before stashing");
        ensure!(!files.is_empty(), "Select changed files to stash");
        let mut paths = std::collections::HashSet::new();
        for file in files {
            ensure!(
                state.files.contains(file) && paths.insert(&file.path),
                "File selection changed; refresh and select again"
            );
            ensure!(
                file.status != " A",
                "Stage or reset intent-to-add files before stashing"
            );
            for path in file.paths() {
                validate_inspection_path(path)?;
            }
        }
        let snapshots: Vec<FileSnapshot> = files
            .iter()
            .map(|f| self.file_snapshot(f))
            .collect::<Result<_>>()?;
        ensure!(
            snapshots[0].head.is_some() && snapshots.iter().all(|s| s.head == snapshots[0].head),
            "HEAD changed while preparing stash; prepare again"
        );
        Ok(StashFilesRequest {
            files: files.to_vec(),
            snapshots,
        })
    }

    pub fn save_stash_files(&self, request: &StashFilesRequest, message: &str) -> Result<()> {
        self.verify_stash_files(request)?;
        let head = request.snapshots[0]
            .head
            .as_ref()
            .context("Stash requires HEAD")?;
        let index = PrivateIndex::new(&self.git_dir)?;
        let tracked: Vec<_> = request
            .files
            .iter()
            .filter(|f| f.status != "??")
            .flat_map(FileEntry::paths)
            .collect();
        let untracked: Vec<_> = request
            .files
            .iter()
            .filter(|f| f.status == "??")
            .flat_map(FileEntry::paths)
            .collect();
        self.private_index(&index, &["read-tree", head], None)?;
        if !tracked.is_empty() {
            self.private_index_paths(&index, &["update-index", "--force-remove"], &tracked)?;
            let entries = self.run_paths(&["ls-files", "--stage", "-z"], &tracked)?;
            self.private_index(
                &index,
                &["update-index", "-z", "--index-info"],
                Some(&entries),
            )?;
        }
        let index_tree = self.private_index_text(&index, &["write-tree"])?;
        let index_commit =
            self.commit_stash_tree(&index_tree, &[head], "git-wirdo selected index")?;
        self.private_index(&index, &["read-tree", head], None)?;
        if !tracked.is_empty() {
            self.private_index_paths(&index, &["add", "--all", "--force"], &tracked)?;
        }
        let work_tree = self.private_index_text(&index, &["write-tree"])?;
        let mut parents = vec![head.as_str(), index_commit.as_str()];
        let untracked_commit;
        if !untracked.is_empty() {
            self.private_index(&index, &["read-tree", "--empty"], None)?;
            self.private_index_paths(&index, &["add", "--force"], &untracked)?;
            let tree = self.private_index_text(&index, &["write-tree"])?;
            untracked_commit =
                self.commit_stash_tree(&tree, &[], "git-wirdo selected untracked files")?;
            parents.push(&untracked_commit);
        }
        let commit = self.commit_stash_tree(&work_tree, &parents, message)?;
        self.verify_stash_files(request)?;
        // Store the recoverable copy before touching the real index or working files.
        self.git(["stash", "store", "--message", message, &commit])?;
        self.verify_stash_files(request)
            .context("Stash saved, but files changed; working files were retained")?;
        if !tracked.is_empty() {
            self.run_paths(
                &["restore", "--source", head, "--staged", "--worktree"],
                &tracked,
            )
            .context("Stash saved, but restoring selected tracked files failed")?;
        }
        for (file, snapshot) in request
            .files
            .iter()
            .zip(&request.snapshots)
            .filter(|(f, _)| f.status == "??")
        {
            ensure!(
                self.file_snapshot(file)? == *snapshot,
                "Stash saved, but untracked file changed; file retained"
            );
            // Cancellation may arrive between Git commands and file deletion.
            // A harmless Git read participates in the same cancellation generation.
            self.git(["rev-parse", "--verify", "HEAD"])?;
            fs::remove_file(self.root.join(&file.path))
                .context("Stash saved, but removing selected untracked file failed")?;
        }
        Ok(())
    }

    fn verify_stash_files(&self, request: &StashFilesRequest) -> Result<()> {
        let current = self.prepare_stash_files(&request.files)?;
        ensure!(
            current.snapshots == request.snapshots,
            "Selected files, index or HEAD changed; prepare stash again"
        );
        Ok(())
    }

    fn private_index(
        &self,
        index: &PrivateIndex,
        args: &[&str],
        input: Option<&[u8]>,
    ) -> Result<Vec<u8>> {
        let mut command = git_command(&self.root, args);
        command.env("GIT_INDEX_FILE", &index.path);
        checked(self.runner.output(&mut command, input)?)
    }
    fn private_index_text(&self, index: &PrivateIndex, args: &[&str]) -> Result<String> {
        utf8_line(&self.private_index(index, args, None)?)
    }
    fn private_index_paths(
        &self,
        index: &PrivateIndex,
        args: &[&str],
        paths: &[&Path],
    ) -> Result<()> {
        let args: Vec<OsString> = std::iter::once(OsString::from("--literal-pathspecs"))
            .chain(args.iter().map(OsString::from))
            .chain(std::iter::once(OsString::from("--")))
            .chain(paths.iter().map(|p| p.as_os_str().to_owned()))
            .collect();
        let mut command = git_command(&self.root, args);
        command.env("GIT_INDEX_FILE", &index.path);
        checked(self.runner.output(&mut command, None)?)?;
        Ok(())
    }
    fn commit_stash_tree(&self, tree: &str, parents: &[&str], message: &str) -> Result<String> {
        let mut args = vec!["commit-tree", tree];
        for parent in parents {
            args.extend(["-p", parent]);
        }
        let output = checked(self.git_with_input(args, Some(message.as_bytes()))?)?;
        utf8_line(&output)
    }
}

struct PrivateIndex {
    path: PathBuf,
    directory: PathBuf,
}
impl PrivateIndex {
    fn new(git_dir: &Path) -> Result<Self> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let directory = git_dir.join(format!("git-wirdo-stash-{}-{stamp}", std::process::id()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new().mode(0o700).create(&directory)?;
        }
        #[cfg(not(unix))]
        fs::create_dir(&directory)?;
        Ok(Self {
            path: directory.join("index"),
            directory,
        })
    }
}
impl Drop for PrivateIndex {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_file(self.directory.join("index.lock"));
        let _ = fs::remove_dir(&self.directory);
    }
}
