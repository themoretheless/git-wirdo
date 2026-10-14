//! Multi-field prompts: every mutation that needs input or a typed confirmation goes
//! through a `PromptKind`. Each domain owns its prompt variants and how they run, so a
//! domain's `submit` matches only its own variants.
mod branches;
mod github;
mod history;
mod remotes;
mod repository;
mod stash;
mod tags;
mod working_tree;

pub use branches::BranchPrompt;
pub use github::GitHubPrompt;
pub use history::HistoryPrompt;
pub use remotes::RemotePrompt;
pub use repository::RepositoryPrompt;
pub use stash::StashPrompt;
pub use tags::TagPrompt;
pub use working_tree::WorkingTreePrompt;

use anyhow::{Context, Result};

use super::App;

#[derive(Debug, Clone)]
pub enum PromptKind {
    ListFilter(crate::model::ViewMode),
    CommandPalette,
    Repository(RepositoryPrompt),
    WorkingTree(WorkingTreePrompt),
    History(HistoryPrompt),
    Branch(BranchPrompt),
    Stash(StashPrompt),
    Tag(TagPrompt),
    Remote(RemotePrompt),
    GitHub(GitHubPrompt),
}

macro_rules! into_prompt_kind {
    ($($domain:ident => $variant:ident),* $(,)?) => {$(
        impl From<$domain> for PromptKind {
            fn from(prompt: $domain) -> Self {
                Self::$variant(prompt)
            }
        }
    )*};
}

into_prompt_kind! {
    RepositoryPrompt => Repository,
    WorkingTreePrompt => WorkingTree,
    HistoryPrompt => History,
    BranchPrompt => Branch,
    StashPrompt => Stash,
    TagPrompt => Tag,
    RemotePrompt => Remote,
    GitHubPrompt => GitHub,
}

#[derive(Debug, Clone)]
pub struct Prompt {
    pub kind: PromptKind,
    pub labels: Vec<&'static str>,
    pub values: Vec<String>,
    pub text: String,
    pub defaults: Vec<String>,
}

/// What a submitted prompt left to do.
pub(super) enum Submitted {
    /// Refresh, then show this outcome in the action line.
    Message(String),
    /// The prompt already replaced or refreshed the state (opened a repository, ran an action).
    Done,
}

impl From<String> for Submitted {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}

impl From<&str> for Submitted {
    fn from(message: &str) -> Self {
        Self::Message(message.into())
    }
}

impl App {
    pub(super) fn start_prompt(&mut self, kind: impl Into<PromptKind>, labels: Vec<&'static str>) {
        self.prompt = Some(Prompt {
            kind: kind.into(),
            labels,
            values: Vec::new(),
            text: String::new(),
            defaults: Vec::new(),
        });
    }

    pub(super) fn start_prefilled_prompt(
        &mut self,
        kind: impl Into<PromptKind>,
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
        let v = prompt.values;
        let submitted = match prompt.kind {
            PromptKind::ListFilter(view) => {
                self.list_filter = v[0].to_lowercase();
                self.filter_view = Some(view);
                let visible = self.visible_indices();
                if !visible.contains(&self.selection())
                    && let Some(index) = visible.first()
                {
                    self.select(*index);
                }
                self.refresh_detail();
                self.notify(format!(
                    "Filter: {} ({} matches)",
                    self.list_filter,
                    visible.len()
                ));
                Submitted::Done
            }
            PromptKind::CommandPalette => self.submit_command_palette(&v[0])?,
            PromptKind::Repository(prompt) => prompt.submit(self, &v)?,
            PromptKind::WorkingTree(prompt) => prompt.submit(self, &v)?,
            PromptKind::History(prompt) => prompt.submit(self, &v)?,
            PromptKind::Branch(prompt) => prompt.submit(self, &v)?,
            PromptKind::Stash(prompt) => prompt.submit(self, &v)?,
            PromptKind::Tag(prompt) => prompt.submit(self, &v)?,
            PromptKind::Remote(prompt) => prompt.submit(self, &v)?,
            PromptKind::GitHub(prompt) => prompt.submit(self, &v)?,
        };
        let Submitted::Message(output) = submitted else {
            return Ok(());
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

    fn submit_command_palette(&mut self, query: &str) -> Result<Submitted> {
        let entries = crate::commands::filtered(self.view, query);
        let entry = entries
            .get(self.palette_selection)
            .context("No matching command; open the palette and change the search")?;
        if let Some(view) = entry.view {
            self.view = view;
        }
        self.apply(entry.action)?;
        Ok(Submitted::Done)
    }
}
