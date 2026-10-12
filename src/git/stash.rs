//! Stashes, including untracked files, with stale-selector checks.
use super::*;

impl Repository {
    pub fn stashes(&self) -> Result<Vec<crate::model::StashEntry>> {
        let bytes = git(
            &self.root,
            ["stash", "list", "-z", "--format=%gd%x00%H%x00%s"],
        )?;
        let text = std::str::from_utf8(&bytes).context("Invalid UTF-8 stash metadata")?;
        let fields: Vec<_> = text.split_terminator('\0').collect();
        let (records, remainder) = fields.as_chunks::<3>();
        ensure!(remainder.is_empty(), "Invalid stash list record");
        Ok(records
            .iter()
            .map(|r| crate::model::StashEntry {
                selector: r[0].into(),
                sha: r[1].into(),
                subject: r[2].into(),
            })
            .collect())
    }

    pub fn stash_detail(&self, stash: &crate::model::StashEntry) -> Result<String> {
        self.text(&[
            "stash",
            "show",
            "--include-untracked",
            "--patch",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            &stash.sha,
        ])
    }

    pub fn save_stash(&self, message: &str) -> Result<()> {
        let state = self.load_state()?;
        ensure!(
            !state.merge_state.in_progress() && state.merge_state.conflicts.is_empty(),
            "Finish the current operation before saving a stash"
        );
        ensure!(!state.files.is_empty(), "No changes to stash");
        git(
            &self.root,
            ["stash", "push", "--include-untracked", "--message", message],
        )?;
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
        git(
            &self.root,
            [
                "stash",
                if pop { "pop" } else { "apply" },
                "--index",
                &stash.selector,
            ],
        )?;
        Ok(())
    }

    pub fn drop_stash(&self, stash: &crate::model::StashEntry) -> Result<()> {
        self.verify_stash(stash)?;
        git(&self.root, ["stash", "drop", &stash.selector])?;
        Ok(())
    }
}
