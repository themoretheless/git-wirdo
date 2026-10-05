//! UI state only: no credentials, drafts, Git refs, index, or working files.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::{app::App, github::PrFilter, model::ViewMode};

const MAX_BYTES: u64 = 1024 * 1024;
static NEXT_FILE: AtomicU64 = AtomicU64::new(0);
static LAST_OPEN: AtomicU64 = AtomicU64::new(0);

fn opened_at() -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0);
    let mut previous = LAST_OPEN.load(Ordering::Relaxed);
    loop {
        let next = now.max(previous.saturating_add(1));
        match LAST_OPEN.compare_exchange_weak(previous, next, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => return next,
            Err(current) => previous = current,
        }
    }
}

// Native units preserve non-UTF8 Unix names and unpaired UTF-16 Windows names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativePath {
    platform: String,
    units: Vec<u32>,
}
impl NativePath {
    pub fn new(path: &Path) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            Self {
                platform: "unix".into(),
                units: path
                    .as_os_str()
                    .as_bytes()
                    .iter()
                    .map(|b| u32::from(*b))
                    .collect(),
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            Self {
                platform: "windows".into(),
                units: path.as_os_str().encode_wide().map(u32::from).collect(),
            }
        }
    }
    pub fn path(&self) -> Result<PathBuf> {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            ensure!(
                self.platform == "unix",
                "Settings contain a foreign platform path"
            );
            let bytes: Vec<u8> = self
                .units
                .iter()
                .map(|u| u8::try_from(*u))
                .collect::<std::result::Result<_, _>>()?;
            Ok(std::ffi::OsString::from_vec(bytes).into())
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStringExt;
            ensure!(
                self.platform == "windows",
                "Settings contain a foreign platform path"
            );
            let units: Vec<u16> = self
                .units
                .iter()
                .map(|u| u16::try_from(*u))
                .collect::<std::result::Result<_, _>>()?;
            Ok(std::ffi::OsString::from_wide(&units).into())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositorySettings {
    pub root: NativePath,
    #[serde(default)]
    pub last_opened: u64,
    pub view: ViewMode,
    pub history_limit: usize,
    pub graph_visible: bool,
    pub pr_filter: PrFilter,
}
impl RepositorySettings {
    fn capture(app: &App) -> Self {
        Self {
            root: NativePath::new(app.repository.root()),
            last_opened: 0,
            // These views need a fresh selected file/PR, never revive an old target.
            view: match app.view {
                ViewMode::Hunks | ViewMode::PrFiles => ViewMode::Files,
                view => view,
            },
            history_limit: app.history_limit.clamp(20, 10_000),
            graph_visible: app.graph_visible,
            pr_filter: PrFilter {
                limit: app.pr_filter.limit.clamp(1, 10_000),
                ..app.pr_filter.clone()
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Navigation {
    version: u32,
    pub repositories: Vec<RepositorySettings>,
}
impl Default for Navigation {
    fn default() -> Self {
        Self {
            version: 1,
            repositories: Vec::new(),
        }
    }
}
impl Navigation {
    pub fn remember(&mut self, app: &App, opened: bool) {
        let mut settings = RepositorySettings::capture(app);
        if let Some(index) = self
            .repositories
            .iter()
            .position(|s| s.root == settings.root)
        {
            settings.last_opened = self.repositories[index].last_opened;
            if app.view == ViewMode::RecentRepositories {
                settings = self.repositories[index].clone();
            }
            if !opened {
                self.repositories[index] = settings;
                return;
            }
            self.repositories.remove(index);
        }
        settings.last_opened = opened_at();
        self.repositories.insert(0, settings);
        self.repositories.truncate(50);
    }
    pub fn restore(&self, app: &mut App) {
        let root = NativePath::new(app.repository.root());
        if let Some(settings) = self.repositories.iter().find(|s| s.root == root) {
            app.view = settings.view;
            app.history_limit = settings.history_limit;
            app.graph_visible = settings.graph_visible;
            app.pr_filter = settings.pr_filter.clone();
            app.detail_text.clear();
        }
    }
    fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1,
            "Unsupported settings version {}",
            self.version
        );
        ensure!(self.repositories.len() <= 50, "Too many saved repositories");
        for (index, settings) in self.repositories.iter().enumerate() {
            let path = settings.root.path()?;
            ensure!(path.is_absolute(), "Saved repository path must be absolute");
            ensure!(
                !path.as_os_str().is_empty() && settings.root.units.iter().all(|u| *u != 0),
                "Invalid repository path"
            );
            ensure!(
                (20..=10_000).contains(&settings.history_limit),
                "Invalid saved history limit"
            );
            ensure!(
                settings.pr_filter.limit <= 10_000 && settings.pr_filter.search.len() <= 64 * 1024,
                "Invalid saved PR filter size"
            );
            settings.pr_filter.validate()?;
            ensure!(
                !matches!(settings.view, ViewMode::PrFiles | ViewMode::Hunks),
                "Saved view requires a fresh review target"
            );
            ensure!(
                !self.repositories[..index]
                    .iter()
                    .any(|s| s.root == settings.root),
                "Duplicate repository settings"
            );
        }
        Ok(())
    }
}

pub fn default_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("GIT_WIRDO_STATE_FILE") {
        ensure!(!path.is_empty(), "GIT_WIRDO_STATE_FILE is empty");
        return Ok(PathBuf::from(path));
    }
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(unix)]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|p| Path::new(p).is_absolute())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")));
    Ok(base
        .context("Cannot locate UI settings; set GIT_WIRDO_STATE_FILE")?
        .join("git-wirdo")
        .join("state.json"))
}

pub fn load(path: &Path) -> Result<Navigation> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Navigation::default());
        }
        Err(error) => return Err(error).context("Cannot read UI settings"),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= MAX_BYTES, "UI settings exceed 1 MiB");
    let navigation: Navigation = serde_json::from_slice(&bytes)
        .context("Invalid UI settings JSON; original file preserved")?;
    navigation.validate()?;
    Ok(navigation)
}

pub fn save(path: &Path, navigation: &Navigation) -> Result<()> {
    navigation.validate()?;
    let bytes = serde_json::to_vec_pretty(navigation)?;
    ensure!(bytes.len() as u64 <= MAX_BYTES, "UI settings exceed 1 MiB");
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let id = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".git-wirdo-{}-{id}.tmp", std::process::id()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    let result = (|| {
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path).context("Cannot replace UI settings")
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Merge only this session's changed entries, retaining other open clients' work.
pub struct Store {
    path: PathBuf,
    baseline: Navigation,
}
struct StateLock(fs::File);
impl Drop for StateLock {
    fn drop(&mut self) {
        // A fork/duplicated descriptor can retain an OS lock after this handle
        // closes. Explicitly release the lock before dropping the owned handle.
        let _ = self.0.unlock();
    }
}
impl Store {
    pub fn open(path: PathBuf) -> Result<(Self, Navigation)> {
        let navigation = load(&path)?;
        Ok((
            Self {
                path,
                baseline: navigation.clone(),
            },
            navigation,
        ))
    }
    pub fn persist(&mut self, navigation: &Navigation) -> Result<Navigation> {
        if navigation == &self.baseline {
            return Ok(navigation.clone());
        }
        navigation.validate()?;
        let parent = self
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;
        let mut lock_name = self.path.as_os_str().to_os_string();
        lock_name.push(".lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(PathBuf::from(lock_name))?;
        // Never remove another client's lock file.
        lock.try_lock()
            .context("UI settings are being saved by another client; retrying")?;
        let _guard = StateLock(lock);
        let mut merged = load(&self.path)?;
        for old in &self.baseline.repositories {
            if !navigation.repositories.iter().any(|s| s.root == old.root) {
                merged.repositories.retain(|s| s.root != old.root);
            }
        }
        for settings in navigation.repositories.iter().rev() {
            if self
                .baseline
                .repositories
                .iter()
                .find(|s| s.root == settings.root)
                != Some(settings)
                || self.baseline.repositories.first().map(|s| &s.root)
                    != navigation.repositories.first().map(|s| &s.root)
                    && navigation.repositories.first() == Some(settings)
            {
                merged.repositories.retain(|s| s.root != settings.root);
                merged.repositories.insert(0, settings.clone());
            }
        }
        merged
            .repositories
            .sort_by_key(|s| std::cmp::Reverse(s.last_opened));
        merged.repositories.truncate(50);
        save(&self.path, &merged)?;
        self.baseline = merged.clone();
        Ok(merged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_releases_lock_even_while_a_duplicate_descriptor_is_alive() {
        let directory = crate::test_support::TempDirectory::new();
        let path = directory.0.join("lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        file.try_lock().unwrap();
        let duplicate = file.try_clone().unwrap();
        drop(StateLock(file));
        let next = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        next.try_lock().unwrap();
        next.unlock().unwrap();
        drop(duplicate);
    }
}
