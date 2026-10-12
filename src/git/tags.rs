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
        text.lines()
            .map(|line| {
                let fields: Vec<_> = line.splitn(4, '\0').collect();
                ensure!(fields.len() == 4, "Invalid tag record");
                Ok(crate::model::TagEntry {
                    name: fields[0].into(),
                    sha: fields[1].into(),
                    kind: fields[2].into(),
                    subject: fields[3].into(),
                })
            })
            .collect()
    }

    pub fn tag_detail(&self, tag: &crate::model::TagEntry) -> Result<String> {
        self.text(&[
            "show",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--stat",
            &tag.sha,
            "--",
        ])
    }

    pub fn create_tag(&self, name: &str, target: &str, message: &str) -> Result<()> {
        let reference = format!("refs/tags/{name}");
        git(&self.root, ["check-ref-format", &reference])?;
        let target = if target.trim().is_empty() {
            "HEAD"
        } else {
            target
        };
        let target = format!("{target}^{{commit}}");
        let sha = self.text(&["rev-parse", "--verify", "--end-of-options", &target])?;
        if message.trim().is_empty() {
            git(&self.root, ["tag", "--no-sign", "--", name, sha.trim()])?;
        } else {
            git(
                &self.root,
                ["tag", "-a", "-m", message, "--", name, sha.trim()],
            )?;
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
        git(&self.root, ["update-ref", "-d", &reference, &tag.sha])?;
        Ok(())
    }

    pub fn publish_tag(&self, tag: &crate::model::TagEntry, remote: &str) -> Result<()> {
        self.verify_remote(remote)?;
        self.verify_tag(tag)?;
        let reference = format!("refs/tags/{}", tag.name);
        // Publish the selected object, not an unrelated tag or a later replacement.
        let spec = format!("{}:{reference}", tag.sha);
        git(&self.root, ["push", "--progress", "--", remote, &spec])?;
        Ok(())
    }

    pub fn delete_remote_tag(&self, tag: &crate::model::TagEntry, remote: &str) -> Result<()> {
        self.verify_remote(remote)?;
        let reference = format!("refs/tags/{}", tag.name);
        let lease = format!("--force-with-lease={reference}:{}", tag.sha);
        let spec = format!(":{reference}");
        git(
            &self.root,
            ["push", "--progress", &lease, "--", remote, &spec],
        )?;
        Ok(())
    }
}
