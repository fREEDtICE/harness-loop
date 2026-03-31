use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use loopsmith_core::domain::PromptOverrides;
use serde::{Deserialize, Serialize};

/// Persisted launcher state for a single workspace quick link.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceProfile {
    pub workspace_path: PathBuf,
    pub display_name: String,
    pub preferred_config_path: Option<PathBuf>,
    pub request_draft: String,
    #[serde(default)]
    pub prompt_overrides: PromptOverrides,
    pub last_opened_at: DateTime<Utc>,
    pub last_run_root: Option<PathBuf>,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct WorkspaceProfileDocument {
    #[serde(default)]
    profiles: Vec<WorkspaceProfile>,
}

/// File-backed quick-link store used by the UI.
#[derive(Debug, Clone)]
pub struct WorkspaceProfileStore {
    path: PathBuf,
}

impl WorkspaceProfile {
    pub fn new(workspace_path: PathBuf) -> Self {
        let display_name = workspace_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| workspace_path.display().to_string());

        Self {
            workspace_path,
            display_name,
            preferred_config_path: None,
            request_draft: String::new(),
            prompt_overrides: PromptOverrides::default(),
            last_opened_at: Utc::now(),
            last_run_root: None,
            pinned: false,
        }
    }
}

impl WorkspaceProfileStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<Vec<WorkspaceProfile>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let bytes = fs::read(&self.path)
            .with_context(|| format!("failed to read {}", self.path.display()))?;
        let mut document: WorkspaceProfileDocument = serde_json::from_slice(&bytes)
            .with_context(|| format!("failed to parse {}", self.path.display()))?;
        document
            .profiles
            .sort_by(|left, right| right.last_opened_at.cmp(&left.last_opened_at));
        Ok(document.profiles)
    }

    pub fn save(&self, profiles: &[WorkspaceProfile]) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let mut sorted = profiles.to_vec();
        sorted.sort_by(|left, right| right.last_opened_at.cmp(&left.last_opened_at));
        let bytes = serde_json::to_vec_pretty(&WorkspaceProfileDocument { profiles: sorted })
            .context("failed to serialize workspace profiles")?;
        fs::write(&self.path, bytes)
            .with_context(|| format!("failed to write {}", self.path.display()))
    }

    pub fn upsert(&self, profile: WorkspaceProfile) -> Result<Vec<WorkspaceProfile>> {
        let mut profiles = self.load()?;
        if let Some(existing) = profiles
            .iter_mut()
            .find(|entry| entry.workspace_path == profile.workspace_path)
        {
            *existing = profile;
        } else {
            profiles.push(profile);
        }
        self.save(&profiles)?;
        self.load()
    }

    pub fn remove(&self, workspace_path: &Path) -> Result<Vec<WorkspaceProfile>> {
        let mut profiles = self.load()?;
        profiles.retain(|profile| profile.workspace_path != workspace_path);
        self.save(&profiles)?;
        self.load()
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{WorkspaceProfile, WorkspaceProfileStore};

    #[test]
    fn upsert_and_remove_profiles_round_trip() {
        let temp = tempdir().expect("tempdir");
        let store = WorkspaceProfileStore::new(temp.path().join("profiles.json"));

        let profile = WorkspaceProfile::new(temp.path().join("workspace-a"));
        let profiles = store.upsert(profile.clone()).expect("upsert");
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].workspace_path, profile.workspace_path);

        let loaded = store.load().expect("load");
        assert_eq!(loaded.len(), 1);

        let remaining = store.remove(&profile.workspace_path).expect("remove");
        assert!(remaining.is_empty());
    }
}
