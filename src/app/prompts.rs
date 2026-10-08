//! Multi-field prompts: every mutation that needs input or a typed confirmation goes
//! through a `PromptKind`, and `submit_prompt` runs it once all fields are filled.
use anyhow::{Context, Result};

use super::App;
use crate::git::Repository;
use crate::model::{ViewMode, display_path};

#[derive(Debug, Clone)]
pub enum PromptKind {
    CommandPalette,
    ImportPatch,
    ExportCommit(String),
    FilterPr,
    EditPr(crate::github::EditablePr),
    ReadyPr(crate::github::PullRequest),
    InlinePr(crate::github::PullRequest, crate::pr_review::PrFile),
    ResetCommit(crate::git::ResetRequest),
    RecoverCommit(String),
    ApplyCommit(String, bool),
    RestoreFile(crate::git::RestoreRequest),
    CreateTag,
    DeleteTag(crate::model::TagEntry),
    PublishTag(crate::model::TagEntry),
    DeleteRemoteTag(crate::model::TagEntry),
    AddRemote,
    EditPushUrl(String),
    EditRemote(String),
    RemoveRemote(String),
    CheckoutRemote(String),
    SetUpstream,
    PublishBranch,
    PullStrategy,
    SaveStash,
    ApplyStash(crate::model::StashEntry, bool),
    DropStash(crate::model::StashEntry),
    Commit(bool),
    Branch,
    RenameBranch(String),
    DeleteBranch(String),
    IntegrateBranch(String, bool),
    CloneRepository(String),
    InitializeRepository,
    CloneGitRepository,
    Workspace,
    OpenRepository,
    Remove(crate::model::Workspace),
    CreatePr,
    Merge(crate::github::PullRequest),
    Review(crate::github::PullRequest, &'static str),
    Comment(u64),
}

#[derive(Debug, Clone)]
pub struct Prompt {
    pub kind: PromptKind,
    pub labels: Vec<&'static str>,
    pub values: Vec<String>,
    pub text: String,
    pub defaults: Vec<String>,
}

impl App {
    pub(super) fn start_prompt(&mut self, kind: PromptKind, labels: Vec<&'static str>) {
        self.prompt = Some(Prompt {
            kind,
            labels,
            values: Vec::new(),
            text: String::new(),
            defaults: Vec::new(),
        });
    }

    pub(super) fn start_prefilled_prompt(
        &mut self,
        kind: PromptKind,
        labels: Vec<&'static str>,
        defaults: Vec<String>,
    ) {
        self.start_prompt(kind, labels);
        if let Some(prompt) = &mut self.prompt {
            prompt.text = defaults.first().cloned().unwrap_or_default();
            prompt.defaults = defaults;
        }
    }

    pub(super) fn submit_prompt(&mut self, prompt: Prompt) -> Result<()> {
        use anyhow::ensure;
        let v = prompt.values;
        let output = match prompt.kind {
            PromptKind::CommandPalette => {
                let entries = crate::commands::filtered(self.view, &v[0]);
                let entry = entries
                    .get(self.palette_selection)
                    .context("No matching command; open the palette and change the search")?;
                if let Some(view) = entry.view {
                    self.view = view;
                }
                return self.apply(entry.action);
            }
            PromptKind::ImportPatch => {
                ensure!(v[1] == "apply", "Import cancelled: type apply exactly");
                let result = self.refresh_after(|app| {
                    app.repository
                        .import_mail_patch(std::path::Path::new(&v[0]))
                });
                if self.state.merge_state.am_in_progress {
                    self.view = ViewMode::Conflicts;
                    self.refresh_detail();
                }
                result?;
                "Imported mail patch".into()
            }

            PromptKind::ExportCommit(sha) => {
                let path = self
                    .repository
                    .export_commit(&sha, std::path::Path::new(&v[0]))?;
                format!("Exported mail patch {}", display_path(&path))
            }

            PromptKind::FilterPr => {
                let filter = crate::github::PrFilter {
                    state: v[0].trim().into(),
                    search: v[1].clone(),
                    limit: 100,
                };
                filter.validate()?;
                let prs = crate::github::list_filtered(self.repository.root(), &filter)?;
                self.pr_filter = filter;
                self.pull_requests = prs;
                self.pr_selection = 0;
                "Updated PR filter".into()
            }
            PromptKind::EditPr(expected) => {
                crate::github::edit(self.repository.root(), &expected, &v[0], &v[1], &v[2])?
            }
            PromptKind::ReadyPr(pr) => {
                ensure!(
                    v[0] == pr.number.to_string(),
                    "Draft transition cancelled: confirm PR number"
                );
                crate::github::set_ready(self.repository.root(), &pr)?
            }
            PromptKind::InlinePr(pr, file) => crate::pr_review::inline_comment(
                self.repository.root(),
                &pr,
                &file,
                v[0].trim(),
                v[1].trim()
                    .parse()
                    .context("Enter a positive diff line number")?,
                &v[2],
            )?,
            PromptKind::RecoverCommit(sha) => {
                self.repository.recover_commit(&sha, &v[0])?;
                format!("Recovered {} into branch {}; checkout unchanged", sha, v[0])
            }
            PromptKind::ResetCommit(request) => {
                let mode = crate::git::ResetMode::parse(v[0].trim())?;
                ensure!(
                    v[1] == request.target,
                    "Reset cancelled: confirm full target commit ID"
                );
                let recovery =
                    self.refresh_after(|app| app.repository.reset_commit(&request, mode))?;
                format!(
                    "Reset {} to {}; previous HEAD saved as {}",
                    v[0], request.target, recovery
                )
            }
            PromptKind::ApplyCommit(sha, revert) => {
                ensure!(
                    v[0] == sha,
                    "Commit operation cancelled: confirm full commit ID"
                );
                let mainline = if v[1].trim().is_empty() {
                    None
                } else {
                    Some(
                        v[1].trim()
                            .parse::<usize>()
                            .context("Mainline must be a parent number")?,
                    )
                };
                self.run_action(
                    |repo| repo.apply_commit(&sha, revert, mainline),
                    if revert {
                        "Reverted commit"
                    } else {
                        "Cherry-picked commit"
                    },
                )?;
                self.message.clone()
            }
            PromptKind::RestoreFile(request) => {
                ensure!(v[0] == "discard", "Discard cancelled");
                self.refresh_after(|app| app.repository.restore_file(&request))?;
                "Restored selected file".into()
            }
            PromptKind::CreateTag => {
                self.repository.create_tag(&v[0], &v[1], &v[2])?;
                "Created tag".into()
            }
            PromptKind::DeleteTag(tag) => {
                ensure!(v[0] == "delete", "Tag deletion cancelled");
                self.repository.delete_tag(&tag)?;
                "Deleted selected local tag".into()
            }
            PromptKind::PublishTag(tag) => {
                self.repository.publish_tag(&tag, &v[0])?;
                "Published selected tag".into()
            }
            PromptKind::DeleteRemoteTag(tag) => {
                ensure!(v[1] == "delete", "Remote tag deletion cancelled");
                self.repository.delete_remote_tag(&tag, &v[0])?;
                "Deleted remote tag; local tag retained".into()
            }

            PromptKind::AddRemote => {
                self.repository.add_remote(&v[0], &v[1])?;
                "Added remote".into()
            }
            PromptKind::EditPushUrl(name) => {
                self.repository.edit_push_url(&name, &v[0])?;
                "Updated push URL".into()
            }
            PromptKind::EditRemote(name) => {
                self.repository.edit_remote(&name, &v[0])?;
                "Updated fetch URL".into()
            }
            PromptKind::RemoveRemote(name) => {
                ensure!(v[0] == "remove", "Remote removal cancelled");
                self.repository.remove_remote(&name)?;
                "Removed remote".into()
            }
            PromptKind::CheckoutRemote(reference) => {
                self.repository.checkout_remote(&reference, &v[0])?;
                "Checked out tracking branch".into()
            }
            PromptKind::SetUpstream => {
                self.repository.set_upstream(&v[0])?;
                "Updated upstream".into()
            }
            PromptKind::PublishBranch => {
                self.refresh_after(|app| app.repository.publish_branch(&v[0]))?;
                "Published current branch and set upstream".into()
            }
            PromptKind::PullStrategy => {
                self.refresh_after(|app| app.repository.pull_strategy(&v[0]))?;
                "Pulled changes".into()
            }

            PromptKind::SaveStash => {
                self.refresh_after(|app| app.repository.save_stash(&v[0]))?;
                "Saved stash including untracked files".into()
            }
            PromptKind::ApplyStash(stash, pop) => {
                ensure!(
                    v[0] == if pop { "pop" } else { "apply" },
                    "Stash operation cancelled"
                );
                self.refresh_after(|app| app.repository.apply_stash(&stash, pop))?;
                if pop {
                    "Applied and removed stash".into()
                } else {
                    "Applied stash; entry retained".into()
                }
            }
            PromptKind::DropStash(stash) => {
                ensure!(v[0] == "drop", "Drop cancelled: type drop exactly");
                self.repository.drop_stash(&stash)?;
                "Dropped selected stash".into()
            }

            PromptKind::Commit(amend) => {
                // Reload after hooks even when they fail, preserving partial state changes.
                self.refresh_after(|app| {
                    if amend {
                        app.repository.amend(&v[0])
                    } else {
                        app.repository.commit(&v[0])
                    }
                })?;
                "Committed changes".into()
            }
            PromptKind::Branch => {
                self.repository.switch_or_create_branch(&v[0])?;
                "Switched branch".into()
            }
            PromptKind::RenameBranch(name) => {
                self.repository.rename_branch(&name, &v[0])?;
                "Renamed branch".into()
            }
            PromptKind::DeleteBranch(name) => {
                ensure!(v[0] == "delete", "Deletion cancelled: type delete exactly");
                self.repository.delete_branch(&name)?;
                "Deleted merged branch".into()
            }
            PromptKind::IntegrateBranch(name, rebase) => {
                ensure!(
                    v[0] == if rebase { "rebase" } else { "merge" },
                    "Operation cancelled: confirmation does not match"
                );
                self.refresh_after(|app| app.repository.integrate_branch(&name, rebase))?;
                "Branch integrated".into()
            }

            PromptKind::InitializeRepository => {
                let repository = Repository::initialize(
                    self.repository.root(),
                    std::path::Path::new(&v[0]),
                    &v[1],
                )?;
                return self.open_repository(repository.root());
            }
            PromptKind::CloneGitRepository => {
                let repository = Repository::clone_into(
                    self.repository.root(),
                    std::ffi::OsStr::new(&v[0]),
                    std::path::Path::new(&v[1]),
                )?;
                return self.open_repository(repository.root());
            }
            PromptKind::CloneRepository(name) => {
                let path = self.repository.resolve(std::path::Path::new(&v[0]));
                crate::github::clone_repository(self.repository.root(), &name, &path)?;
                return self.open_repository(&path);
            }
            PromptKind::Workspace => {
                self.repository
                    .create_workspace(std::path::Path::new(&v[0]), &v[1])?;
                "Workspace created".into()
            }
            PromptKind::OpenRepository => {
                let path = self.repository.resolve(std::path::Path::new(&v[0]));
                return self.open_repository(&path);
            }
            PromptKind::Remove(w) => {
                ensure!(v[0] == "remove", "Removal cancelled: type remove exactly");
                self.repository.remove_workspace(&w)?;
                "Workspace removed; branch retained".into()
            }
            PromptKind::CreatePr => {
                self.repository.ensure_clean()?;
                crate::github::create(self.repository.root(), &v[0], &v[1], &v[2], &v[3])?
            }
            PromptKind::Merge(pr) => {
                ensure!(
                    v[0] == pr.number.to_string(),
                    "Merge cancelled: enter the selected PR number"
                );
                crate::github::merge_with_method(
                    self.repository.root(),
                    &pr,
                    crate::github::MergeMethod::parse(v[1].trim())?,
                )?
            }
            PromptKind::Review(pr, verdict) => {
                crate::github::review_selected(self.repository.root(), &pr, verdict, &v[0])?
            }
            PromptKind::Comment(number) => {
                crate::github::comment(self.repository.root(), number, &v[0])?
            }
        };
        // Preserve the operation result even if a network refresh then fails.
        let refreshed = self.refresh();
        self.notify(if let Err(error) = refreshed {
            format!("{output}\nRefresh failed: {error:#}")
        } else {
            output
        });
        Ok(())
    }
}
