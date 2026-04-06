use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::{DirEntry, WalkDir};

use crate::paths::normalize_path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveryFact {
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub evidence: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositoryProfile {
    pub name: String,
    pub root: PathBuf,
    #[serde(default)]
    pub evidence: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DependencyRelationship {
    pub from: String,
    pub to: String,
    pub kind: String,
    #[serde(default)]
    pub evidence: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LayerDefinition {
    pub name: String,
    #[serde(default)]
    pub responsibilities: Vec<String>,
    #[serde(default)]
    pub paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LayeringProfile {
    pub summary: String,
    #[serde(default)]
    pub layers: Vec<LayerDefinition>,
    #[serde(default)]
    pub allowed_dependency_directions: Vec<String>,
    #[serde(default)]
    pub unresolved_ambiguities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DetectedCommand {
    pub label: String,
    pub command: Vec<String>,
    pub source: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CommandCatalog {
    #[serde(default)]
    pub build: Vec<DetectedCommand>,
    #[serde(default)]
    pub test: Vec<DetectedCommand>,
    #[serde(default)]
    pub dev: Vec<DetectedCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoverySourceFile {
    pub path: PathBuf,
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceDiscoveryScan {
    pub workspace_path: PathBuf,
    pub scanned_at: DateTime<Utc>,
    pub workspace_fingerprint: String,
    #[serde(default)]
    pub source_files: Vec<DiscoverySourceFile>,
    #[serde(default)]
    pub tech_stack: Vec<DiscoveryFact>,
    #[serde(default)]
    pub repositories: Vec<RepositoryProfile>,
    #[serde(default)]
    pub dependency_relationships: Vec<DependencyRelationship>,
    #[serde(default)]
    pub api_contracts: Vec<DiscoveryFact>,
    pub layering: LayeringProfile,
    #[serde(default)]
    pub user_journeys: Vec<DiscoveryFact>,
    #[serde(default)]
    pub e2e_test_cases: Vec<DiscoveryFact>,
    #[serde(default)]
    pub auth: Vec<DiscoveryFact>,
    #[serde(default)]
    pub coding_conventions: Vec<DiscoveryFact>,
    #[serde(default)]
    pub commands: CommandCatalog,
    #[serde(default)]
    pub scan_notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceProfile {
    pub workspace_path: PathBuf,
    pub generated_at: DateTime<Utc>,
    pub summary: String,
    #[serde(default)]
    pub key_concepts: Vec<String>,
    #[serde(default)]
    pub tech_stack: Vec<DiscoveryFact>,
    #[serde(default)]
    pub repositories: Vec<RepositoryProfile>,
    #[serde(default)]
    pub dependency_relationships: Vec<DependencyRelationship>,
    #[serde(default)]
    pub api_contracts: Vec<DiscoveryFact>,
    pub layering: LayeringProfile,
    #[serde(default)]
    pub user_journeys: Vec<DiscoveryFact>,
    #[serde(default)]
    pub e2e_test_cases: Vec<DiscoveryFact>,
    #[serde(default)]
    pub auth: Vec<DiscoveryFact>,
    #[serde(default)]
    pub coding_conventions: Vec<DiscoveryFact>,
    #[serde(default)]
    pub commands: CommandCatalog,
    #[serde(default)]
    pub risks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceDiscoveryRequest {
    pub scan: WorkspaceDiscoveryScan,
    #[serde(default)]
    pub previous_profile: Option<WorkspaceProfile>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceDiscoveryPhase {
    #[default]
    Idle,
    Scanning,
    ReusingCachedProfile,
    Polishing,
    UsingFallbackProfile,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceDiscoveryStatus {
    pub workspace_path: PathBuf,
    pub scan_path: PathBuf,
    pub profile_path: PathBuf,
    pub workspace_fingerprint: String,
    #[serde(default)]
    pub profile_fingerprint: Option<String>,
    pub last_scanned_at: DateTime<Utc>,
    #[serde(default)]
    pub last_refreshed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_refresh_error: Option<String>,
    #[serde(default)]
    pub used_fallback_profile: bool,
    #[serde(default)]
    pub current_phase: WorkspaceDiscoveryPhase,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceProfileSelection {
    pub profile: WorkspaceProfile,
    pub canonical_profile_path: PathBuf,
    pub scan_path: PathBuf,
    pub status_path: PathBuf,
    pub workspace_fingerprint: String,
    pub profile_fingerprint: String,
    pub last_scanned_at: DateTime<Utc>,
    pub last_refreshed_at: DateTime<Utc>,
    #[serde(default)]
    pub refresh_error: Option<String>,
    #[serde(default)]
    pub used_fallback_profile: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunWorkspaceProfileSnapshot {
    pub snapshot_path: PathBuf,
    pub canonical_profile_path: PathBuf,
    pub workspace_fingerprint: String,
    pub profile_fingerprint: String,
    pub last_scanned_at: DateTime<Utc>,
    pub last_refreshed_at: DateTime<Utc>,
    #[serde(default)]
    pub refresh_error: Option<String>,
    #[serde(default)]
    pub used_fallback_profile: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceDiscoveryPayload {
    pub status: WorkspaceDiscoveryStatus,
    #[serde(default)]
    pub profile_summary: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceDiscoveryStore {
    root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveryArtifactSet {
    pub root: PathBuf,
    pub prompt_file: PathBuf,
    pub output_file: PathBuf,
    pub stdout_log: PathBuf,
    pub stderr_log: PathBuf,
    pub result_file: PathBuf,
}

impl WorkspaceDiscoveryStore {
    pub fn new(workspace_path: impl AsRef<Path>) -> Self {
        Self {
            root: workspace_path.as_ref().join(".loopsmith").join("discovery"),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn scan_path(&self) -> PathBuf {
        self.root.join("scan.json")
    }

    pub fn profile_path(&self) -> PathBuf {
        self.root.join("profile.json")
    }

    pub fn status_path(&self) -> PathBuf {
        self.root.join("status.json")
    }

    pub fn worker_artifacts(&self) -> DiscoveryArtifactSet {
        let worker_root = self.root.join("worker");
        DiscoveryArtifactSet {
            prompt_file: worker_root.join("prompt.md"),
            output_file: worker_root.join("workspace-profile.json"),
            stdout_log: worker_root.join("stdout.log"),
            stderr_log: worker_root.join("stderr.log"),
            result_file: worker_root.join("result.json"),
            root: worker_root,
        }
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("failed to create {}", self.root.display()))?;
        let worker = self.worker_artifacts();
        fs::create_dir_all(&worker.root)
            .with_context(|| format!("failed to create {}", worker.root.display()))?;
        Ok(())
    }

    pub fn load_profile(&self) -> Result<Option<WorkspaceProfile>> {
        read_json_if_exists(&self.profile_path())
    }

    pub fn load_status(&self) -> Result<Option<WorkspaceDiscoveryStatus>> {
        read_json_if_exists(&self.status_path())
    }

    pub fn load_payload(&self) -> Result<Option<WorkspaceDiscoveryPayload>> {
        let Some(status) = self.load_status()? else {
            return Ok(None);
        };
        let profile_summary = self.load_profile()?.map(|profile| profile.summary);
        Ok(Some(WorkspaceDiscoveryPayload {
            status,
            profile_summary,
        }))
    }

    pub fn save_scan(&self, scan: &WorkspaceDiscoveryScan) -> Result<()> {
        self.ensure_dirs()?;
        write_json_pretty(&self.scan_path(), scan)
    }

    pub fn save_profile(&self, profile: &WorkspaceProfile) -> Result<()> {
        self.ensure_dirs()?;
        write_json_pretty(&self.profile_path(), profile)
    }

    pub fn save_status(&self, status: &WorkspaceDiscoveryStatus) -> Result<()> {
        self.ensure_dirs()?;
        write_json_pretty(&self.status_path(), status)
    }
}

impl WorkspaceDiscoveryRequest {
    pub fn synthesize_profile(&self) -> WorkspaceProfile {
        let scan = &self.scan;
        let primary_stack = scan
            .tech_stack
            .iter()
            .map(|entry| entry.title.as_str())
            .take(4)
            .collect::<Vec<_>>();
        let repo_count = scan.repositories.len().max(1);
        let mut key_concepts = Vec::new();

        if !primary_stack.is_empty() {
            key_concepts.push(format!("Primary stack: {}.", primary_stack.join(", ")));
        }
        if !scan.layering.layers.is_empty() {
            key_concepts.push(format!(
                "Detected layers: {}.",
                scan.layering
                    .layers
                    .iter()
                    .map(|layer| layer.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !scan.commands.test.is_empty() {
            key_concepts.push(format!(
                "Primary test commands: {}.",
                scan.commands
                    .test
                    .iter()
                    .take(3)
                    .map(|command| command.command.join(" "))
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        if !scan.auth.is_empty() {
            key_concepts.push("Authentication-related source files were detected.".to_string());
        }

        let mut risks = scan.scan_notes.clone();
        risks.extend(scan.layering.unresolved_ambiguities.clone());

        WorkspaceProfile {
            workspace_path: scan.workspace_path.clone(),
            generated_at: Utc::now(),
            summary: format!(
                "Workspace profile derived from {} source-of-truth files across {} detected repository root(s).",
                scan.source_files.len(),
                repo_count
            ),
            key_concepts,
            tech_stack: scan.tech_stack.clone(),
            repositories: scan.repositories.clone(),
            dependency_relationships: scan.dependency_relationships.clone(),
            api_contracts: scan.api_contracts.clone(),
            layering: scan.layering.clone(),
            user_journeys: scan.user_journeys.clone(),
            e2e_test_cases: scan.e2e_test_cases.clone(),
            auth: scan.auth.clone(),
            coding_conventions: scan.coding_conventions.clone(),
            commands: scan.commands.clone(),
            risks,
        }
    }
}

impl WorkspaceProfile {
    pub fn prompt_context(&self) -> String {
        let mut lines = vec![format!("- summary: {}", self.summary)];

        if !self.key_concepts.is_empty() {
            lines.push(format!(
                "- key_concepts: {}",
                self.key_concepts
                    .iter()
                    .take(6)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" | ")
            ));
        }
        if !self.repositories.is_empty() {
            lines.push(format!(
                "- repositories: {}",
                self.repositories
                    .iter()
                    .take(6)
                    .map(|repo| format!("{} ({})", repo.name, repo.root.display()))
                    .collect::<Vec<_>>()
                    .join(" | ")
            ));
        }
        if !self.layering.allowed_dependency_directions.is_empty() {
            lines.push(format!(
                "- layering_rules: {}",
                self.layering
                    .allowed_dependency_directions
                    .iter()
                    .take(6)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" | ")
            ));
        }
        if !self.api_contracts.is_empty() {
            lines.push(format!(
                "- api_contracts: {}",
                self.api_contracts
                    .iter()
                    .take(5)
                    .map(|fact| fact.title.clone())
                    .collect::<Vec<_>>()
                    .join(" | ")
            ));
        }
        if !self.commands.test.is_empty() {
            lines.push(format!(
                "- test_commands: {}",
                self.commands
                    .test
                    .iter()
                    .take(4)
                    .map(|command| command.command.join(" "))
                    .collect::<Vec<_>>()
                    .join(" | ")
            ));
        }
        if !self.commands.build.is_empty() {
            lines.push(format!(
                "- build_commands: {}",
                self.commands
                    .build
                    .iter()
                    .take(4)
                    .map(|command| command.command.join(" "))
                    .collect::<Vec<_>>()
                    .join(" | ")
            ));
        }
        if !self.auth.is_empty() {
            lines.push(format!(
                "- auth: {}",
                self.auth
                    .iter()
                    .take(4)
                    .map(|fact| fact.title.clone())
                    .collect::<Vec<_>>()
                    .join(" | ")
            ));
        }
        if !self.layering.unresolved_ambiguities.is_empty() {
            lines.push(format!(
                "- layering_ambiguities: {}",
                self.layering
                    .unresolved_ambiguities
                    .iter()
                    .take(4)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" | ")
            ));
        }

        lines.join("\n")
    }

    pub fn contract_scope_notes(&self) -> Vec<String> {
        let mut notes = Vec::new();

        for rule in self.layering.allowed_dependency_directions.iter().take(3) {
            notes.push(format!("Respect workspace layering: {rule}"));
        }

        if !self.coding_conventions.is_empty() {
            let conventions = self
                .coding_conventions
                .iter()
                .take(3)
                .map(|fact| fact.title.clone())
                .collect::<Vec<_>>()
                .join(", ");
            notes.push(format!(
                "Follow detected coding conventions from: {conventions}."
            ));
        }

        if !self.api_contracts.is_empty() {
            let contracts = self
                .api_contracts
                .iter()
                .take(3)
                .map(|fact| fact.title.clone())
                .collect::<Vec<_>>()
                .join(", ");
            notes.push(format!(
                "Preserve detected API contracts and exposed interfaces: {contracts}."
            ));
        }

        notes
    }
}

pub fn scan_workspace(workspace_path: &Path) -> Result<WorkspaceDiscoveryScan> {
    let workspace_path = normalize_path(workspace_path.to_path_buf());
    let scanned_at = Utc::now();
    let mut source_files = Vec::new();
    let mut tech_stack = Vec::new();
    let mut repositories = Vec::new();
    let mut dependency_relationships = Vec::new();
    let mut api_contracts = Vec::new();
    let mut user_journeys = Vec::new();
    let mut e2e_test_cases = Vec::new();
    let mut auth = Vec::new();
    let mut coding_conventions = Vec::new();
    let mut commands = CommandCatalog::default();
    let mut scan_notes = Vec::new();
    let mut layers = BTreeMap::<String, BTreeSet<PathBuf>>::new();
    let mut repo_roots = BTreeSet::<PathBuf>::new();

    for entry in WalkDir::new(&workspace_path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !should_skip_entry(entry))
    {
        let entry = entry.with_context(|| {
            format!(
                "failed while scanning workspace {}",
                workspace_path.display()
            )
        })?;
        let path = entry.path();

        if entry.file_type().is_dir() {
            if path.join(".git").exists() {
                repo_roots.insert(normalize_path(path.to_path_buf()));
            }
            continue;
        }

        if !is_candidate_file(path) {
            continue;
        }

        let rel = relative_to_workspace(&workspace_path, path);
        let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
        let content_hash = hash_bytes(&bytes);
        source_files.push(DiscoverySourceFile {
            path: rel.clone(),
            content_hash,
        });

        let text = String::from_utf8_lossy(&bytes);
        extend_layer_candidates(&mut layers, &rel);
        detect_conventions(&rel, &text, &mut coding_conventions);
        detect_user_journeys(&rel, &text, &mut user_journeys, &mut e2e_test_cases);
        detect_auth(&rel, &text, &mut auth);
        detect_api_contracts(&rel, &text, &mut api_contracts);
        detect_commands(
            &workspace_path,
            &rel,
            &text,
            &mut commands,
            &mut dependency_relationships,
            &mut tech_stack,
        )?;
        detect_stack_and_infra(&rel, &text, &mut tech_stack, &mut scan_notes);
    }

    if repo_roots.is_empty() {
        repositories.push(RepositoryProfile {
            name: workspace_path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| workspace_path.display().to_string()),
            root: PathBuf::from("."),
            evidence: source_files
                .iter()
                .take(3)
                .map(|file| file.path.clone())
                .collect(),
        });
    } else {
        repositories.extend(repo_roots.into_iter().map(|root| {
            RepositoryProfile {
                name: root
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| root.display().to_string()),
                root: relative_to_workspace(&workspace_path, &root),
                evidence: vec![relative_to_workspace(&workspace_path, &root.join(".git"))],
            }
        }));
    }

    source_files.sort_by(|left, right| left.path.cmp(&right.path));
    tech_stack.sort_by(|left, right| left.title.cmp(&right.title));
    repositories.sort_by(|left, right| left.root.cmp(&right.root));
    dependency_relationships.sort_by(|left, right| {
        left.from
            .cmp(&right.from)
            .then_with(|| left.to.cmp(&right.to))
            .then_with(|| left.kind.cmp(&right.kind))
    });
    api_contracts.sort_by(|left, right| left.title.cmp(&right.title));
    user_journeys.sort_by(|left, right| left.title.cmp(&right.title));
    e2e_test_cases.sort_by(|left, right| left.title.cmp(&right.title));
    auth.sort_by(|left, right| left.title.cmp(&right.title));
    coding_conventions.sort_by(|left, right| left.title.cmp(&right.title));
    dedupe_facts(&mut tech_stack);
    dedupe_facts(&mut api_contracts);
    dedupe_facts(&mut user_journeys);
    dedupe_facts(&mut e2e_test_cases);
    dedupe_facts(&mut auth);
    dedupe_facts(&mut coding_conventions);
    dedupe_commands(&mut commands);
    dedupe_relationships(&mut dependency_relationships);

    let layering = build_layering_profile(layers);
    let workspace_fingerprint = hash_json(&source_files)?;

    Ok(WorkspaceDiscoveryScan {
        workspace_path,
        scanned_at,
        workspace_fingerprint,
        source_files,
        tech_stack,
        repositories,
        dependency_relationships,
        api_contracts,
        layering,
        user_journeys,
        e2e_test_cases,
        auth,
        coding_conventions,
        commands,
        scan_notes,
    })
}

pub fn profile_fingerprint(profile: &WorkspaceProfile) -> Result<String> {
    hash_json(profile)
}

fn should_skip_entry(entry: &DirEntry) -> bool {
    let Some(name) = entry.file_name().to_str() else {
        return true;
    };

    if entry.file_type().is_dir() {
        return matches!(
            name,
            ".git"
                | ".loopsmith-runs"
                | ".loopsmith"
                | "node_modules"
                | "target"
                | "dist"
                | "build"
                | "coverage"
                | ".next"
                | ".venv"
                | "__pycache__"
        );
    }

    false
}

fn is_candidate_file(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let lower = file_name.to_ascii_lowercase();
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .unwrap_or_default();
    let path_text = path.to_string_lossy().to_ascii_lowercase();

    lower == "cargo.toml"
        || lower == "cargo.lock"
        || lower == "package.json"
        || lower == "package-lock.json"
        || lower == "pnpm-lock.yaml"
        || lower == "yarn.lock"
        || lower == "makefile"
        || lower == "dockerfile"
        || lower == ".editorconfig"
        || lower == "rustfmt.toml"
        || lower.starts_with(".eslintrc")
        || lower.starts_with("eslint.config.")
        || lower == "agents.md"
        || lower == "agents.md".to_ascii_lowercase()
        || lower == ".cursorrules"
        || lower.starts_with(".claude")
        || lower.starts_with("openapi")
        || lower.starts_with("swagger")
        || lower.ends_with(".openapi.json")
        || matches!(
            extension.as_str(),
            "proto" | "graphql" | "gql" | "toml" | "yml" | "yaml" | "md"
        )
        || path_text.contains("/tests/")
        || path_text.contains("/test/")
        || path_text.contains("/e2e/")
        || path_text.contains("/playwright/")
        || path_text.contains("/cypress/")
        || path_text.contains("/scripts/")
        || path_text.contains("/auth")
        || path_text.contains("/api")
        || path_text.contains("/routes")
        || lower == "lib.rs"
        || lower == "mod.rs"
        || lower == "main.rs"
        || lower == "index.ts"
        || lower == "index.tsx"
        || lower == "app.ts"
        || lower == "app.tsx"
}

fn relative_to_workspace(workspace: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(workspace)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| path.to_path_buf())
}

fn extend_layer_candidates(layers: &mut BTreeMap<String, BTreeSet<PathBuf>>, rel: &Path) {
    let lower = rel.to_string_lossy().to_ascii_lowercase();
    let layer = if lower.contains("/ui/") || lower.contains("/web/") || lower.contains("ui") {
        Some("ui")
    } else if lower.contains("/service/") || lower.contains("/application/") {
        Some("service")
    } else if lower.contains("/domain/") || lower.contains("/core/") {
        Some("core")
    } else if lower.contains("/worker/") || lower.contains("/adapter/") || lower.contains("/cli/") {
        Some("adapter")
    } else if lower.contains("/infra/") || lower.contains("/infrastructure/") {
        Some("infra")
    } else {
        None
    };

    if let Some(layer) = layer {
        layers
            .entry(layer.to_string())
            .or_default()
            .insert(rel.to_path_buf());
    }
}

fn detect_conventions(rel: &Path, text: &str, output: &mut Vec<DiscoveryFact>) {
    let Some(name) = rel.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    let lower = name.to_ascii_lowercase();
    if lower == ".editorconfig"
        || lower == "rustfmt.toml"
        || lower.starts_with(".eslintrc")
        || lower.starts_with("eslint.config.")
        || lower == "agents.md"
        || lower == ".cursorrules"
        || lower.starts_with(".claude")
    {
        output.push(DiscoveryFact {
            title: name.to_string(),
            summary: summarize_lines(text, 3),
            evidence: vec![rel.to_path_buf()],
        });
    }
}

fn detect_user_journeys(
    rel: &Path,
    text: &str,
    journeys: &mut Vec<DiscoveryFact>,
    e2e: &mut Vec<DiscoveryFact>,
) {
    let path_text = rel.to_string_lossy().to_ascii_lowercase();
    if path_text.contains("journey") || text.to_ascii_lowercase().contains("journey") {
        journeys.push(DiscoveryFact {
            title: format!("User journey evidence in {}", rel.display()),
            summary: summarize_lines_matching(text, &["journey", "user journey"], 3)
                .unwrap_or_else(|| summarize_lines(text, 3)),
            evidence: vec![rel.to_path_buf()],
        });
    }

    if path_text.contains("e2e")
        || path_text.contains("playwright")
        || path_text.contains("cypress")
        || path_text.contains("live_codex")
        || path_text.contains("run_smoke")
        || text.contains("CODEX_LIVE_E2E")
    {
        e2e.push(DiscoveryFact {
            title: format!("E2E evidence in {}", rel.display()),
            summary: summarize_lines(text, 3),
            evidence: vec![rel.to_path_buf()],
        });
    }
}

fn detect_auth(rel: &Path, text: &str, output: &mut Vec<DiscoveryFact>) {
    let path_text = rel.to_string_lossy().to_ascii_lowercase();
    let keywords = ["auth", "oauth", "jwt", "session", "token", "oidc", "openid"];
    if keywords.iter().any(|keyword| path_text.contains(keyword))
        || keywords
            .iter()
            .any(|keyword| text.to_ascii_lowercase().contains(keyword))
    {
        output.push(DiscoveryFact {
            title: format!("Auth evidence in {}", rel.display()),
            summary: summarize_lines_matching(text, &keywords, 3)
                .unwrap_or_else(|| summarize_lines(text, 3)),
            evidence: vec![rel.to_path_buf()],
        });
    }
}

fn detect_api_contracts(rel: &Path, text: &str, output: &mut Vec<DiscoveryFact>) {
    let path_text = rel.to_string_lossy().to_ascii_lowercase();
    if matches!(
        rel.extension().and_then(|ext| ext.to_str()),
        Some("proto" | "graphql" | "gql")
    ) || path_text.contains("openapi")
        || path_text.contains("swagger")
    {
        output.push(DiscoveryFact {
            title: format!("Explicit API contract {}", rel.display()),
            summary: summarize_lines(text, 3),
            evidence: vec![rel.to_path_buf()],
        });
        return;
    }

    if let Some(exports) = summarize_module_exports(rel, text) {
        output.push(DiscoveryFact {
            title: format!("Exposed module interfaces in {}", rel.display()),
            summary: exports,
            evidence: vec![rel.to_path_buf()],
        });
    }

    if let Some(routes) = summarize_http_routes(text) {
        output.push(DiscoveryFact {
            title: format!("HTTP route evidence in {}", rel.display()),
            summary: routes,
            evidence: vec![rel.to_path_buf()],
        });
    }
}

fn detect_commands(
    workspace_path: &Path,
    rel: &Path,
    text: &str,
    commands: &mut CommandCatalog,
    dependency_relationships: &mut Vec<DependencyRelationship>,
    tech_stack: &mut Vec<DiscoveryFact>,
) -> Result<()> {
    let Some(name) = rel.file_name().and_then(|name| name.to_str()) else {
        return Ok(());
    };
    let lower = name.to_ascii_lowercase();

    if lower == "makefile" {
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((target, _)) = trimmed.split_once(':') else {
                continue;
            };
            if target.contains(' ') || target.starts_with('.') {
                continue;
            }
            let label = target.trim().to_string();
            let detected = DetectedCommand {
                label: label.clone(),
                command: vec!["make".to_string(), label.clone()],
                source: rel.to_path_buf(),
            };
            classify_command(commands, detected);
        }
        tech_stack.push(DiscoveryFact {
            title: "Make".to_string(),
            summary: format!("Detected Makefile targets in {}.", rel.display()),
            evidence: vec![rel.to_path_buf()],
        });
    } else if lower == "package.json" {
        let package: serde_json::Value = serde_json::from_str(text)
            .with_context(|| format!("failed to parse {}", rel.display()))?;

        if let Some(package_name) = package.get("name").and_then(serde_json::Value::as_str) {
            tech_stack.push(DiscoveryFact {
                title: "Node.js package".to_string(),
                summary: format!("Package `{package_name}` declared in {}.", rel.display()),
                evidence: vec![rel.to_path_buf()],
            });
        }

        if package.get("dependencies").is_some() || package.get("devDependencies").is_some() {
            tech_stack.push(DiscoveryFact {
                title: "JavaScript / TypeScript".to_string(),
                summary: extract_package_dependencies(&package),
                evidence: vec![rel.to_path_buf()],
            });
        }

        if let Some(scripts) = package
            .get("scripts")
            .and_then(serde_json::Value::as_object)
        {
            for (label, value) in scripts {
                let Some(command_str) = value.as_str() else {
                    continue;
                };
                let detected = DetectedCommand {
                    label: label.clone(),
                    command: vec!["npm".to_string(), "run".to_string(), label.clone()],
                    source: rel.to_path_buf(),
                };
                classify_command(commands, detected);
                if label.contains("dev") || label.contains("start") {
                    commands.dev.push(DetectedCommand {
                        label: format!("{label} ({command_str})"),
                        command: split_shell_words(command_str),
                        source: rel.to_path_buf(),
                    });
                }
            }
        }

        for section in ["dependencies", "devDependencies"] {
            if let Some(deps) = package.get(section).and_then(serde_json::Value::as_object) {
                for (dep_name, dep_value) in deps {
                    let Some(spec) = dep_value.as_str() else {
                        continue;
                    };
                    if spec.starts_with("file:") || spec.starts_with("workspace:") {
                        dependency_relationships.push(DependencyRelationship {
                            from: rel
                                .parent()
                                .unwrap_or_else(|| Path::new("."))
                                .display()
                                .to_string(),
                            to: dep_name.clone(),
                            kind: "npm_local_dependency".to_string(),
                            evidence: vec![rel.to_path_buf()],
                        });
                    }
                }
            }
        }
    } else if lower == "cargo.toml" {
        let cargo: toml::Value =
            toml::from_str(text).with_context(|| format!("failed to parse {}", rel.display()))?;
        tech_stack.push(DiscoveryFact {
            title: "Rust".to_string(),
            summary: extract_cargo_dependencies(&cargo),
            evidence: vec![rel.to_path_buf()],
        });
        commands.build.push(DetectedCommand {
            label: format!("cargo build ({})", rel.display()),
            command: vec!["cargo".to_string(), "build".to_string()],
            source: rel.to_path_buf(),
        });
        commands.test.push(DetectedCommand {
            label: format!("cargo test ({})", rel.display()),
            command: vec!["cargo".to_string(), "test".to_string()],
            source: rel.to_path_buf(),
        });

        if let Some(workspace_members) = cargo
            .get("workspace")
            .and_then(|workspace| workspace.get("members"))
            .and_then(toml::Value::as_array)
        {
            for member in workspace_members.iter().filter_map(toml::Value::as_str) {
                dependency_relationships.push(DependencyRelationship {
                    from: rel
                        .parent()
                        .unwrap_or_else(|| Path::new("."))
                        .display()
                        .to_string(),
                    to: member.to_string(),
                    kind: "cargo_workspace_member".to_string(),
                    evidence: vec![rel.to_path_buf()],
                });
            }
        }

        for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
            if let Some(table) = cargo.get(section).and_then(toml::Value::as_table) {
                for (name, value) in table {
                    let Some(path) = value
                        .get("path")
                        .and_then(toml::Value::as_str)
                        .map(PathBuf::from)
                    else {
                        continue;
                    };
                    let dep_path = normalize_path(
                        workspace_path
                            .join(rel.parent().unwrap_or_else(|| Path::new(".")))
                            .join(path),
                    );
                    dependency_relationships.push(DependencyRelationship {
                        from: rel
                            .parent()
                            .unwrap_or_else(|| Path::new("."))
                            .display()
                            .to_string(),
                        to: relative_to_workspace(workspace_path, &dep_path)
                            .display()
                            .to_string(),
                        kind: format!("cargo_{section}"),
                        evidence: vec![rel.to_path_buf()],
                    });
                    tech_stack.push(DiscoveryFact {
                        title: format!("Rust dependency `{name}`"),
                        summary: format!(
                            "Local path dependency from {} to {}.",
                            rel.display(),
                            relative_to_workspace(workspace_path, &dep_path).display()
                        ),
                        evidence: vec![rel.to_path_buf()],
                    });
                }
            }
        }
    }

    Ok(())
}

fn detect_stack_and_infra(
    rel: &Path,
    text: &str,
    tech_stack: &mut Vec<DiscoveryFact>,
    notes: &mut Vec<String>,
) {
    let path_text = rel.to_string_lossy().to_ascii_lowercase();
    if path_text.contains("docker")
        || path_text.ends_with("compose.yaml")
        || path_text.ends_with("compose.yml")
    {
        tech_stack.push(DiscoveryFact {
            title: "Container infrastructure".to_string(),
            summary: format!(
                "Container/runtime definition detected in {}.",
                rel.display()
            ),
            evidence: vec![rel.to_path_buf()],
        });
    }
    if path_text.ends_with(".tf") {
        tech_stack.push(DiscoveryFact {
            title: "Terraform".to_string(),
            summary: format!("Terraform infrastructure detected in {}.", rel.display()),
            evidence: vec![rel.to_path_buf()],
        });
    }
    if path_text.contains(".github/workflows") {
        tech_stack.push(DiscoveryFact {
            title: "CI/CD workflow".to_string(),
            summary: summarize_lines(text, 3),
            evidence: vec![rel.to_path_buf()],
        });
    }
    if path_text.contains(".loopsmith/config.toml") {
        notes.push(format!(
            "Harness-local LoopSmith config detected at {} and included in command/context extraction.",
            rel.display()
        ));
    }
}

fn classify_command(commands: &mut CommandCatalog, detected: DetectedCommand) {
    let label = detected.label.to_ascii_lowercase();
    if label.contains("test") || label.contains("e2e") || label.contains("journey") {
        commands.test.push(detected);
    } else if label.contains("dev") || label.contains("start") || label.contains("serve") {
        commands.dev.push(detected);
    } else {
        commands.build.push(detected);
    }
}

fn extract_package_dependencies(package: &serde_json::Value) -> String {
    let mut names = Vec::new();
    for section in ["dependencies", "devDependencies"] {
        if let Some(deps) = package.get(section).and_then(serde_json::Value::as_object) {
            names.extend(deps.keys().take(5).cloned());
        }
    }
    if names.is_empty() {
        "Detected package manifest with no dependency list.".to_string()
    } else {
        format!("Detected dependencies/frameworks: {}.", names.join(", "))
    }
}

fn extract_cargo_dependencies(cargo: &toml::Value) -> String {
    let mut names = Vec::new();
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(table) = cargo.get(section).and_then(toml::Value::as_table) {
            names.extend(table.keys().take(5).cloned());
        }
    }
    if names.is_empty() {
        "Detected Cargo manifest with no dependency list.".to_string()
    } else {
        format!("Detected crates/frameworks: {}.", names.join(", "))
    }
}

fn summarize_module_exports(rel: &Path, text: &str) -> Option<String> {
    let mut exports = Vec::new();
    if rel.extension().and_then(|ext| ext.to_str()) == Some("rs") {
        for line in text.lines() {
            let trimmed = line.trim_start();
            if let Some(symbol) = trimmed.strip_prefix("pub fn ") {
                exports.push(format!(
                    "fn {}",
                    symbol.split('(').next().unwrap_or(symbol).trim()
                ));
            } else if let Some(symbol) = trimmed.strip_prefix("pub struct ") {
                exports.push(format!(
                    "struct {}",
                    symbol
                        .split(|ch: char| ch == '{' || ch.is_whitespace())
                        .next()
                        .unwrap_or(symbol)
                ));
            } else if let Some(symbol) = trimmed.strip_prefix("pub enum ") {
                exports.push(format!(
                    "enum {}",
                    symbol
                        .split(|ch: char| ch == '{' || ch.is_whitespace())
                        .next()
                        .unwrap_or(symbol)
                ));
            } else if let Some(symbol) = trimmed.strip_prefix("pub trait ") {
                exports.push(format!(
                    "trait {}",
                    symbol
                        .split(|ch: char| ch == '{' || ch.is_whitespace())
                        .next()
                        .unwrap_or(symbol)
                ));
            }
            if exports.len() >= 5 {
                break;
            }
        }
    } else if matches!(
        rel.extension().and_then(|ext| ext.to_str()),
        Some("ts" | "tsx" | "js" | "jsx")
    ) {
        for line in text.lines() {
            let trimmed = line.trim_start();
            for prefix in [
                "export function ",
                "export async function ",
                "export const ",
                "export interface ",
                "export type ",
                "export class ",
            ] {
                if let Some(symbol) = trimmed.strip_prefix(prefix) {
                    exports.push(
                        symbol
                            .split(|ch: char| {
                                ch == '(' || ch == '=' || ch == '{' || ch.is_whitespace()
                            })
                            .next()
                            .unwrap_or(symbol)
                            .to_string(),
                    );
                    break;
                }
            }
            if exports.len() >= 5 {
                break;
            }
        }
    }

    if exports.is_empty() {
        None
    } else {
        Some(format!("Exports/interfaces: {}.", exports.join(", ")))
    }
}

fn summarize_http_routes(text: &str) -> Option<String> {
    let mut routes = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.contains(".get(")
            || trimmed.contains(".post(")
            || trimmed.contains(".put(")
            || trimmed.contains(".delete(")
            || trimmed.contains("route(\"/")
            || trimmed.contains("axum::routing::")
        {
            routes.push(trimmed.to_string());
        }
        if routes.len() >= 3 {
            break;
        }
    }

    if routes.is_empty() {
        None
    } else {
        Some(format!("Route signatures: {}.", routes.join(" | ")))
    }
}

fn summarize_lines(text: &str, limit: usize) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(limit)
        .collect::<Vec<_>>()
        .join(" | ")
}

fn summarize_lines_matching(text: &str, keywords: &[&str], limit: usize) -> Option<String> {
    let lower_keywords = keywords
        .iter()
        .map(|keyword| keyword.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let matched = text
        .lines()
        .map(str::trim)
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            !line.is_empty() && lower_keywords.iter().any(|keyword| lower.contains(keyword))
        })
        .take(limit)
        .collect::<Vec<_>>();
    if matched.is_empty() {
        None
    } else {
        Some(matched.join(" | "))
    }
}

fn split_shell_words(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>()
}

fn build_layering_profile(layers: BTreeMap<String, BTreeSet<PathBuf>>) -> LayeringProfile {
    if layers.is_empty() {
        return LayeringProfile {
            summary:
                "No strong layer names were detected from directory and file naming heuristics."
                    .to_string(),
            layers: Vec::new(),
            allowed_dependency_directions: Vec::new(),
            unresolved_ambiguities: vec![
                "Layering could not be derived confidently from file layout alone.".to_string(),
            ],
        };
    }

    let layers = layers
        .into_iter()
        .map(|(name, paths)| LayerDefinition {
            responsibilities: default_responsibilities_for_layer(&name),
            name,
            paths: paths.into_iter().take(8).collect(),
        })
        .collect::<Vec<_>>();

    LayeringProfile {
        summary: format!(
            "Detected architectural layers: {}.",
            layers
                .iter()
                .map(|layer| layer.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        layers,
        allowed_dependency_directions: vec![
            "UI and interface layers may depend inward on service and core layers, not the reverse."
                .to_string(),
            "Core and domain layers must stay free of direct UI dependencies.".to_string(),
            "Adapter and infrastructure layers should depend on core contracts; core should not depend on adapters.".to_string(),
        ],
        unresolved_ambiguities: Vec::new(),
    }
}

fn default_responsibilities_for_layer(name: &str) -> Vec<String> {
    match name {
        "ui" => vec!["User-facing surfaces and presentation concerns.".to_string()],
        "service" => vec!["Application orchestration and workflow coordination.".to_string()],
        "core" => vec!["Durable domain logic and shared invariants.".to_string()],
        "adapter" => vec!["CLI, worker, or integration adapter boundaries.".to_string()],
        "infra" => vec!["Infrastructure and environment integration code.".to_string()],
        _ => Vec::new(),
    }
}

fn dedupe_facts(items: &mut Vec<DiscoveryFact>) {
    let mut seen = BTreeSet::new();
    items.retain(|item| seen.insert((item.title.clone(), item.summary.clone())));
}

fn dedupe_commands(commands: &mut CommandCatalog) {
    dedupe_command_list(&mut commands.build);
    dedupe_command_list(&mut commands.test);
    dedupe_command_list(&mut commands.dev);
}

fn dedupe_command_list(commands: &mut Vec<DetectedCommand>) {
    let mut seen = BTreeSet::new();
    commands.retain(|command| {
        seen.insert((
            command.label.clone(),
            command.command.clone(),
            command.source.clone(),
        ))
    });
}

fn dedupe_relationships(relationships: &mut Vec<DependencyRelationship>) {
    let mut seen = BTreeSet::new();
    relationships.retain(|relationship| {
        seen.insert((
            relationship.from.clone(),
            relationship.to.clone(),
            relationship.kind.clone(),
        ))
    });
}

fn write_json_pretty<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value).context("failed to serialize json")?;
    fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))
}

fn read_json_if_exists<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Option<T>> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let value = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(value))
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn hash_json<T: Serialize>(value: &T) -> Result<String> {
    let bytes = serde_json::to_vec(value).context("failed to serialize hash input")?;
    Ok(hash_bytes(&bytes))
}

#[cfg(test)]
mod tests {
    use super::{CommandCatalog, WorkspaceDiscoveryRequest, profile_fingerprint, scan_workspace};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn scanner_detects_mixed_rust_and_node_workspace() {
        let temp = tempdir().expect("tempdir");
        fs::write(
            temp.path().join("Cargo.toml"),
            "[package]\nname = \"mixed\"\nversion = \"0.1.0\"\n[dependencies]\nserde = \"1\"\n",
        )
        .expect("write cargo");
        fs::write(
            temp.path().join("package.json"),
            r#"{"name":"mixed-ui","scripts":{"test":"vitest","dev":"vite"},"dependencies":{"react":"18.0.0"}}"#,
        )
        .expect("write package");

        let scan = scan_workspace(temp.path()).expect("scan");
        assert!(
            scan.tech_stack
                .iter()
                .any(|fact| fact.title.contains("Rust"))
        );
        assert!(
            scan.tech_stack
                .iter()
                .any(|fact| fact.title.contains("JavaScript") || fact.title.contains("Node"))
        );
        assert!(
            scan.commands
                .test
                .iter()
                .any(|command| command.command == vec!["cargo", "test"])
        );
        assert!(
            scan.commands
                .dev
                .iter()
                .any(|command| command.label.contains("dev"))
        );
    }

    #[test]
    fn scanner_detects_local_dependencies_and_api_contracts() {
        let temp = tempdir().expect("tempdir");
        let crate_a = temp.path().join("crate-a");
        let crate_b = temp.path().join("crate-b");
        fs::create_dir_all(crate_a.join("src")).expect("crate-a src");
        fs::create_dir_all(crate_b.join("src")).expect("crate-b src");
        fs::write(
            crate_a.join("Cargo.toml"),
            "[package]\nname = \"crate-a\"\nversion = \"0.1.0\"\n[dependencies]\ncrate-b = { path = \"../crate-b\" }\n",
        )
        .expect("write cargo a");
        fs::write(
            crate_b.join("Cargo.toml"),
            "[package]\nname = \"crate-b\"\nversion = \"0.1.0\"\n",
        )
        .expect("write cargo b");
        fs::write(
            crate_b.join("src/lib.rs"),
            "pub struct PublicApi;\npub fn greet() {}\n",
        )
        .expect("write lib");

        let scan = scan_workspace(temp.path()).expect("scan");
        assert!(
            scan.dependency_relationships
                .iter()
                .any(|relationship| relationship.kind.starts_with("cargo_"))
        );
        assert!(
            scan.api_contracts
                .iter()
                .any(|fact| fact.summary.contains("Exports/interfaces"))
        );
    }

    #[test]
    fn request_synthesizes_profile_and_fingerprint() {
        let temp = tempdir().expect("tempdir");
        fs::write(temp.path().join("Makefile"), "test:\n\tcargo test\n").expect("makefile");
        let scan = scan_workspace(temp.path()).expect("scan");
        let request = WorkspaceDiscoveryRequest {
            scan,
            previous_profile: None,
        };

        let profile = request.synthesize_profile();
        let fingerprint = profile_fingerprint(&profile).expect("fingerprint");
        assert!(!fingerprint.is_empty());
        assert!(!profile.summary.is_empty());
        assert!(matches!(profile.commands, CommandCatalog { .. }));
    }
}
