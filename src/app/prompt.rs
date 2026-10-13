//! Multi-field forms. Every mutation that needs typed input or a typed confirmation is a prompt.
use anyhow::{Context, Result, ensure};
use std::path::{Path, PathBuf};

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
    CreateGitHubRepository,
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

impl Prompt {
    /// Accept the current field. Returns true once every field has a value.
    pub fn advance(&mut self) -> bool {
        self.values.push(std::mem::take(&mut self.text));
        if self.values.len() < self.labels.len() {
            self.text = self
                .defaults
                .get(self.values.len())
                .cloned()
                .unwrap_or_default();
            false
        } else {
            true
        }
    }
}

/// Typed confirmations protect destructive operations from a stray keypress.
fn confirm(value: &str, expected: &str, cancelled: &str) -> Result<()> {
    ensure!(value == expected, "{cancelled}: type {expected} exactly");
    Ok(())
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

    /// Prompts that confirm an entry selected earlier announce which entry that is.
    pub(super) fn start_selected_prompt(
        &mut self,
        message: String,
        kind: PromptKind,
        labels: Vec<&'static str>,
    ) {
        self.message = message;
        self.start_prompt(kind, labels);
    }

    pub(super) fn submit_prompt(&mut self, prompt: Prompt) -> Result<()> {
        let v = prompt.values;
        let root = self.repository.root().to_owned();
        let repository = &self.repository;
        let result: Result<String> = match prompt.kind {
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
                let result = confirm(&v[1], "apply", "Import cancelled").and_then(|()| {
                    repository
                        .import_mail_patch(Path::new(&v[0]))
                        .map(|()| "Imported mail patch".into())
                });
                let outcome = self.finish(result);
                if self.state.merge_state.am_in_progress {
                    self.enter_view(ViewMode::Conflicts)?;
                }
                return outcome;
            }
            PromptKind::ExportCommit(sha) => repository
                .export_commit(&sha, Path::new(&v[0]))
                .map(|path| format!("Exported mail patch {}", display_path(&path))),
            PromptKind::FilterPr => {
                let filter = crate::github::PrFilter {
                    state: v[0].trim().into(),
                    search: v[1].clone(),
                    limit: 100,
                };
                filter.validate()?;
                let prs = crate::github::list_filtered(&root, &filter)?;
                self.pr_filter = filter;
                self.pull_requests = prs;
                self.pr_selection = 0;
                Ok("Updated PR filter".into())
            }
            PromptKind::EditPr(expected) => {
                crate::github::edit(&root, &expected, &v[0], &v[1], &v[2])
            }
            PromptKind::ReadyPr(pr) => {
                confirm(&v[0], &pr.number.to_string(), "Draft transition cancelled")
                    .and_then(|()| crate::github::set_ready(&root, &pr))
            }
            PromptKind::InlinePr(pr, file) => v[1]
                .trim()
                .parse()
                .context("Enter a positive diff line number")
                .and_then(|line| {
                    crate::pr_review::inline_comment(&root, &pr, &file, v[0].trim(), line, &v[2])
                }),
            PromptKind::RecoverCommit(sha) => repository
                .recover_commit(&sha, &v[0])
                .map(|()| format!("Recovered {sha} into branch {}; checkout unchanged", v[0])),
            PromptKind::ResetCommit(request) => crate::git::ResetMode::parse(v[0].trim())
                .and_then(|mode| {
                    confirm(&v[1], &request.target, "Reset cancelled")?;
                    repository.reset_commit(&request, mode)
                })
                .map(|recovery| {
                    format!(
                        "Reset {} to {}; previous HEAD saved as {recovery}",
                        v[0], request.target
                    )
                }),
            PromptKind::ApplyCommit(sha, revert) => {
                confirm(&v[0], &sha, "Commit operation cancelled").and_then(|()| {
                    let mainline = match v[1].trim() {
                        "" => None,
                        number => Some(
                            number
                                .parse::<usize>()
                                .context("Mainline must be a parent number")?,
                        ),
                    };
                    repository.apply_commit(&sha, revert, mainline).map(|()| {
                        if revert {
                            "Reverted commit".into()
                        } else {
                            "Cherry-picked commit".into()
                        }
                    })
                })
            }
            PromptKind::RestoreFile(request) => confirm(&v[0], "discard", "Discard cancelled")
                .and_then(|()| repository.restore_file(&request))
                .map(|()| "Restored selected file".into()),
            PromptKind::CreateTag => repository
                .create_tag(&v[0], &v[1], &v[2])
                .map(|()| "Created tag".into()),
            PromptKind::DeleteTag(tag) => confirm(&v[0], "delete", "Tag deletion cancelled")
                .and_then(|()| repository.delete_tag(&tag))
                .map(|()| "Deleted selected local tag".into()),
            PromptKind::PublishTag(tag) => repository
                .publish_tag(&tag, &v[0])
                .map(|()| "Published selected tag".into()),
            PromptKind::DeleteRemoteTag(tag) => {
                confirm(&v[1], "delete", "Remote tag deletion cancelled")
                    .and_then(|()| repository.delete_remote_tag(&tag, &v[0]))
                    .map(|()| "Deleted remote tag; local tag retained".into())
            }
            PromptKind::AddRemote => repository
                .add_remote(&v[0], &v[1])
                .map(|()| "Added remote".into()),
            PromptKind::EditPushUrl(name) => repository
                .edit_push_url(&name, &v[0])
                .map(|()| "Updated push URL".into()),
            PromptKind::EditRemote(name) => repository
                .edit_remote(&name, &v[0])
                .map(|()| "Updated fetch URL".into()),
            PromptKind::RemoveRemote(name) => confirm(&v[0], "remove", "Remote removal cancelled")
                .and_then(|()| repository.remove_remote(&name))
                .map(|()| "Removed remote".into()),
            PromptKind::CheckoutRemote(reference) => repository
                .checkout_remote(&reference, &v[0])
                .map(|()| "Checked out tracking branch".into()),
            PromptKind::SetUpstream => repository
                .set_upstream(&v[0])
                .map(|()| "Updated upstream".into()),
            PromptKind::PublishBranch => repository
                .publish_branch(&v[0])
                .map(|()| "Published current branch and set upstream".into()),
            PromptKind::PullStrategy => repository
                .pull_strategy(&v[0])
                .map(|()| "Pulled changes".into()),
            PromptKind::SaveStash => repository
                .save_stash(&v[0])
                .map(|()| "Saved stash including untracked files".into()),
            PromptKind::ApplyStash(stash, pop) => {
                let (word, success) = if pop {
                    ("pop", "Applied and removed stash")
                } else {
                    ("apply", "Applied stash; entry retained")
                };
                confirm(&v[0], word, "Stash operation cancelled")
                    .and_then(|()| repository.apply_stash(&stash, pop))
                    .map(|()| success.into())
            }
            PromptKind::DropStash(stash) => confirm(&v[0], "drop", "Drop cancelled")
                .and_then(|()| repository.drop_stash(&stash))
                .map(|()| "Dropped selected stash".into()),
            // Reload after hooks even when they fail, preserving partial state changes.
            PromptKind::Commit(amend) => {
                let committed = if amend {
                    repository.amend(&v[0])
                } else {
                    repository.commit(&v[0])
                };
                committed.map(|()| "Committed changes".into())
            }
            PromptKind::Branch => repository
                .switch_or_create_branch(&v[0])
                .map(|()| "Switched branch".into()),
            PromptKind::RenameBranch(name) => repository
                .rename_branch(&name, &v[0])
                .map(|()| "Renamed branch".into()),
            PromptKind::DeleteBranch(name) => confirm(&v[0], "delete", "Deletion cancelled")
                .and_then(|()| repository.delete_branch(&name))
                .map(|()| "Deleted merged branch".into()),
            PromptKind::IntegrateBranch(name, rebase) => {
                let word = if rebase { "rebase" } else { "merge" };
                confirm(&v[0], word, "Operation cancelled")
                    .and_then(|()| repository.integrate_branch(&name, rebase))
                    .map(|()| "Branch integrated".into())
            }
            PromptKind::InitializeRepository => {
                let repository = Repository::initialize(&root, Path::new(&v[0]), &v[1])?;
                return self.open_repository(repository.root());
            }
            PromptKind::CloneGitRepository => {
                let repository =
                    Repository::clone_into(&root, std::ffi::OsStr::new(&v[0]), Path::new(&v[1]))?;
                return self.open_repository(repository.root());
            }
            PromptKind::CloneRepository(name) => {
                let path = repository.resolve_path(&PathBuf::from(&v[0]));
                crate::github::clone_repository(&root, &name, &path)?;
                return self.open_repository(&path);
            }
            PromptKind::CreateGitHubRepository => {
                confirm(&v[3], "create", "Repository creation cancelled").and_then(|()| {
                    crate::github::create_repository(&root, &v[0], v[1].trim(), v[2].trim())
                })
            }
            PromptKind::Workspace => repository
                .create_workspace(Path::new(&v[0]), &v[1])
                .map(|()| "Workspace created".into()),
            PromptKind::OpenRepository => {
                let path = repository.resolve_path(&PathBuf::from(&v[0]));
                return self.open_repository(&path);
            }
            PromptKind::Remove(w) => confirm(&v[0], "remove", "Removal cancelled")
                .and_then(|()| repository.remove_workspace(&w))
                .map(|()| "Workspace removed; branch retained".into()),
            PromptKind::CreatePr => repository
                .ensure_clean()
                .and_then(|()| crate::github::create(&root, &v[0], &v[1], &v[2], &v[3])),
            PromptKind::Merge(pr) => confirm(&v[0], &pr.number.to_string(), "Merge cancelled")
                .and_then(|()| {
                    crate::github::merge_with_method(
                        &root,
                        &pr,
                        crate::github::MergeMethod::parse(v[1].trim())?,
                    )
                }),
            PromptKind::Review(pr, verdict) => {
                crate::github::review_selected(&root, &pr, verdict, &v[0])
            }
            PromptKind::Comment(number) => crate::github::comment(&root, number, &v[0]),
        };
        self.finish(result)
    }
}
