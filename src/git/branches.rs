//! Local branches: listing, switching, renaming, deletion and integration.
use super::*;

impl Repository {
    pub(super) fn has_head(&self) -> Result<bool> {
        let output = git_output(&self.root, ["rev-parse", "--verify", "--quiet", "HEAD"])?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false), // An unborn branch is not an error.
            _ => checked(output).map(|_| false),
        }
    }

    pub(super) fn current_branch(&self) -> Result<String> {
        let output = git_output(&self.root, ["symbolic-ref", "--quiet", "--short", "HEAD"])?;
        match output.status.code() {
            Some(0) => utf8_line(&output.stdout),
            Some(1) => {
                let sha = git(&self.root, ["rev-parse", "--short", "HEAD"])?;
                Ok(format!("HEAD ({})", utf8_line(&sha)?))
            }
            _ => checked(output).and_then(|bytes| utf8_line(&bytes)),
        }
    }

    pub(super) fn branches(&self) -> Result<Vec<BranchEntry>> {
        let bytes = git(
            &self.root,
            [
                "for-each-ref",
                "--format=%(HEAD)%00%(refname:strip=2)",
                "refs/heads/",
            ],
        )?;
        let text = std::str::from_utf8(&bytes).context("Git returned a non-UTF-8 branch name")?;
        text.lines()
            .map(|line| {
                let (marker, name) = line
                    .split_once('\0')
                    .context("Invalid branch record from Git")?;
                Ok(BranchEntry {
                    name: name.to_owned(),
                    current: marker == "*",
                })
            })
            .collect()
    }

    pub fn rename_branch(&self, old: &str, new: &str) -> Result<()> {
        git(&self.root, ["check-ref-format", "--branch", new])?;
        git(&self.root, ["branch", "-m", "--", old, new])?;
        Ok(())
    }

    pub fn delete_branch(&self, branch: &str) -> Result<()> {
        ensure!(
            branch != self.current_branch()?,
            "Cannot delete the current branch"
        );
        ensure!(
            !branch.starts_with("release/") && !branch.starts_with("hotfix/"),
            "Release and hotfix branches are protected"
        );
        // Explicit ancestry check avoids branch -d accepting commits merged only in its upstream.
        let tip = format!("refs/heads/{branch}");
        let output = git_output(&self.root, ["merge-base", "--is-ancestor", &tip, "HEAD"])?;
        ensure!(
            output.status.success(),
            "Branch contains commits not merged into the current HEAD"
        );
        git(&self.root, ["branch", "-d", "--", branch])?;
        Ok(())
    }

    pub fn integrate_branch(&self, branch: &str, rebase: bool) -> Result<()> {
        self.ensure_clean()?;
        let reference = format!("refs/heads/{branch}");
        git(&self.root, ["show-ref", "--verify", &reference])?;
        if rebase {
            git(&self.root, ["rebase", &reference])?;
        } else {
            git(&self.root, ["merge", "--no-edit", &reference])?;
        }
        Ok(())
    }

    pub fn switch_branch(&self, name: &str) -> Result<()> {
        git(&self.root, ["switch", "--no-guess", "--", name])?;
        Ok(())
    }

    pub fn switch_or_create_branch(&self, name: &str) -> Result<()> {
        git(&self.root, ["check-ref-format", "--branch", name])?;
        let reference = format!("refs/heads/{name}");
        let output = git_output(&self.root, ["show-ref", "--verify", "--quiet", &reference])?;
        match output.status.code() {
            Some(0) => self.switch_branch(name),
            Some(1) => {
                git(&self.root, ["switch", "-c", name])?;
                Ok(())
            }
            _ => checked(output).map(|_| ()),
        }
    }
}
