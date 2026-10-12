//! Remotes, tracking branches, upstream state and network operations.
use super::*;

impl Repository {
    pub fn remotes(&self) -> Result<Vec<crate::model::RemoteEntry>> {
        self.text(&["remote"])?
            .lines()
            .map(|name| {
                Ok(crate::model::RemoteEntry {
                    name: name.into(),
                    fetch_urls: self.text(&["remote", "get-url", "--all", name])?,
                    push_urls: self.text(&["remote", "get-url", "--push", "--all", name])?,
                })
            })
            .collect()
    }

    pub(super) fn verify_remote(&self, name: &str) -> Result<()> {
        ensure!(
            self.remotes()?.iter().any(|r| r.name == name),
            "Remote no longer exists; refresh first"
        );
        Ok(())
    }

    pub fn add_remote(&self, name: &str, url: &str) -> Result<()> {
        ensure!(
            !name.is_empty() && !name.starts_with('-') && !url.trim().is_empty(),
            "Remote name and URL are required"
        );
        git(&self.root, ["remote", "add", "--", name, url])?;
        Ok(())
    }

    pub fn edit_remote(&self, name: &str, url: &str) -> Result<()> {
        self.verify_remote(name)?;
        ensure!(!url.trim().is_empty(), "URL is required");
        git(&self.root, ["remote", "set-url", "--", name, url])?;
        Ok(())
    }

    pub fn edit_push_url(&self, name: &str, url: &str) -> Result<()> {
        self.verify_remote(name)?;
        ensure!(!url.trim().is_empty(), "Push URL is required");
        git(&self.root, ["remote", "set-url", "--push", "--", name, url])?;
        Ok(())
    }

    pub fn remove_remote(&self, name: &str) -> Result<()> {
        self.verify_remote(name)?;
        git(&self.root, ["remote", "remove", "--", name])?;
        Ok(())
    }

    pub fn remote_branches(&self) -> Result<Vec<String>> {
        let text = self.text(&[
            "for-each-ref",
            "--format=%(refname:strip=2)%00%(symref)",
            "refs/remotes/",
        ])?;
        Ok(text
            .lines()
            .filter_map(|line| line.split_once('\0'))
            .filter(|(_, symref)| symref.is_empty())
            .map(|(name, _)| name.to_owned())
            .collect())
    }

    pub fn checkout_remote(&self, remote_branch: &str, local: &str) -> Result<()> {
        ensure!(
            self.remote_branches()?.iter().any(|b| b == remote_branch),
            "Remote branch no longer exists"
        );
        git(&self.root, ["check-ref-format", "--branch", local])?;
        let reference = format!("refs/remotes/{remote_branch}");
        git(&self.root, ["switch", "--track", "-c", local, &reference])?;
        Ok(())
    }

    pub fn tracking(&self) -> Result<crate::model::Tracking> {
        let branch = self.current_branch()?;
        let reference = format!("refs/heads/{branch}");
        let upstream = self
            .text(&["for-each-ref", "--format=%(upstream:short)", &reference])?
            .trim()
            .to_owned();
        if upstream.is_empty() {
            return Ok(crate::model::Tracking::default());
        }
        let range = format!("HEAD...{upstream}");
        let counts = self.text(&["rev-list", "--left-right", "--count", &range])?;
        let mut counts = counts.split_whitespace();
        Ok(crate::model::Tracking {
            error: None,
            upstream: Some(upstream),
            ahead: counts.next().context("Missing ahead count")?.parse()?,
            behind: counts.next().context("Missing behind count")?.parse()?,
        })
    }

    pub fn set_upstream(&self, reference: &str) -> Result<()> {
        if reference.trim().is_empty() {
            git(&self.root, ["branch", "--unset-upstream"])?;
            return Ok(());
        }
        let arg = format!("--set-upstream-to={reference}");
        git(&self.root, ["branch", &arg])?;
        Ok(())
    }

    pub fn publish_branch(&self, remote: &str) -> Result<()> {
        self.verify_remote(remote)?;
        let branch = self.current_branch()?;
        let reference = format!("refs/heads/{branch}");
        git(&self.root, ["show-ref", "--verify", &reference])?;
        git(
            &self.root,
            [
                "push",
                "--progress",
                "--set-upstream",
                "--",
                remote,
                &reference,
            ],
        )?;
        Ok(())
    }

    pub fn pull_strategy(&self, strategy: &str) -> Result<()> {
        self.ensure_clean()?;
        match strategy {
            "ff-only" => {
                git(&self.root, ["pull", "--progress", "--ff-only"])?;
            }
            "merge" => {
                git(
                    &self.root,
                    ["pull", "--progress", "--no-rebase", "--ff", "--no-edit"],
                )?;
            }
            "rebase" => {
                git(&self.root, ["pull", "--progress", "--rebase", "--ff"])?;
            }
            _ => bail!("Choose ff-only, merge or rebase"),
        }
        Ok(())
    }

    pub fn fetch_remote(&self, name: &str) -> Result<()> {
        self.verify_remote(name)?;
        git(&self.root, ["fetch", "--progress", "--", name])?;
        Ok(())
    }

    pub fn fetch(&self) -> Result<()> {
        git(&self.root, ["fetch", "--all", "--progress"])?;
        Ok(())
    }

    pub fn pull(&self) -> Result<()> {
        git(&self.root, ["pull", "--progress", "--ff-only"])?;
        Ok(())
    }

    pub fn push(&self) -> Result<()> {
        git(&self.root, ["push", "--progress"])?;
        Ok(())
    }
}
