//! Local and remote tags.
use super::*;

impl Repository {
    pub fn tags(&self) -> Result<Vec<crate::model::TagEntry>> {
        let text = self.text(&[
            "for-each-ref",
            "--sort=-creatordate",
            "--format=%(refname:strip=2)%00%(objectname)%00%(objecttype)%00%(subject)",
            "refs/tags/",
        ])?;
        Ok(line_records::<4>(&text, "tag")?
            .into_iter()
            .map(|[name, sha, kind, subject]| crate::model::TagEntry {
                name,
                sha,
                kind,
                subject,
            })
            .collect())
    }

    pub fn tag_detail(&self, tag: &crate::model::TagEntry) -> Result<String> {
        self.text(&args(&[&["show"], PLAIN, &["--stat", &tag.sha, "--"]]))
    }

    pub fn create_tag(&self, name: &str, target: &str, message: &str) -> Result<()> {
        let reference = format!("refs/tags/{name}");
        self.git(["check-ref-format", &reference])?;
        let target = if target.trim().is_empty() {
            "HEAD"
        } else {
            target
        };
        let target = format!("{target}^{{commit}}");
        let sha = self.text(&["rev-parse", "--verify", "--end-of-options", &target])?;
        if message.trim().is_empty() {
            self.git(["tag", "--no-sign", "--", name, sha.trim()])?;
        } else {
            self.git(["tag", "-a", "-m", message, "--", name, sha.trim()])?;
        }
        Ok(())
    }

    pub(super) fn verify_tag(&self, tag: &crate::model::TagEntry) -> Result<()> {
        ensure!(
            self.tags()?
                .iter()
                .any(|t| t.name == tag.name && t.sha == tag.sha),
            "Tag changed or disappeared; refresh first"
        );
        Ok(())
    }

    pub fn delete_tag(&self, tag: &crate::model::TagEntry) -> Result<()> {
        // Compare-and-swap ref deletion refuses a tag changed since selection.
        let reference = format!("refs/tags/{}", tag.name);
        self.git(["update-ref", "-d", &reference, &tag.sha])?;
        Ok(())
    }

    pub fn publish_tag(&self, tag: &crate::model::TagEntry, remote: &str) -> Result<()> {
        self.verify_remote(remote)?;
        self.verify_tag(tag)?;
        let reference = format!("refs/tags/{}", tag.name);
        // Publish the selected object, not an unrelated tag or a later replacement.
        let spec = format!("{}:{reference}", tag.sha);
        self.git(["push", "--progress", "--", remote, &spec])?;
        Ok(())
    }

    pub fn delete_remote_tag(&self, tag: &crate::model::TagEntry, remote: &str) -> Result<()> {
        self.verify_remote(remote)?;
        let reference = format!("refs/tags/{}", tag.name);
        let lease = format!("--force-with-lease={reference}:{}", tag.sha);
        let spec = format!(":{reference}");
        self.git(["push", "--progress", &lease, "--", remote, &spec])?;
        Ok(())
    }
}
