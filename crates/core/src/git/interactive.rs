//! A constrained todo editor: explicit commit actions, no user-provided shell commands.
use super::*;

#[derive(Debug, Clone)]
pub struct InteractiveRebase {
    pub todo: String,
    base: String,
    head: String,
    branch: String,
    commits: Vec<String>,
    messages: std::collections::BTreeMap<String, String>,
    reworded: std::collections::BTreeMap<String, String>,
}

impl InteractiveRebase {
    pub fn message(&self, sha: &str) -> Option<&str> {
        self.reworded
            .get(sha)
            .or_else(|| self.messages.get(sha))
            .map(String::as_str)
    }
    pub fn reword(&mut self, sha: &str, message: &str) -> Result<()> {
        ensure!(self.messages.contains_key(sha), "Unknown prepared commit");
        ensure!(
            !message.trim().is_empty() && !message.contains('\0'),
            "Commit message must be nonempty and contain no NUL"
        );
        ensure!(message.len() <= 64 * 1024, "Commit message exceeds 64 KiB");
        self.reworded.insert(sha.to_owned(), message.to_owned());
        Ok(())
    }
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
        let records = self.git([
            "log",
            "--no-show-signature",
            "--reverse",
            "--max-count=201",
            "--format=%H%x00%B",
            "-z",
            &range,
        ])?;
        let mut commits = Vec::new();
        let mut messages = std::collections::BTreeMap::new();
        let mut todo = String::new();
        for [sha, message] in nul_records::<2>(&records, "rebase commit")? {
            let subject = message.lines().next().unwrap_or("");
            commits.push(sha.clone());
            messages.insert(sha.clone(), message.clone());
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
            messages,
            reworded: Default::default(),
        })
    }

    pub fn run_interactive_rebase(&self, plan: &InteractiveRebase, todo: &str) -> Result<()> {
        let mut ids = std::collections::HashSet::new();
        let mut canonical = String::new();
        let mut kept = 0;
        for line in todo.split(['\n', ';']).filter(|l| !l.trim().is_empty()) {
            let fields: Vec<_> = line.split_whitespace().collect();
            ensure!(
                (fields.len() == 2 || (fields.len() > 2 && fields[2] == "#"))
                    && matches!(
                        fields[0],
                        "pick" | "squash" | "fixup" | "edit" | "drop" | "reword"
                    ),
                "Use pick, squash, fixup, edit, drop or reword followed by a full commit ID"
            );
            ensure!(
                kept != 0 || matches!(fields[0], "pick" | "edit" | "drop" | "reword"),
                "First retained commit must use pick, edit or reword"
            );
            ensure!(
                plan.commits.iter().any(|sha| sha == fields[1]) && ids.insert(fields[1]),
                "Unknown or duplicate commit in todo"
            );
            if fields[0] == "reword" {
                let message = plan
                    .reworded
                    .get(fields[1])
                    .context("Set the replacement message in the visual editor before rewording")?;
                // Only generated octal byte escapes enter shell source. Neither paths nor
                // user message text are interpolated. Git persists exec in its todo, so
                // messages also survive a conflict/edit stop and a later --continue.
                let encoded: String = message.bytes().map(|b| format!("\\{b:03o}")).collect();
                canonical.push_str(&format!("pick {}\nexec printf '{}' | git commit --amend --allow-empty --cleanup=verbatim --file=-\n", fields[1], encoded));
            } else {
                canonical.push_str(&format!("{} {}\n", fields[0], fields[1]));
            }
            if fields[0] != "drop" {
                kept += 1;
            }
        }
        ensure!(
            ids.len() == plan.commits.len(),
            "Todo must include every prepared commit exactly once"
        );
        ensure!(
            kept > 0,
            "Keep at least one commit in the interactive editor"
        );
        ensure!(
            canonical.len() <= 24 * 1024,
            "Rebase plan exceeds 24 KiB; shorten replacement messages or split the rebase"
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
