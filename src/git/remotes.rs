//! Remotes, tracking branches, upstream state and network operations.
use super::*;

impl Repository {
    /// One `remote -v` call lists every fetch and push URL; URLs may contain spaces, so the
    /// trailing kind marker is split off from the right.
    pub fn remotes(&self) -> Result<Vec<crate::model::RemoteEntry>> {
        let mut remotes: Vec<crate::model::RemoteEntry> = Vec::new();
        for line in self.text(&["remote", "-v"])?.lines() {
            let (name, rest) = line
                .split_once('\t')
                .context("Invalid remote record from Git")?;
            let (url, kind) = rest
                .rsplit_once(' ')
                .context("Invalid remote URL record from Git")?;
            let entry = match remotes.iter_mut().position(|remote| remote.name == name) {
                Some(index) => &mut remotes[index],
                None => {
                    remotes.push(crate::model::RemoteEntry {
                        name: name.to_owned(),
                        fetch_urls: String::new(),
                        push_urls: String::new(),
                    });
                    remotes.last_mut().expect("just pushed")
                }
            };
            let urls = match kind {
                "(fetch)" => &mut entry.fetch_urls,
                "(push)" => &mut entry.push_urls,
                _ => bail!("Unknown remote URL kind {kind}"),
            };
            urls.push_str(url);
            urls.push('\n');
        }
        Ok(remotes)
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
        self.git(["remote", "add", "--", name, url])?;
        Ok(())
    }

    pub fn edit_remote(&self, name: &str, url: &str) -> Result<()> {
        self.verify_remote(name)?;
        ensure!(!url.trim().is_empty(), "URL is required");
        self.git(["remote", "set-url", "--", name, url])?;
        Ok(())
    }

    pub fn edit_push_url(&self, name: &str, url: &str) -> Result<()> {
        self.verify_remote(name)?;
        ensure!(!url.trim().is_empty(), "Push URL is required");
        self.git(["remote", "set-url", "--push", "--", name, url])?;
        Ok(())
    }

    pub fn remove_remote(&self, name: &str) -> Result<()> {
        self.verify_remote(name)?;
        self.git(["remote", "remove", "--", name])?;
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
        self.git(["check-ref-format", "--branch", local])?;
        let reference = format!("refs/remotes/{remote_branch}");
        self.git(["switch", "--track", "-c", local, &reference])?;
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
            self.git(["branch", "--unset-upstream"])?;
            return Ok(());
        }
        let arg = format!("--set-upstream-to={reference}");
        self.git(["branch", &arg])?;
        Ok(())
    }

    pub fn publish_branch(&self, remote: &str) -> Result<()> {
        self.verify_remote(remote)?;
        let branch = self.current_branch()?;
        let reference = format!("refs/heads/{branch}");
        self.git(["show-ref", "--verify", &reference])?;
        self.git([
            "push",
            "--progress",
            "--set-upstream",
            "--",
            remote,
            &reference,
        ])?;
        Ok(())
    }

    pub fn pull_strategy(&self, strategy: &str) -> Result<()> {
        self.ensure_clean()?;
        match strategy {
            "ff-only" => {
                self.git(["pull", "--progress", "--ff-only"])?;
            }
            "merge" => {
                self.git(["pull", "--progress", "--no-rebase", "--ff", "--no-edit"])?;
            }
            "rebase" => {
                self.git(["pull", "--progress", "--rebase", "--ff"])?;
            }
            _ => bail!("Choose ff-only, merge or rebase"),
        }
        Ok(())
    }

    pub fn fetch_remote(&self, name: &str) -> Result<()> {
        self.verify_remote(name)?;
        self.git(["fetch", "--progress", "--", name])?;
        Ok(())
    }

    pub fn fetch(&self) -> Result<()> {
        self.git(["fetch", "--all", "--progress"])?;
        Ok(())
    }

    pub fn pull(&self) -> Result<()> {
        self.git(["pull", "--progress", "--ff-only"])?;
        Ok(())
    }

    pub fn push(&self) -> Result<()> {
        self.git(["push", "--progress"])?;
        Ok(())
    }
}
