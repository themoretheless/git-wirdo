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
