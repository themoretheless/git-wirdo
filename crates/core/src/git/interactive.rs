//! A constrained todo editor: no shell commands or omitted commits.
use super::*;

#[derive(Debug, Clone)]
pub struct InteractiveRebase {
    pub todo: String,
    base: String,
    head: String,
    branch: String,
    commits: Vec<String>,
}

impl Repository {
    pub fn prepare_interactive_rebase(&self, base: &str) -> Result<InteractiveRebase> {
        self.ensure_clean()?;
        let base = self
            .text(&[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{base}^{{commit}}"),
            ])?
            .trim()
            .to_owned();
        let head = self.text(&["rev-parse", "HEAD"])?.trim().to_owned();
        let branch = self
            .text(&["symbolic-ref", "--quiet", "HEAD"])?
            .trim()
            .to_owned();
        self.git(["merge-base", "--is-ancestor", &base, &head])?;
        let range = format!("{base}..{head}");
        ensure!(
            self.text(&["rev-list", "--merges", &range])?
                .trim()
                .is_empty(),
            "Interactive editor requires linear history; merge commits are not supported"
        );
        let records = self.text(&[
            "log",
            "--no-show-signature",
            "--reverse",
            "--max-count=201",
            "--format=%H%x00%s",
            &range,
        ])?;
        let mut commits = Vec::new();
        let mut todo = String::new();
        for line in records.lines() {
            let (sha, subject) = line
                .split_once('\0')
                .context("Malformed rebase commit record")?;
            commits.push(sha.to_owned());
            todo.push_str(&format!(
                "pick {sha} # {}\n",
                subject.escape_debug().to_string().replace(';', "\\u{3b}")
            ));
        }
        ensure!(
            !commits.is_empty() && commits.len() <= 200,
            "Select a base with 1 to 200 subsequent commits"
        );
        Ok(InteractiveRebase {
            todo,
            base,
            head,
            branch,
            commits,
        })
    }

    pub fn run_interactive_rebase(&self, plan: &InteractiveRebase, todo: &str) -> Result<()> {
        let mut ids = std::collections::HashSet::new();
        let mut canonical = String::new();
        for (index, line) in todo
            .split(['\n', ';'])
            .filter(|l| !l.trim().is_empty())
            .enumerate()
        {
            let fields: Vec<_> = line.split_whitespace().collect();
            ensure!(
                (fields.len() == 2 || (fields.len() > 2 && fields[2] == "#"))
                    && matches!(fields[0], "pick" | "squash" | "fixup"),
                "Use only pick, squash or fixup followed by a full commit ID"
            );
            ensure!(
                index != 0 || fields[0] == "pick",
                "First commit must use pick"
            );
            ensure!(
                plan.commits.iter().any(|sha| sha == fields[1]) && ids.insert(fields[1]),
                "Unknown or duplicate commit in todo"
            );
            canonical.push_str(&format!("{} {}\n", fields[0], fields[1]));
        }
        ensure!(
            ids.len() == plan.commits.len(),
            "Todo must include every prepared commit exactly once"
        );
        self.ensure_clean()?;
        ensure!(
            self.text(&["rev-parse", "HEAD"])?.trim() == plan.head
                && self.text(&["symbolic-ref", "--quiet", "HEAD"])?.trim() == plan.branch,
            "HEAD or branch changed; prepare rebase again"
        );
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let recovery = format!("refs/git-wirdo/recovery/rebase-{stamp}");
        self.git([
            "update-ref",
            "--create-reflog",
            &recovery,
            &plan.head,
            &"0".repeat(plan.head.len()),
        ])?;
        // Git supplies the todo path as $1. The contents are environment data,
        // never interpolated into shell source. Git for Windows provides sh.
        let mut command = git_command(
            &self.root,
            [
                "rebase",
                "--interactive",
                "--no-autosquash",
                "--no-rebase-merges",
                "--no-rerere-autoupdate",
                "--no-autostash",
                "--no-update-refs",
                &plan.base,
            ],
        );
        command.env("GIT_WIRDO_TODO", canonical).env(
            "GIT_SEQUENCE_EDITOR",
            "sh -c 'printf \"%s\" \"$GIT_WIRDO_TODO\" > \"$1\"' -",
        );
        checked(self.runner.output(&mut command, None)?)?;
        Ok(())
    }
}
