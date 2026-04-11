use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::{DirEntry, WalkDir};

use crate::paths::normalize_path;

/// Information-value tier for a discovery fact.
///
/// Ordered from highest negentropy (most constraining, fewest bits needed to
/// eliminate large swaths of behaviour space) to lowest.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NegentropyTier {
    /// L1 – Specifications / conventions / contracts that constrain all code.
    Specification,
    /// L2 – Verification baselines (E2E, smoke, user-journey tests).
    Verification,
    /// L3 – Structural facts (tech stack, deps, commands, topology).
    Structure,
    /// L4 – Implementation-level observations.
    Implementation,
}

impl Default for NegentropyTier {
    fn default() -> Self {
        Self::Structure
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveryFact {
    #[serde(default)]
    pub id: String,
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub evidence: Vec<PathBuf>,
    #[serde(default)]
    pub tier: NegentropyTier,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositoryProfile {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub root: PathBuf,
    #[serde(default)]
    pub evidence: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DependencyRelationship {
    #[serde(default)]
    pub id: String,
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
    #[serde(default)]
    pub id: String,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ChangeBoundaryProfile {
    pub frozen_paths: Vec<PathBuf>,
    pub high_risk_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoverySourceFile {
    #[serde(default)]
    pub id: String,
    pub path: PathBuf,
    pub content_hash: String,
}

pub type WorkspaceDiscoveryEvidence = WorkspaceDiscoveryScan;

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
    #[serde(default)]
    pub project_intent: Vec<DiscoveryFact>,
    #[serde(default)]
    pub environment_requirements: Vec<DiscoveryFact>,
    #[serde(default)]
    pub change_boundaries: ChangeBoundaryProfile,
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
    #[serde(default)]
    pub project_intent: Vec<DiscoveryFact>,
    #[serde(default)]
    pub environment_requirements: Vec<DiscoveryFact>,
    #[serde(default)]
    pub change_boundaries: ChangeBoundaryProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceDiscoveryRequest {
    pub scan: WorkspaceDiscoveryScan,
    #[serde(default)]
    pub previous_profile: Option<WorkspaceProfile>,
    #[serde(default)]
    pub previous_inference: Option<WorkspaceDiscoveryInference>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryEvidenceChainStrength {
    Weak,
    Moderate,
    Strong,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveryEvidenceChain {
    pub label: String,
    pub strength: DiscoveryEvidenceChainStrength,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveryInference {
    pub id: String,
    pub category: String,
    pub statement: String,
    pub confidence: u8,
    pub rationale: String,
    #[serde(default)]
    pub evidence_chains: Vec<DiscoveryEvidenceChain>,
    #[serde(default)]
    pub assumptions: Vec<String>,
    #[serde(default)]
    pub contradictions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceDiscoveryInference {
    pub workspace_path: PathBuf,
    pub generated_at: DateTime<Utc>,
    pub summary: String,
    #[serde(default)]
    pub inferences: Vec<DiscoveryInference>,
    #[serde(default)]
    pub risks: Vec<String>,
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
    #[serde(default)]
    pub evidence_path: PathBuf,
    pub profile_path: PathBuf,
    #[serde(default)]
    pub inference_path: PathBuf,
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
    #[serde(default)]
    pub phase_heartbeat_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceProfileSelection {
    pub profile: WorkspaceProfile,
    pub canonical_profile_path: PathBuf,
    pub scan_path: PathBuf,
    #[serde(default)]
    pub evidence_path: PathBuf,
    #[serde(default)]
    pub inference_path: PathBuf,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkspaceDiscoveryPayload {
    pub status: WorkspaceDiscoveryStatus,
    #[serde(default)]
    pub profile_summary: Option<String>,
    #[serde(default)]
    pub inference_summary: Option<String>,
    #[serde(default)]
    pub overview: WorkspaceDiscoveryOverview,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct WorkspaceDiscoveryOverview {
    #[serde(default)]
    pub source_file_count: usize,
    #[serde(default)]
    pub repository_count: usize,
    #[serde(default)]
    pub dependency_relationship_count: usize,
    #[serde(default)]
    pub layer_count: usize,
    #[serde(default)]
    pub api_contract_count: usize,
    #[serde(default)]
    pub user_journey_count: usize,
    #[serde(default)]
    pub e2e_test_case_count: usize,
    #[serde(default)]
    pub auth_surface_count: usize,
    #[serde(default)]
    pub coding_convention_count: usize,
    #[serde(default)]
    pub build_command_count: usize,
    #[serde(default)]
    pub test_command_count: usize,
    #[serde(default)]
    pub dev_command_count: usize,
    #[serde(default)]
    pub tech_stack: Vec<String>,
    #[serde(default)]
    pub key_concepts: Vec<String>,
    #[serde(default)]
    pub repositories: Vec<String>,
    #[serde(default)]
    pub layering_summary: Option<String>,
    #[serde(default)]
    pub layering_rules: Vec<String>,
    #[serde(default)]
    pub layering_ambiguities: Vec<String>,
    #[serde(default)]
    pub api_contracts: Vec<String>,
    #[serde(default)]
    pub user_journeys: Vec<String>,
    #[serde(default)]
    pub e2e_test_cases: Vec<String>,
    #[serde(default)]
    pub auth_surfaces: Vec<String>,
    #[serde(default)]
    pub coding_conventions: Vec<String>,
    #[serde(default)]
    pub build_commands: Vec<String>,
    #[serde(default)]
    pub test_commands: Vec<String>,
    #[serde(default)]
    pub dev_commands: Vec<String>,
    #[serde(default)]
    pub risks: Vec<String>,
    #[serde(default)]
    pub project_intent: Vec<String>,
    #[serde(default)]
    pub environment_requirements: Vec<String>,
    #[serde(default)]
    pub frozen_paths: Vec<String>,
    #[serde(default)]
    pub high_risk_paths: Vec<String>,
    #[serde(default)]
    pub inference_count: usize,
    #[serde(default)]
    pub strongest_inferences: Vec<String>,
    #[serde(default)]
    pub weakest_inferences: Vec<String>,
    #[serde(default)]
    pub average_inference_confidence: Option<f32>,
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

    pub fn evidence_path(&self) -> PathBuf {
        self.root.join("evidence.json")
    }

    pub fn profile_path(&self) -> PathBuf {
        self.root.join("profile.json")
    }

    pub fn inference_path(&self) -> PathBuf {
        self.root.join("inference.json")
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

    pub fn load_inference(&self) -> Result<Option<WorkspaceDiscoveryInference>> {
        read_json_if_exists(&self.inference_path())
    }

    pub fn load_evidence(&self) -> Result<Option<WorkspaceDiscoveryEvidence>> {
        if self.evidence_path().exists() {
            return read_json_if_exists(&self.evidence_path());
        }
        read_json_if_exists(&self.scan_path())
    }

    pub fn load_scan(&self) -> Result<Option<WorkspaceDiscoveryScan>> {
        self.load_evidence()
    }

    pub fn load_status(&self) -> Result<Option<WorkspaceDiscoveryStatus>> {
        let Some(mut status) =
            read_json_if_exists::<WorkspaceDiscoveryStatus>(&self.status_path())?
        else {
            return Ok(None);
        };
        if status.scan_path.as_os_str().is_empty() {
            status.scan_path = self.scan_path();
        }
        if status.evidence_path.as_os_str().is_empty() {
            status.evidence_path = self.evidence_path();
        }
        if status.profile_path.as_os_str().is_empty() {
            status.profile_path = self.profile_path();
        }
        if status.inference_path.as_os_str().is_empty() {
            status.inference_path = self.inference_path();
        }
        self.reconcile_orphaned_status(status).map(Some)
    }

    pub fn load_payload(&self) -> Result<Option<WorkspaceDiscoveryPayload>> {
        let Some(status) = self.load_status()? else {
            return Ok(None);
        };
        let scan = self.load_evidence()?;
        let profile = self.load_profile()?;
        let inference = self.load_inference()?;
        let profile_summary = profile.as_ref().map(|profile| profile.summary.clone());
        let inference_summary = inference.as_ref().map(|item| item.summary.clone());
        Ok(Some(WorkspaceDiscoveryPayload {
            status,
            profile_summary,
            inference_summary,
            overview: WorkspaceDiscoveryOverview::from_sources(
                scan.as_ref(),
                inference.as_ref(),
                profile.as_ref(),
            ),
        }))
    }

    pub fn save_evidence(&self, evidence: &WorkspaceDiscoveryEvidence) -> Result<()> {
        self.ensure_dirs()?;
        write_json_pretty(&self.evidence_path(), evidence)?;
        write_json_pretty(&self.scan_path(), evidence)
    }

    pub fn save_scan(&self, scan: &WorkspaceDiscoveryScan) -> Result<()> {
        self.save_evidence(scan)
    }

    pub fn save_profile(&self, profile: &WorkspaceProfile) -> Result<()> {
        self.ensure_dirs()?;
        write_json_pretty(&self.profile_path(), profile)
    }

    pub fn save_inference(&self, inference: &WorkspaceDiscoveryInference) -> Result<()> {
        self.ensure_dirs()?;
        write_json_pretty(&self.inference_path(), inference)
    }

    pub fn save_status(&self, status: &WorkspaceDiscoveryStatus) -> Result<()> {
        self.ensure_dirs()?;
        write_json_pretty(&self.status_path(), status)
    }

    pub fn reset_worker_artifacts(&self) -> Result<()> {
        self.ensure_dirs()?;
        let worker = self.worker_artifacts();
        for path in [
            &worker.prompt_file,
            &worker.output_file,
            &worker.stdout_log,
            &worker.stderr_log,
            &worker.result_file,
        ] {
            remove_file_if_exists(path)?;
        }
        Ok(())
    }

    fn reconcile_orphaned_status(
        &self,
        mut status: WorkspaceDiscoveryStatus,
    ) -> Result<WorkspaceDiscoveryStatus> {
        if !status.current_phase.is_in_progress() {
            return Ok(status);
        }

        let is_stale = status.phase_heartbeat_at.map_or(true, |heartbeat| {
            Utc::now().signed_duration_since(heartbeat)
                > Duration::seconds(ORPHANED_DISCOVERY_PHASE_TIMEOUT_SECS)
        });
        if !is_stale {
            return Ok(status);
        }

        let error_message = format!(
            "previous discovery refresh ended before completion during {}",
            status.current_phase.as_str()
        );
        if let Some(profile) = self.load_profile()? {
            status.profile_fingerprint = Some(profile_fingerprint(&profile)?);
            status.last_refreshed_at.get_or_insert(profile.generated_at);
            status.last_refresh_error = Some(error_message);
            status.used_fallback_profile = true;
            status.current_phase = WorkspaceDiscoveryPhase::UsingFallbackProfile;
        } else {
            status.profile_fingerprint = None;
            status.last_refresh_error = Some(error_message);
            status.used_fallback_profile = false;
            status.current_phase = WorkspaceDiscoveryPhase::Failed;
        }
        status.phase_heartbeat_at = Some(Utc::now());
        self.save_status(&status)?;
        Ok(status)
    }
}

const ORPHANED_DISCOVERY_PHASE_TIMEOUT_SECS: i64 = 20;

impl WorkspaceDiscoveryRequest {
    pub fn synthesize_profile(&self) -> WorkspaceProfile {
        synthesize_profile_from_evidence(&self.scan)
    }

    pub fn synthesize_inference(&self) -> WorkspaceDiscoveryInference {
        let scan = &self.scan;
        let mut inferences = Vec::new();

        if !scan.tech_stack.is_empty() {
            inferences.push(DiscoveryInference {
                id: "inference.system-summary".to_string(),
                category: "system_summary".to_string(),
                statement: format!(
                    "Primary stack centers on {}.",
                    scan.tech_stack
                        .iter()
                        .take(3)
                        .map(|fact| fact.title.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                confidence: 9,
                rationale:
                    "The workspace manifests and dependency files consistently indicate the primary technologies."
                        .to_string(),
                evidence_chains: vec![strong_chain(
                    "stack manifests",
                    preferred_evidence_ids(
                        scan,
                        scan.tech_stack
                            .iter()
                            .take(3)
                            .map(|fact| fact.id.clone())
                            .collect(),
                        3,
                    ),
                )],
                assumptions: Vec::new(),
                contradictions: Vec::new(),
            });
        }

        for (index, rule) in scan
            .layering
            .allowed_dependency_directions
            .iter()
            .take(4)
            .enumerate()
        {
            inferences.push(DiscoveryInference {
                id: format!("inference.layering-rule.{:02}", index + 1),
                category: "layering_rule".to_string(),
                statement: rule.clone(),
                confidence: 10,
                rationale:
                    "The dependency direction is directly supported by one coherent evidence chain."
                        .to_string(),
                evidence_chains: vec![strong_chain(
                    "dependency direction chain",
                    preferred_evidence_ids(
                        scan,
                        scan.dependency_relationships
                            .iter()
                            .take(3)
                            .map(|relationship| relationship.id.clone())
                            .collect(),
                        3,
                    ),
                )],
                assumptions: Vec::new(),
                contradictions: Vec::new(),
            });
        }

        for (index, ambiguity) in scan
            .layering
            .unresolved_ambiguities
            .iter()
            .take(4)
            .enumerate()
        {
            inferences.push(DiscoveryInference {
                id: format!("inference.layering-ambiguity.{:02}", index + 1),
                category: "layering_ambiguity".to_string(),
                statement: ambiguity.clone(),
                confidence: 4,
                rationale:
                    "The deterministic scan found signals, but the architecture boundary remains ambiguous."
                        .to_string(),
                evidence_chains: vec![DiscoveryEvidenceChain {
                    label: "layer ambiguity chain".to_string(),
                    strength: DiscoveryEvidenceChainStrength::Moderate,
                    evidence_ids: preferred_evidence_ids(
                        scan,
                        scan.dependency_relationships
                            .iter()
                            .take(2)
                            .map(|relationship| relationship.id.clone())
                            .collect(),
                        2,
                    ),
                }],
                assumptions: Vec::new(),
                contradictions: Vec::new(),
            });
        }

        if !scan.commands.test.is_empty() {
            inferences.push(DiscoveryInference {
                id: "inference.test-command".to_string(),
                category: "key_concept".to_string(),
                statement: format!(
                    "Primary test commands include {}.",
                    scan.commands
                        .test
                        .iter()
                        .take(3)
                        .map(|command| command.command.join(" "))
                        .collect::<Vec<_>>()
                        .join("; ")
                ),
                confidence: 10,
                rationale:
                    "Detected test commands come directly from build metadata and explicit scripts."
                        .to_string(),
                evidence_chains: vec![strong_chain(
                    "test command manifests",
                    preferred_evidence_ids(
                        scan,
                        scan.commands
                            .test
                            .iter()
                            .take(3)
                            .map(|command| command.id.clone())
                            .collect(),
                        3,
                    ),
                )],
                assumptions: Vec::new(),
                contradictions: Vec::new(),
            });
        }

        if !scan.auth.is_empty() {
            inferences.push(DiscoveryInference {
                id: "inference.auth-surface".to_string(),
                category: "key_concept".to_string(),
                statement: "Authentication-related surfaces exist and should be treated as change-sensitive."
                    .to_string(),
                confidence: 8,
                rationale:
                    "Auth signals are present in the deterministic scan, but the exact enforcement flow may still need manual confirmation."
                        .to_string(),
                evidence_chains: vec![DiscoveryEvidenceChain {
                    label: "auth sources".to_string(),
                    strength: DiscoveryEvidenceChainStrength::Strong,
                    evidence_ids: preferred_evidence_ids(
                        scan,
                        scan.auth
                            .iter()
                            .take(3)
                            .map(|fact| fact.id.clone())
                            .collect(),
                        3,
                    ),
                }],
                assumptions: Vec::new(),
                contradictions: Vec::new(),
            });
        }

        WorkspaceDiscoveryInference {
            workspace_path: scan.workspace_path.clone(),
            generated_at: Utc::now(),
            summary: synthesize_profile_from_evidence(scan).summary,
            inferences,
            risks: scan
                .scan_notes
                .iter()
                .chain(scan.layering.unresolved_ambiguities.iter())
                .take(8)
                .cloned()
                .collect(),
        }
    }
}

impl WorkspaceDiscoveryInference {
    pub fn validate(&self, evidence: &WorkspaceDiscoveryEvidence) -> Result<()> {
        let valid_ids = evidence.collect_evidence_ids();
        for inference in &self.inferences {
            if !(1..=10).contains(&inference.confidence) {
                anyhow::bail!(
                    "discovery inference {} has invalid confidence {}",
                    inference.id,
                    inference.confidence
                );
            }
            if inference.statement.trim().is_empty() {
                anyhow::bail!(
                    "discovery inference {} has an empty statement",
                    inference.id
                );
            }
            if inference.confidence == 10 {
                let has_strong_chain = inference
                    .evidence_chains
                    .iter()
                    .any(|chain| chain.strength == DiscoveryEvidenceChainStrength::Strong);
                if !has_strong_chain {
                    anyhow::bail!(
                        "discovery inference {} used confidence 10 without any strong evidence chain",
                        inference.id
                    );
                }
            }
            for chain in &inference.evidence_chains {
                if chain.evidence_ids.is_empty() {
                    anyhow::bail!(
                        "discovery inference {} contains an empty evidence chain",
                        inference.id
                    );
                }
                for evidence_id in &chain.evidence_ids {
                    if !valid_ids.contains(evidence_id) {
                        anyhow::bail!(
                            "discovery inference {} references unknown evidence id {}",
                            inference.id,
                            evidence_id
                        );
                    }
                }
            }
        }
        Ok(())
    }

    pub fn assemble_profile(&self, evidence: &WorkspaceDiscoveryEvidence) -> WorkspaceProfile {
        let mut profile = synthesize_profile_from_evidence(evidence);

        if !self.summary.trim().is_empty() {
            profile.summary = self.summary.clone();
        }

        let mut key_concepts = self
            .inferences
            .iter()
            .filter(|inference| {
                matches!(
                    inference.category.as_str(),
                    "system_summary" | "key_concept" | "api_contract" | "auth" | "topology"
                )
            })
            .collect::<Vec<_>>();
        key_concepts.sort_by(|left, right| {
            right
                .confidence
                .cmp(&left.confidence)
                .then_with(|| left.id.cmp(&right.id))
        });
        let key_concepts = key_concepts
            .into_iter()
            .take(8)
            .map(|item| item.statement.clone())
            .collect::<Vec<_>>();
        if !key_concepts.is_empty() {
            profile.key_concepts = key_concepts;
        }

        let mut layering = evidence.layering.clone();
        let inferred_rules = self
            .inferences
            .iter()
            .filter(|inference| inference.category == "layering_rule")
            .map(|item| item.statement.clone())
            .collect::<Vec<_>>();
        if !inferred_rules.is_empty() {
            layering.allowed_dependency_directions = inferred_rules;
        }
        let inferred_ambiguities = self
            .inferences
            .iter()
            .filter(|inference| inference.category == "layering_ambiguity")
            .map(|item| item.statement.clone())
            .collect::<Vec<_>>();
        if !inferred_ambiguities.is_empty() {
            layering.unresolved_ambiguities = inferred_ambiguities;
        }
        if let Some(summary) = self
            .inferences
            .iter()
            .find(|inference| inference.category == "architecture_summary")
            .map(|item| item.statement.clone())
        {
            layering.summary = summary;
        }
        profile.layering = layering;

        let mut risks = evidence.scan_notes.clone();
        risks.extend(self.risks.clone());
        risks.extend(
            self.inferences
                .iter()
                .filter(|inference| {
                    inference.confidence <= 4 || !inference.contradictions.is_empty()
                })
                .map(|item| item.statement.clone()),
        );
        if !risks.is_empty() {
            risks.sort();
            risks.dedup();
            profile.risks = risks;
        }

        profile.generated_at = self.generated_at;
        profile
    }

    pub fn from_legacy_profile(
        profile: &WorkspaceProfile,
        evidence: &WorkspaceDiscoveryEvidence,
    ) -> Self {
        let fallback_chain = strong_chain(
            "legacy compatibility evidence",
            evidence.default_supporting_evidence_ids(3),
        );
        let mut inferences = profile
            .key_concepts
            .iter()
            .enumerate()
            .map(|(index, statement)| DiscoveryInference {
                id: format!("legacy.key-concept.{:02}", index + 1),
                category: "key_concept".to_string(),
                statement: statement.clone(),
                confidence: 6,
                rationale: "Derived from a legacy discovery profile for backward compatibility."
                    .to_string(),
                evidence_chains: vec![fallback_chain.clone()],
                assumptions: vec!["Migrated from legacy profile output.".to_string()],
                contradictions: Vec::new(),
            })
            .collect::<Vec<_>>();
        inferences.extend(
            profile
                .layering
                .allowed_dependency_directions
                .iter()
                .enumerate()
                .map(|(index, statement)| DiscoveryInference {
                    id: format!("legacy.layering-rule.{:02}", index + 1),
                    category: "layering_rule".to_string(),
                    statement: statement.clone(),
                    confidence: 6,
                    rationale:
                        "Derived from a legacy discovery profile for backward compatibility."
                            .to_string(),
                    evidence_chains: vec![fallback_chain.clone()],
                    assumptions: vec!["Migrated from legacy profile output.".to_string()],
                    contradictions: Vec::new(),
                }),
        );

        Self {
            workspace_path: profile.workspace_path.clone(),
            generated_at: profile.generated_at,
            summary: profile.summary.clone(),
            inferences,
            risks: profile.risks.clone(),
        }
    }
}

impl WorkspaceDiscoveryPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Scanning => "scanning",
            Self::ReusingCachedProfile => "reusing_cached_profile",
            Self::Polishing => "polishing",
            Self::UsingFallbackProfile => "using_fallback_profile",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }

    fn is_in_progress(self) -> bool {
        matches!(
            self,
            Self::Scanning | Self::ReusingCachedProfile | Self::Polishing
        )
    }
}

impl WorkspaceDiscoveryOverview {
    pub fn from_sources(
        scan: Option<&WorkspaceDiscoveryScan>,
        inference: Option<&WorkspaceDiscoveryInference>,
        profile: Option<&WorkspaceProfile>,
    ) -> Self {
        let layering = profile
            .map(|item| &item.layering)
            .or_else(|| scan.map(|item| &item.layering));
        let commands = profile
            .map(|item| &item.commands)
            .or_else(|| scan.map(|item| &item.commands));
        let tech_stack = profile.map_or_else(
            || scan.map_or_else(Vec::new, |item| fact_titles(&item.tech_stack)),
            |item| fact_titles(&item.tech_stack),
        );
        let api_contracts = profile.map_or_else(
            || scan.map_or_else(Vec::new, |item| fact_titles(&item.api_contracts)),
            |item| fact_titles(&item.api_contracts),
        );
        let user_journeys = profile.map_or_else(
            || scan.map_or_else(Vec::new, |item| fact_titles(&item.user_journeys)),
            |item| fact_titles(&item.user_journeys),
        );
        let e2e_test_cases = profile.map_or_else(
            || scan.map_or_else(Vec::new, |item| fact_titles(&item.e2e_test_cases)),
            |item| fact_titles(&item.e2e_test_cases),
        );
        let auth_surfaces = profile.map_or_else(
            || scan.map_or_else(Vec::new, |item| fact_titles(&item.auth)),
            |item| fact_titles(&item.auth),
        );
        let coding_conventions = profile.map_or_else(
            || scan.map_or_else(Vec::new, |item| fact_titles(&item.coding_conventions)),
            |item| fact_titles(&item.coding_conventions),
        );
        let build_commands = commands.map_or_else(Vec::new, |item| command_lines(&item.build));
        let test_commands = commands.map_or_else(Vec::new, |item| command_lines(&item.test));
        let dev_commands = commands.map_or_else(Vec::new, |item| command_lines(&item.dev));
        let api_contract_count = api_contracts.len();
        let user_journey_count = user_journeys.len();
        let e2e_test_case_count = e2e_test_cases.len();
        let auth_surface_count = auth_surfaces.len();
        let coding_convention_count = coding_conventions.len();
        let build_command_count = build_commands.len();
        let test_command_count = test_commands.len();
        let dev_command_count = dev_commands.len();

        Self {
            source_file_count: scan.map_or(0, |item| item.source_files.len()),
            repository_count: profile.map_or_else(
                || scan.map_or(0, |item| item.repositories.len()),
                |item| item.repositories.len(),
            ),
            dependency_relationship_count: profile.map_or_else(
                || scan.map_or(0, |item| item.dependency_relationships.len()),
                |item| item.dependency_relationships.len(),
            ),
            layer_count: layering.map_or(0, |item| item.layers.len()),
            api_contract_count,
            user_journey_count,
            e2e_test_case_count,
            auth_surface_count,
            coding_convention_count,
            build_command_count,
            test_command_count,
            dev_command_count,
            tech_stack,
            key_concepts: profile.map_or_else(Vec::new, |item| item.key_concepts.clone()),
            repositories: profile.map_or_else(
                || scan.map_or_else(Vec::new, |item| repository_summaries(&item.repositories)),
                |item| repository_summaries(&item.repositories),
            ),
            layering_summary: layering.map(|item| item.summary.clone()),
            layering_rules: layering
                .map_or_else(Vec::new, |item| item.allowed_dependency_directions.clone()),
            layering_ambiguities: layering
                .map_or_else(Vec::new, |item| item.unresolved_ambiguities.clone()),
            api_contracts,
            user_journeys,
            e2e_test_cases,
            auth_surfaces,
            coding_conventions,
            build_commands,
            test_commands,
            dev_commands,
            risks: profile.map_or_else(Vec::new, |item| item.risks.clone()),
            project_intent: profile.map_or_else(
                || scan.map_or_else(Vec::new, |item| fact_titles(&item.project_intent)),
                |item| fact_titles(&item.project_intent),
            ),
            environment_requirements: profile.map_or_else(
                || scan.map_or_else(Vec::new, |item| fact_titles(&item.environment_requirements)),
                |item| fact_titles(&item.environment_requirements),
            ),
            frozen_paths: profile.map_or_else(
                || {
                    scan.map_or_else(Vec::new, |item| {
                        item.change_boundaries
                            .frozen_paths
                            .iter()
                            .map(|p| p.display().to_string())
                            .collect()
                    })
                },
                |item| {
                    item.change_boundaries
                        .frozen_paths
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect()
                },
            ),
            high_risk_paths: profile.map_or_else(
                || {
                    scan.map_or_else(Vec::new, |item| {
                        item.change_boundaries
                            .high_risk_paths
                            .iter()
                            .map(|p| p.display().to_string())
                            .collect()
                    })
                },
                |item| {
                    item.change_boundaries
                        .high_risk_paths
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect()
                },
            ),
            inference_count: inference.map_or(0, |item| item.inferences.len()),
            strongest_inferences: inference.map_or_else(Vec::new, |item| {
                let mut entries = item.inferences.iter().collect::<Vec<_>>();
                entries.sort_by(|left, right| {
                    right
                        .confidence
                        .cmp(&left.confidence)
                        .then_with(|| left.id.cmp(&right.id))
                });
                entries
                    .into_iter()
                    .take(6)
                    .map(|entry| format!("[{}/10] {}", entry.confidence, entry.statement))
                    .collect()
            }),
            weakest_inferences: inference.map_or_else(Vec::new, |item| {
                let mut entries = item.inferences.iter().collect::<Vec<_>>();
                entries.sort_by(|left, right| {
                    left.confidence
                        .cmp(&right.confidence)
                        .then_with(|| left.id.cmp(&right.id))
                });
                entries
                    .into_iter()
                    .take(6)
                    .map(|entry| format!("[{}/10] {}", entry.confidence, entry.statement))
                    .collect()
            }),
            average_inference_confidence: inference.and_then(|item| {
                if item.inferences.is_empty() {
                    return None;
                }
                let total: usize = item
                    .inferences
                    .iter()
                    .map(|entry| usize::from(entry.confidence))
                    .sum();
                Some(total as f32 / item.inferences.len() as f32)
            }),
        }
    }
}

fn strong_chain(label: impl Into<String>, evidence_ids: Vec<String>) -> DiscoveryEvidenceChain {
    DiscoveryEvidenceChain {
        label: label.into(),
        strength: DiscoveryEvidenceChainStrength::Strong,
        evidence_ids,
    }
}

fn preferred_evidence_ids(
    evidence: &WorkspaceDiscoveryEvidence,
    preferred: Vec<String>,
    fallback_limit: usize,
) -> Vec<String> {
    if preferred.is_empty() {
        return evidence.default_supporting_evidence_ids(fallback_limit.max(1));
    }
    preferred
}

fn synthesize_profile_from_evidence(scan: &WorkspaceDiscoveryEvidence) -> WorkspaceProfile {
    let tech_stack = consolidate_facts(&scan.tech_stack);
    let api_contracts = consolidate_facts(&scan.api_contracts);
    let coding_conventions = tier_sorted_clone(&scan.coding_conventions);
    let user_journeys = tier_sorted_clone(&scan.user_journeys);
    let e2e_test_cases = tier_sorted_clone(&scan.e2e_test_cases);
    let auth = tier_sorted_clone(&scan.auth);

    let primary_stack = fact_titles(&tech_stack)
        .into_iter()
        .take(4)
        .collect::<Vec<_>>();
    let repo_count = scan.repositories.len().max(1);
    let mut key_concepts = Vec::new();

    for intent in scan.project_intent.iter().take(2) {
        if !intent.summary.is_empty() {
            let preview = if intent.summary.len() > 200 {
                format!("{}…", &intent.summary[..200])
            } else {
                intent.summary.clone()
            };
            key_concepts.push(format!("Project: {}.", preview));
        }
    }

    if !primary_stack.is_empty() {
        key_concepts.push(format!("Primary stack: {}.", primary_stack.join(", ")));
    }

    for convention in coding_conventions
        .iter()
        .filter(|f| f.tier == NegentropyTier::Specification)
        .take(3)
    {
        key_concepts.push(format!(
            "Convention: {} (from {}).",
            convention.title,
            convention
                .evidence
                .first()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
        ));
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
        let primary_test_commands = command_lines(&scan.commands.test)
            .into_iter()
            .take(3)
            .collect::<Vec<_>>();
        key_concepts.push(format!(
            "Primary test commands: {}.",
            primary_test_commands.join("; ")
        ));
    }

    for journey in user_journeys.iter().take(2) {
        if !journey.summary.is_empty() {
            key_concepts.push(journey.summary.clone());
        }
    }

    if !auth.is_empty() {
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
        tech_stack,
        repositories: scan.repositories.clone(),
        dependency_relationships: scan.dependency_relationships.clone(),
        api_contracts,
        layering: scan.layering.clone(),
        user_journeys,
        e2e_test_cases,
        auth,
        coding_conventions,
        commands: scan.commands.clone(),
        risks,
        project_intent: tier_sorted_clone(&scan.project_intent),
        environment_requirements: tier_sorted_clone(&scan.environment_requirements),
        change_boundaries: scan.change_boundaries.clone(),
    }
}

fn consolidate_facts(facts: &[DiscoveryFact]) -> Vec<DiscoveryFact> {
    let mut by_title: BTreeMap<String, DiscoveryFact> = BTreeMap::new();
    for fact in facts {
        let entry = by_title
            .entry(fact.title.clone())
            .or_insert_with(|| DiscoveryFact {
                id: fact.id.clone(),
                title: fact.title.clone(),
                summary: String::new(),
                evidence: Vec::new(),
                tier: fact.tier,
            });
        if entry.tier > fact.tier {
            entry.tier = fact.tier;
        }
        if entry.summary.is_empty() {
            entry.summary = fact.summary.clone();
        } else if entry.summary != fact.summary && entry.summary.len() < 512 {
            entry.summary = format!("{} | {}", entry.summary, fact.summary);
        }
        for path in &fact.evidence {
            if !entry.evidence.contains(path) && entry.evidence.len() < 6 {
                entry.evidence.push(path.clone());
            }
        }
    }
    let mut result: Vec<DiscoveryFact> = by_title.into_values().collect();
    result.sort_by(|a, b| a.tier.cmp(&b.tier).then_with(|| a.title.cmp(&b.title)));
    result
}

fn tier_sorted_clone(facts: &[DiscoveryFact]) -> Vec<DiscoveryFact> {
    let mut result = facts.to_vec();
    result.sort_by(|a, b| a.tier.cmp(&b.tier).then_with(|| a.title.cmp(&b.title)));
    result
}

fn fact_titles(items: &[DiscoveryFact]) -> Vec<String> {
    dedupe_display_strings(items.iter().map(|item| item.title.clone()))
}

fn repository_summaries(items: &[RepositoryProfile]) -> Vec<String> {
    items
        .iter()
        .map(|item| format!("{} ({})", item.name, item.root.display()))
        .collect()
}

fn command_lines(items: &[DetectedCommand]) -> Vec<String> {
    dedupe_display_strings(items.iter().map(|item| item.command.join(" ")))
}

fn dedupe_display_strings(items: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();
    for item in items {
        if seen.insert(item.clone()) {
            deduped.push(item);
        }
    }
    deduped
}

impl WorkspaceDiscoveryScan {
    pub fn collect_evidence_ids(&self) -> BTreeSet<String> {
        let mut ids = BTreeSet::new();
        ids.extend(
            self.source_files
                .iter()
                .map(|item| item.id.clone())
                .filter(|id| !id.is_empty()),
        );
        ids.extend(
            self.tech_stack
                .iter()
                .map(|item| item.id.clone())
                .filter(|id| !id.is_empty()),
        );
        ids.extend(
            self.repositories
                .iter()
                .map(|item| item.id.clone())
                .filter(|id| !id.is_empty()),
        );
        ids.extend(
            self.dependency_relationships
                .iter()
                .map(|item| item.id.clone())
                .filter(|id| !id.is_empty()),
        );
        ids.extend(
            self.api_contracts
                .iter()
                .map(|item| item.id.clone())
                .filter(|id| !id.is_empty()),
        );
        ids.extend(
            self.user_journeys
                .iter()
                .map(|item| item.id.clone())
                .filter(|id| !id.is_empty()),
        );
        ids.extend(
            self.e2e_test_cases
                .iter()
                .map(|item| item.id.clone())
                .filter(|id| !id.is_empty()),
        );
        ids.extend(
            self.auth
                .iter()
                .map(|item| item.id.clone())
                .filter(|id| !id.is_empty()),
        );
        ids.extend(
            self.coding_conventions
                .iter()
                .map(|item| item.id.clone())
                .filter(|id| !id.is_empty()),
        );
        ids.extend(
            self.commands
                .build
                .iter()
                .chain(self.commands.test.iter())
                .chain(self.commands.dev.iter())
                .map(|item| item.id.clone())
                .filter(|id| !id.is_empty()),
        );
        ids.extend(
            self.project_intent
                .iter()
                .map(|item| item.id.clone())
                .filter(|id| !id.is_empty()),
        );
        ids.extend(
            self.environment_requirements
                .iter()
                .map(|item| item.id.clone())
                .filter(|id| !id.is_empty()),
        );
        ids
    }

    fn default_supporting_evidence_ids(&self, limit: usize) -> Vec<String> {
        self.collect_evidence_ids()
            .into_iter()
            .take(limit)
            .collect()
    }
}

impl WorkspaceProfile {
    pub fn prompt_context(&self) -> String {
        let mut lines = vec![format!("- summary: {}", self.summary)];

        if !self.project_intent.is_empty() {
            lines.push("- project_intent:".to_string());
            for fact in self.project_intent.iter().take(3) {
                let preview = if fact.summary.len() > 400 {
                    format!("{}…", &fact.summary[..400])
                } else {
                    fact.summary.clone()
                };
                lines.push(format!("  - {}: {}", fact.title, preview));
            }
        }

        if !self.key_concepts.is_empty() {
            lines.push(format!(
                "- key_concepts: {}",
                self.key_concepts
                    .iter()
                    .take(8)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" | ")
            ));
        }

        let spec_conventions: Vec<_> = self
            .coding_conventions
            .iter()
            .filter(|f| f.tier == NegentropyTier::Specification)
            .collect();
        if !spec_conventions.is_empty() {
            lines.push("- specifications:".to_string());
            for fact in spec_conventions.iter().take(4) {
                let preview = if fact.summary.len() > 300 {
                    format!("{}…", &fact.summary[..300])
                } else {
                    fact.summary.clone()
                };
                lines.push(format!("  - {}: {}", fact.title, preview));
            }
        }

        let spec_contracts: Vec<_> = self
            .api_contracts
            .iter()
            .filter(|f| f.tier == NegentropyTier::Specification)
            .collect();
        if !spec_contracts.is_empty() {
            lines.push(format!(
                "- explicit_api_contracts: {}",
                spec_contracts
                    .iter()
                    .take(4)
                    .map(|fact| fact.title.clone())
                    .collect::<Vec<_>>()
                    .join(" | ")
            ));
        }

        if !self.user_journeys.is_empty() || !self.e2e_test_cases.is_empty() {
            let mut verification_items = Vec::new();
            for fact in self.user_journeys.iter().take(4) {
                verification_items.push(fact.summary.clone());
            }
            for fact in self.e2e_test_cases.iter().take(4) {
                verification_items.push(fact.summary.clone());
            }
            if !verification_items.is_empty() {
                lines.push(format!(
                    "- verification_baselines: {}",
                    verification_items
                        .into_iter()
                        .take(6)
                        .collect::<Vec<_>>()
                        .join(" | ")
                ));
            }
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

        if !self.environment_requirements.is_empty() {
            lines.push(format!(
                "- environment_requirements: {}",
                self.environment_requirements
                    .iter()
                    .take(4)
                    .map(|fact| format!("{}: {}", fact.title, fact.summary))
                    .collect::<Vec<_>>()
                    .join(" | ")
            ));
        }

        if !self.change_boundaries.frozen_paths.is_empty() {
            lines.push(format!(
                "- frozen_files (do not modify): {}",
                self.change_boundaries
                    .frozen_paths
                    .iter()
                    .take(6)
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(" | ")
            ));
        }
        if !self.change_boundaries.high_risk_paths.is_empty() {
            lines.push(format!(
                "- high_risk_files (modify with caution): {}",
                self.change_boundaries
                    .high_risk_paths
                    .iter()
                    .take(6)
                    .map(|p| p.display().to_string())
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

        if !self.change_boundaries.frozen_paths.is_empty() {
            let paths = self
                .change_boundaries
                .frozen_paths
                .iter()
                .take(4)
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            notes.push(format!("Do not modify frozen/generated files: {paths}."));
        }

        if !self.change_boundaries.high_risk_paths.is_empty() {
            let paths = self
                .change_boundaries
                .high_risk_paths
                .iter()
                .take(4)
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            notes.push(format!(
                "Exercise caution when modifying high-risk files: {paths}."
            ));
        }

        notes
    }
}

pub fn scan_workspace(workspace_path: &Path) -> Result<WorkspaceDiscoveryScan> {
    let workspace_path = normalize_path(workspace_path.to_path_buf());
    let scanned_at = Utc::now();
    let gitignore_matcher = GitignoreMatcher::load(&workspace_path)?;
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
    let mut project_intent = Vec::new();
    let mut environment_requirements = Vec::new();
    let mut frozen_paths = Vec::new();
    let mut high_risk_paths = Vec::new();

    for entry in WalkDir::new(&workspace_path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            let rel = relative_to_workspace(&workspace_path, entry.path());
            !should_skip_entry(entry)
                && !should_ignore_source_path(&rel, entry.file_type().is_dir())
                && !gitignore_matcher.is_ignored(&rel, entry.file_type().is_dir())
        })
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

        let rel = relative_to_workspace(&workspace_path, path);
        if should_ignore_source_path(&rel, false) || !is_candidate_file(path) {
            continue;
        }

        let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
        let content_hash = hash_bytes(&bytes);
        source_files.push(DiscoverySourceFile {
            id: String::new(),
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
        detect_project_intent(&rel, &text, &mut project_intent);
        detect_environment_requirements(&rel, &text, &mut environment_requirements);
        detect_change_boundaries(&rel, &mut frozen_paths, &mut high_risk_paths);
    }

    if repo_roots.is_empty() {
        repositories.push(RepositoryProfile {
            id: String::new(),
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
                id: String::new(),
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
    assign_source_file_ids(&mut source_files);
    assign_fact_ids("tech_stack", &mut tech_stack);
    assign_repository_ids(&mut repositories);
    assign_relationship_ids(&mut dependency_relationships);
    assign_fact_ids("api_contract", &mut api_contracts);
    assign_fact_ids("user_journey", &mut user_journeys);
    assign_fact_ids("e2e_test_case", &mut e2e_test_cases);
    assign_fact_ids("auth", &mut auth);
    assign_fact_ids("coding_convention", &mut coding_conventions);
    assign_command_ids("build_command", &mut commands.build);
    assign_command_ids("test_command", &mut commands.test);
    assign_command_ids("dev_command", &mut commands.dev);

    project_intent.sort_by(|left, right| left.title.cmp(&right.title));
    environment_requirements.sort_by(|left, right| left.title.cmp(&right.title));
    dedupe_facts(&mut project_intent);
    dedupe_facts(&mut environment_requirements);
    frozen_paths.sort();
    frozen_paths.dedup();
    high_risk_paths.sort();
    high_risk_paths.dedup();
    assign_fact_ids("project_intent", &mut project_intent);
    assign_fact_ids("env_requirement", &mut environment_requirements);

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
        project_intent,
        environment_requirements,
        change_boundaries: ChangeBoundaryProfile {
            frozen_paths,
            high_risk_paths,
        },
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

#[derive(Debug, Clone, Default)]
struct GitignoreMatcher {
    rule_sets: Vec<GitignoreRuleSet>,
}

#[derive(Debug, Clone)]
struct GitignoreRuleSet {
    base: PathBuf,
    rules: Vec<GitignoreRule>,
}

#[derive(Debug, Clone)]
struct GitignoreRule {
    pattern: String,
    anchored: bool,
    directory_only: bool,
    negated: bool,
    has_slash: bool,
}

impl GitignoreMatcher {
    fn load(workspace_path: &Path) -> Result<Self> {
        let mut rule_sets = Vec::new();

        for entry in WalkDir::new(workspace_path)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| !should_skip_entry(entry))
        {
            let entry = entry.with_context(|| {
                format!(
                    "failed while loading gitignore rules from {}",
                    workspace_path.display()
                )
            })?;
            if entry.file_type().is_dir() {
                continue;
            }
            if entry.file_name().to_str() != Some(".gitignore") {
                continue;
            }

            let rel = relative_to_workspace(workspace_path, entry.path());
            let rules = parse_gitignore_rules(
                &fs::read_to_string(entry.path())
                    .with_context(|| format!("failed to read {}", entry.path().display()))?,
            );
            if rules.is_empty() {
                continue;
            }

            rule_sets.push(GitignoreRuleSet {
                base: rel.parent().map(Path::to_path_buf).unwrap_or_default(),
                rules,
            });
        }

        rule_sets.sort_by(|left, right| {
            path_depth(&left.base)
                .cmp(&path_depth(&right.base))
                .then_with(|| left.base.cmp(&right.base))
        });

        Ok(Self { rule_sets })
    }

    fn is_ignored(&self, rel: &Path, is_dir: bool) -> bool {
        if rel.as_os_str().is_empty() {
            return false;
        }

        let mut ignored = false;
        for rule_set in &self.rule_sets {
            let Some(rel_from_base) = strip_relative_prefix(rel, &rule_set.base) else {
                continue;
            };

            for rule in &rule_set.rules {
                if rule.matches(rel_from_base, is_dir) {
                    ignored = !rule.negated;
                }
            }
        }

        ignored
    }
}

impl GitignoreRule {
    fn matches(&self, rel: &Path, is_dir: bool) -> bool {
        if self.directory_only && !is_dir {
            return false;
        }

        let rel_text = normalize_relative_path(rel);
        if rel_text.is_empty() {
            return false;
        }

        if self.anchored || self.has_slash {
            wildcard_match(&self.pattern, &rel_text)
        } else {
            rel.components().any(|component| {
                wildcard_match(&self.pattern, &component.as_os_str().to_string_lossy())
            })
        }
    }
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
        || lower == "readme.md"
        || lower == "readme"
        || lower == "readme.rst"
        || lower == "readme.txt"
        || lower == "rust-toolchain"
        || lower == "rust-toolchain.toml"
        || lower == ".nvmrc"
        || lower == ".node-version"
        || lower == ".tool-versions"
        || lower == ".env.example"
        || lower == ".env.sample"
        || lower == ".env.template"
        || lower.starts_with("docker-compose")
        || lower.starts_with("compose.")
}

fn should_ignore_source_path(rel: &Path, is_dir: bool) -> bool {
    if rel
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .any(|component| {
            matches!(
                component.to_ascii_lowercase().as_str(),
                "dictionary" | "dictionaries"
            )
        })
    {
        return true;
    }

    let Some(name) = rel.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    if lower == ".gitignore" {
        return true;
    }
    if is_dir {
        return false;
    }

    let stem = rel
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.to_ascii_lowercase());
    let extension = rel
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase());

    matches!(stem.as_deref(), Some("dictionary" | "dictionaries"))
        || matches!(extension.as_deref(), Some("dic" | "dict" | "dictionary"))
}

fn parse_gitignore_rules(text: &str) -> Vec<GitignoreRule> {
    text.lines()
        .filter_map(parse_gitignore_rule)
        .collect::<Vec<_>>()
}

fn parse_gitignore_rule(line: &str) -> Option<GitignoreRule> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut raw = trimmed.to_string();
    if let Some(rest) = raw.strip_prefix("\\#") {
        raw = format!("#{rest}");
    } else if raw.starts_with('#') {
        return None;
    }

    let negated = if let Some(rest) = raw.strip_prefix("\\!") {
        raw = format!("!{rest}");
        false
    } else if let Some(rest) = raw.strip_prefix('!') {
        raw = rest.to_string();
        true
    } else {
        false
    };

    let directory_only = raw.ends_with('/');
    if directory_only {
        raw.pop();
    }

    let anchored = raw.starts_with('/');
    if anchored {
        raw.remove(0);
    }

    let pattern = raw.trim();
    if pattern.is_empty() {
        return None;
    }

    Some(GitignoreRule {
        has_slash: pattern.contains('/'),
        pattern: pattern.to_string(),
        anchored,
        directory_only,
        negated,
    })
}

fn strip_relative_prefix<'a>(path: &'a Path, prefix: &Path) -> Option<&'a Path> {
    if prefix.as_os_str().is_empty() {
        Some(path)
    } else {
        path.strip_prefix(prefix).ok()
    }
}

fn normalize_relative_path(rel: &Path) -> String {
    rel.components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn path_depth(path: &Path) -> usize {
    path.components().count()
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    let pattern = pattern.as_bytes();
    let text = text.as_bytes();
    let (mut pattern_index, mut text_index) = (0usize, 0usize);
    let mut star_index = None;
    let mut match_index = 0usize;

    while text_index < text.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == b'?' || pattern[pattern_index] == text[text_index])
        {
            pattern_index += 1;
            text_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            star_index = Some(pattern_index);
            match_index = text_index;
            pattern_index += 1;
        } else if let Some(star) = star_index {
            pattern_index = star + 1;
            match_index += 1;
            text_index = match_index;
        } else {
            return false;
        }
    }

    while pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
        pattern_index += 1;
    }

    pattern_index == pattern.len()
}

fn relative_to_workspace(workspace: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(workspace)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| path.to_path_buf())
}

fn extend_layer_candidates(layers: &mut BTreeMap<String, BTreeSet<PathBuf>>, rel: &Path) {
    let components: Vec<String> = rel
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .map(|s| s.to_ascii_lowercase())
        .collect();

    let layer = components
        .iter()
        .find_map(|component| match component.as_str() {
            "ui" | "web" | "frontend" => Some("ui"),
            "service" | "application" => Some("service"),
            "domain" | "core" => Some("core"),
            "worker" | "adapter" | "cli" => Some("adapter"),
            "infra" | "infrastructure" => Some("infra"),
            _ => None,
        });

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
        let summary = if text.len() <= 4096 {
            text.to_string()
        } else {
            summarize_lines(text, 20)
        };
        output.push(DiscoveryFact {
            id: String::new(),
            title: name.to_string(),
            summary,
            evidence: vec![rel.to_path_buf()],
            tier: NegentropyTier::Specification,
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
        let test_names = extract_test_function_names(rel, text);
        let summary = if !test_names.is_empty() {
            format!("User journey tests: {}.", test_names.join(", "))
        } else {
            summarize_lines_matching(text, &["journey", "user journey"], 3)
                .unwrap_or_else(|| summarize_lines(text, 3))
        };
        journeys.push(DiscoveryFact {
            id: String::new(),
            title: format!("User journey evidence in {}", rel.display()),
            summary,
            evidence: vec![rel.to_path_buf()],
            tier: NegentropyTier::Verification,
        });
    }

    if path_text.contains("e2e")
        || path_text.contains("playwright")
        || path_text.contains("cypress")
        || path_text.contains("live_codex")
        || path_text.contains("run_smoke")
        || text.contains("CODEX_LIVE_E2E")
    {
        let test_names = extract_test_function_names(rel, text);
        let summary = if !test_names.is_empty() {
            format!("E2E test cases: {}.", test_names.join(", "))
        } else {
            summarize_lines(text, 3)
        };
        e2e.push(DiscoveryFact {
            id: String::new(),
            title: format!("E2E evidence in {}", rel.display()),
            summary,
            evidence: vec![rel.to_path_buf()],
            tier: NegentropyTier::Verification,
        });
    }
}

fn extract_test_function_names(rel: &Path, text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let ext = rel.extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext {
        "rs" => {
            let mut in_test_attr = false;
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed == "#[test]" || trimmed.starts_with("#[tokio::test") {
                    in_test_attr = true;
                    continue;
                }
                if in_test_attr {
                    if let Some(rest) = trimmed.strip_prefix("fn ") {
                        if let Some(name) = rest.split('(').next() {
                            names.push(name.trim().to_string());
                        }
                    } else if let Some(rest) = trimmed.strip_prefix("async fn ") {
                        if let Some(name) = rest.split('(').next() {
                            names.push(name.trim().to_string());
                        }
                    }
                    in_test_attr = false;
                }
            }
        }
        "ts" | "tsx" | "js" | "jsx" | "mjs" => {
            for line in text.lines() {
                let trimmed = line.trim();
                for prefix in ["it(", "test(", "it.only(", "test.only("] {
                    if let Some(rest) = trimmed.strip_prefix(prefix) {
                        if let Some(label) = extract_js_string_literal(rest) {
                            names.push(label);
                            break;
                        }
                    }
                }
                if names.len() >= 12 {
                    break;
                }
            }
        }
        _ => {}
    }
    names.truncate(12);
    names
}

fn extract_js_string_literal(text: &str) -> Option<String> {
    let text = text.trim();
    let (quote, rest) = if let Some(rest) = text.strip_prefix('\'') {
        ('\'', rest)
    } else if let Some(rest) = text.strip_prefix('"') {
        ('"', rest)
    } else if let Some(rest) = text.strip_prefix('`') {
        ('`', rest)
    } else {
        return None;
    };
    rest.find(quote).map(|end| rest[..end].to_string())
}

fn detect_auth(rel: &Path, text: &str, output: &mut Vec<DiscoveryFact>) {
    let path_text = rel.to_string_lossy().to_ascii_lowercase();
    let strong_keywords = ["auth", "oauth", "jwt", "oidc", "openid"];
    let contextual_keywords = ["session", "token"];

    let strong_path_match = strong_keywords
        .iter()
        .any(|keyword| path_text.contains(keyword));
    let lower_text = text.to_ascii_lowercase();
    let strong_content_match = strong_keywords
        .iter()
        .any(|keyword| lower_text.contains(keyword));

    let contextual_match = if !strong_path_match && !strong_content_match {
        contextual_keywords.iter().any(|keyword| {
            let in_content = lower_text.contains(&format!("{}auth", keyword))
                || lower_text.contains(&format!("auth{}", keyword))
                || lower_text.contains(&format!("{}_id", keyword))
                || lower_text.contains(&format!("access_{}", keyword))
                || lower_text.contains(&format!("refresh_{}", keyword))
                || lower_text.contains(&format!("bearer_{}", keyword));
            in_content
        })
    } else {
        false
    };

    if strong_path_match || strong_content_match || contextual_match {
        let all_keywords: Vec<&str> = strong_keywords
            .iter()
            .chain(contextual_keywords.iter())
            .copied()
            .collect();
        output.push(DiscoveryFact {
            id: String::new(),
            title: format!("Auth evidence in {}", rel.display()),
            summary: summarize_lines_matching(text, &all_keywords, 3)
                .unwrap_or_else(|| summarize_lines(text, 3)),
            evidence: vec![rel.to_path_buf()],
            tier: NegentropyTier::Structure,
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
            id: String::new(),
            title: format!("Explicit API contract {}", rel.display()),
            summary: summarize_lines(text, 3),
            evidence: vec![rel.to_path_buf()],
            tier: NegentropyTier::Specification,
        });
        return;
    }

    if let Some(exports) = summarize_module_exports(rel, text) {
        output.push(DiscoveryFact {
            id: String::new(),
            title: format!("Exposed module interfaces in {}", rel.display()),
            summary: exports,
            evidence: vec![rel.to_path_buf()],
            tier: NegentropyTier::Implementation,
        });
    }

    if let Some(routes) = summarize_http_routes(text) {
        output.push(DiscoveryFact {
            id: String::new(),
            title: format!("HTTP route evidence in {}", rel.display()),
            summary: routes,
            evidence: vec![rel.to_path_buf()],
            tier: NegentropyTier::Implementation,
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
                id: String::new(),
                label: label.clone(),
                command: vec!["make".to_string(), label.clone()],
                source: rel.to_path_buf(),
            };
            classify_command(commands, detected);
        }
        tech_stack.push(DiscoveryFact {
            id: String::new(),
            title: "Make".to_string(),
            summary: format!("Detected Makefile targets in {}.", rel.display()),
            evidence: vec![rel.to_path_buf()],
            tier: NegentropyTier::Structure,
        });
    } else if lower == "package.json" {
        let package: serde_json::Value = serde_json::from_str(text)
            .with_context(|| format!("failed to parse {}", rel.display()))?;

        if let Some(package_name) = package.get("name").and_then(serde_json::Value::as_str) {
            tech_stack.push(DiscoveryFact {
                id: String::new(),
                title: "Node.js package".to_string(),
                summary: format!("Package `{package_name}` declared in {}.", rel.display()),
                evidence: vec![rel.to_path_buf()],
                tier: NegentropyTier::Structure,
            });
        }

        if package.get("dependencies").is_some() || package.get("devDependencies").is_some() {
            tech_stack.push(DiscoveryFact {
                id: String::new(),
                title: "JavaScript / TypeScript".to_string(),
                summary: extract_package_dependencies(&package),
                evidence: vec![rel.to_path_buf()],
                tier: NegentropyTier::Structure,
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
                    id: String::new(),
                    label: label.clone(),
                    command: vec!["npm".to_string(), "run".to_string(), label.clone()],
                    source: rel.to_path_buf(),
                };
                classify_command(commands, detected);
                if label.contains("dev") || label.contains("start") {
                    commands.dev.push(DetectedCommand {
                        id: String::new(),
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
                            id: String::new(),
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
            id: String::new(),
            title: "Rust".to_string(),
            summary: extract_cargo_dependencies(&cargo),
            evidence: vec![rel.to_path_buf()],
            tier: NegentropyTier::Structure,
        });
        commands.build.push(DetectedCommand {
            id: String::new(),
            label: format!("cargo build ({})", rel.display()),
            command: vec!["cargo".to_string(), "build".to_string()],
            source: rel.to_path_buf(),
        });
        commands.test.push(DetectedCommand {
            id: String::new(),
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
                    id: String::new(),
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
                        id: String::new(),
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
                        id: String::new(),
                        title: format!("Rust dependency `{name}`"),
                        summary: format!(
                            "Local path dependency from {} to {}.",
                            rel.display(),
                            relative_to_workspace(workspace_path, &dep_path).display()
                        ),
                        evidence: vec![rel.to_path_buf()],
                        tier: NegentropyTier::Structure,
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
            id: String::new(),
            title: "Container infrastructure".to_string(),
            summary: format!(
                "Container/runtime definition detected in {}.",
                rel.display()
            ),
            evidence: vec![rel.to_path_buf()],
            tier: NegentropyTier::Structure,
        });
    }
    if path_text.ends_with(".tf") {
        tech_stack.push(DiscoveryFact {
            id: String::new(),
            title: "Terraform".to_string(),
            summary: format!("Terraform infrastructure detected in {}.", rel.display()),
            evidence: vec![rel.to_path_buf()],
            tier: NegentropyTier::Structure,
        });
    }
    if path_text.contains(".github/workflows") {
        tech_stack.push(DiscoveryFact {
            id: String::new(),
            title: "CI/CD workflow".to_string(),
            summary: summarize_lines(text, 3),
            evidence: vec![rel.to_path_buf()],
            tier: NegentropyTier::Structure,
        });
    }
    if path_text.contains(".loopsmith/config.toml") {
        notes.push(format!(
            "Harness-local LoopSmith config detected at {} and included in command/context extraction.",
            rel.display()
        ));
    }
}

fn detect_project_intent(rel: &Path, text: &str, output: &mut Vec<DiscoveryFact>) {
    let Some(name) = rel.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    let lower = name.to_ascii_lowercase();

    if lower == "readme.md" || lower == "readme" || lower == "readme.rst" || lower == "readme.txt" {
        let summary = if text.len() <= 2048 {
            text.to_string()
        } else {
            summarize_lines(text, 20)
        };
        output.push(DiscoveryFact {
            id: String::new(),
            title: format!("Project README ({})", rel.display()),
            summary,
            evidence: vec![rel.to_path_buf()],
            tier: NegentropyTier::Specification,
        });
        return;
    }

    if lower == "cargo.toml" {
        for line in text.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("description") {
                let rest = rest.trim_start().strip_prefix('=').unwrap_or(rest).trim();
                let desc = rest.trim_matches('"').trim_matches('\'');
                if !desc.is_empty() {
                    output.push(DiscoveryFact {
                        id: String::new(),
                        title: format!("Cargo description ({})", rel.display()),
                        summary: desc.to_string(),
                        evidence: vec![rel.to_path_buf()],
                        tier: NegentropyTier::Specification,
                    });
                    return;
                }
            }
        }
    }

    if lower == "package.json" {
        if let Some(desc_start) = text.find("\"description\"") {
            let rest = &text[desc_start + "\"description\"".len()..];
            let rest = rest
                .trim_start()
                .strip_prefix(':')
                .unwrap_or(rest)
                .trim_start();
            if let Some(val) = rest.strip_prefix('"') {
                if let Some(end) = val.find('"') {
                    let desc = &val[..end];
                    if !desc.is_empty() {
                        output.push(DiscoveryFact {
                            id: String::new(),
                            title: format!("npm description ({})", rel.display()),
                            summary: desc.to_string(),
                            evidence: vec![rel.to_path_buf()],
                            tier: NegentropyTier::Specification,
                        });
                    }
                }
            }
        }
    }
}

fn detect_environment_requirements(rel: &Path, text: &str, output: &mut Vec<DiscoveryFact>) {
    let Some(name) = rel.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    let lower = name.to_ascii_lowercase();

    if lower == "rust-toolchain" || lower == "rust-toolchain.toml" {
        let summary = if text.len() <= 1024 {
            text.to_string()
        } else {
            summarize_lines(text, 5)
        };
        output.push(DiscoveryFact {
            id: String::new(),
            title: format!("Rust toolchain ({})", rel.display()),
            summary,
            evidence: vec![rel.to_path_buf()],
            tier: NegentropyTier::Structure,
        });
    }

    if lower == ".nvmrc" || lower == ".node-version" {
        output.push(DiscoveryFact {
            id: String::new(),
            title: format!("Node version ({})", rel.display()),
            summary: text.trim().to_string(),
            evidence: vec![rel.to_path_buf()],
            tier: NegentropyTier::Structure,
        });
    }

    if lower == ".tool-versions" {
        let summary = if text.len() <= 512 {
            text.to_string()
        } else {
            summarize_lines(text, 5)
        };
        output.push(DiscoveryFact {
            id: String::new(),
            title: format!("Tool versions ({})", rel.display()),
            summary,
            evidence: vec![rel.to_path_buf()],
            tier: NegentropyTier::Structure,
        });
    }

    if lower == ".env.example" || lower == ".env.sample" || lower == ".env.template" {
        let var_names: Vec<&str> = text
            .lines()
            .filter(|l| !l.trim_start().starts_with('#') && l.contains('='))
            .filter_map(|l| l.split('=').next())
            .map(|k| k.trim())
            .take(10)
            .collect();
        if !var_names.is_empty() {
            output.push(DiscoveryFact {
                id: String::new(),
                title: format!("Environment template ({})", rel.display()),
                summary: format!("Required env vars: {}.", var_names.join(", ")),
                evidence: vec![rel.to_path_buf()],
                tier: NegentropyTier::Structure,
            });
        }
    }

    if lower == "docker-compose.yml"
        || lower == "docker-compose.yaml"
        || lower == "compose.yml"
        || lower == "compose.yaml"
    {
        let services: Vec<&str> = text
            .lines()
            .filter_map(|l| {
                let t = l.trim_start();
                if t.ends_with(':')
                    && !l.starts_with(' ') == false
                    && l.starts_with("  ")
                    && !l.starts_with("    ")
                {
                    Some(t.trim_end_matches(':'))
                } else {
                    None
                }
            })
            .take(8)
            .collect();
        let summary = if services.is_empty() {
            summarize_lines(text, 5)
        } else {
            format!("Docker Compose services: {}.", services.join(", "))
        };
        output.push(DiscoveryFact {
            id: String::new(),
            title: format!("Container orchestration ({})", rel.display()),
            summary,
            evidence: vec![rel.to_path_buf()],
            tier: NegentropyTier::Structure,
        });
    }
}

fn detect_change_boundaries(rel: &Path, frozen: &mut Vec<PathBuf>, high_risk: &mut Vec<PathBuf>) {
    let Some(name) = rel.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    let lower = name.to_ascii_lowercase();

    let is_lock_file = lower == "cargo.lock"
        || lower == "package-lock.json"
        || lower == "pnpm-lock.yaml"
        || lower == "yarn.lock"
        || lower == "gemfile.lock"
        || lower == "poetry.lock"
        || lower == "go.sum"
        || lower == "flake.lock";
    if is_lock_file {
        frozen.push(rel.to_path_buf());
        return;
    }

    let path_text = rel.to_string_lossy().to_ascii_lowercase();
    let is_ci_config = path_text.contains(".github/workflows/")
        || path_text.contains(".gitlab-ci")
        || path_text.contains("jenkinsfile")
        || path_text.contains(".circleci/")
        || path_text.contains("bitbucket-pipelines");
    if is_ci_config {
        high_risk.push(rel.to_path_buf());
        return;
    }

    let is_deploy = lower == "dockerfile"
        || lower.starts_with("docker-compose")
        || lower.starts_with("compose.")
        || path_text.contains("/deploy/")
        || path_text.contains("/infra/")
        || lower.ends_with(".tf")
        || lower.ends_with(".tfvars");
    if is_deploy {
        high_risk.push(rel.to_path_buf());
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
            responsibilities: Vec::new(),
            name,
            paths: paths.into_iter().take(8).collect(),
        })
        .collect::<Vec<_>>();

    LayeringProfile {
        summary: format!(
            "Potential architectural layer names inferred from path naming heuristics: {}.",
            layers
                .iter()
                .map(|layer| layer.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        layers,
        allowed_dependency_directions: Vec::new(),
        unresolved_ambiguities: vec![
            "Layer names were inferred from directory naming heuristics; dependency directions were not verified from explicit architecture evidence.".to_string(),
        ],
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

fn assign_source_file_ids(items: &mut [DiscoverySourceFile]) {
    for (index, item) in items.iter_mut().enumerate() {
        item.id = format!("source_file.{:03}", index + 1);
    }
}

fn assign_fact_ids(prefix: &str, items: &mut [DiscoveryFact]) {
    for (index, item) in items.iter_mut().enumerate() {
        item.id = format!("{prefix}.{:03}", index + 1);
    }
}

fn assign_repository_ids(items: &mut [RepositoryProfile]) {
    for (index, item) in items.iter_mut().enumerate() {
        item.id = format!("repository.{:03}", index + 1);
    }
}

fn assign_relationship_ids(items: &mut [DependencyRelationship]) {
    for (index, item) in items.iter_mut().enumerate() {
        item.id = format!("relationship.{:03}", index + 1);
    }
}

fn assign_command_ids(prefix: &str, items: &mut [DetectedCommand]) {
    for (index, item) in items.iter_mut().enumerate() {
        item.id = format!("{prefix}.{:03}", index + 1);
    }
}

fn write_json_pretty<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value).context("failed to serialize json")?;
    fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))
}

fn remove_file_if_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    fs::remove_file(path).with_context(|| format!("failed to remove {}", path.display()))
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
    use super::{
        ChangeBoundaryProfile, CommandCatalog, DetectedCommand, DiscoveryEvidenceChain,
        DiscoveryEvidenceChainStrength, DiscoveryFact, DiscoveryInference, LayeringProfile,
        WorkspaceDiscoveryInference, WorkspaceDiscoveryOverview, WorkspaceDiscoveryPhase,
        WorkspaceDiscoveryRequest, WorkspaceDiscoveryScan, WorkspaceDiscoveryStatus,
        WorkspaceDiscoveryStore, profile_fingerprint, scan_workspace,
        synthesize_profile_from_evidence,
    };
    use crate::worker::{DiscoveryContext, render_discovery_prompt};
    use chrono::{Duration, Utc};
    use std::{fs, path::PathBuf};
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
    fn scanner_keeps_heuristic_layers_ambiguous_without_generic_rules() {
        let temp = tempdir().expect("tempdir");
        fs::create_dir_all(temp.path().join("ui/src")).expect("ui dir");
        fs::create_dir_all(temp.path().join("core/src")).expect("core dir");
        fs::create_dir_all(temp.path().join("worker/src")).expect("worker dir");
        fs::write(
            temp.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"core\", \"worker\"]\n",
        )
        .expect("write root cargo");
        fs::write(temp.path().join("ui/src/main.rs"), "pub fn ui() {}\n").expect("write ui file");
        fs::write(temp.path().join("core/src/lib.rs"), "pub fn core() {}\n")
            .expect("write core file");
        fs::write(
            temp.path().join("worker/src/lib.rs"),
            "pub fn adapter() {}\n",
        )
        .expect("write worker file");

        let scan = scan_workspace(temp.path()).expect("scan");

        assert!(scan.layering.summary.starts_with(
            "Potential architectural layer names inferred from path naming heuristics:"
        ));
        assert!(scan.layering.allowed_dependency_directions.is_empty());
        assert_eq!(
            scan.layering.unresolved_ambiguities,
            vec![
                "Layer names were inferred from directory naming heuristics; dependency directions were not verified from explicit architecture evidence."
                    .to_string()
            ]
        );
        assert!(
            scan.layering
                .layers
                .iter()
                .all(|layer| layer.responsibilities.is_empty())
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
            previous_inference: None,
        };

        let profile = request.synthesize_profile();
        let fingerprint = profile_fingerprint(&profile).expect("fingerprint");
        assert!(!fingerprint.is_empty());
        assert!(!profile.summary.is_empty());
        assert!(matches!(profile.commands, CommandCatalog { .. }));
    }

    #[test]
    fn overview_and_profile_synthesis_dedupe_rendered_fact_and_command_strings() {
        let scan = WorkspaceDiscoveryScan {
            workspace_path: PathBuf::from("/tmp/workspace"),
            scanned_at: Utc::now(),
            workspace_fingerprint: "fingerprint".to_string(),
            source_files: Vec::new(),
            tech_stack: vec![
                DiscoveryFact {
                    id: "tech_stack.001".to_string(),
                    title: "Rust".to_string(),
                    summary: "Rust crate".to_string(),
                    evidence: vec![PathBuf::from("Cargo.toml")],
                    tier: super::NegentropyTier::Structure,
                },
                DiscoveryFact {
                    id: "tech_stack.002".to_string(),
                    title: "Rust".to_string(),
                    summary: "Workspace contains more Rust.".to_string(),
                    evidence: vec![PathBuf::from("crates/core/Cargo.toml")],
                    tier: super::NegentropyTier::Structure,
                },
            ],
            repositories: Vec::new(),
            dependency_relationships: Vec::new(),
            api_contracts: vec![
                DiscoveryFact {
                    id: "api_contract.001".to_string(),
                    title: "HTTP API".to_string(),
                    summary: "REST interface".to_string(),
                    evidence: vec![PathBuf::from("openapi.yaml")],
                    tier: super::NegentropyTier::Specification,
                },
                DiscoveryFact {
                    id: "api_contract.002".to_string(),
                    title: "HTTP API".to_string(),
                    summary: "Same contract from another source".to_string(),
                    evidence: vec![PathBuf::from("docs/api.md")],
                    tier: super::NegentropyTier::Specification,
                },
            ],
            layering: LayeringProfile {
                summary: "layered".to_string(),
                layers: Vec::new(),
                allowed_dependency_directions: Vec::new(),
                unresolved_ambiguities: Vec::new(),
            },
            user_journeys: Vec::new(),
            e2e_test_cases: Vec::new(),
            auth: Vec::new(),
            coding_conventions: Vec::new(),
            commands: CommandCatalog {
                build: Vec::new(),
                test: vec![
                    DetectedCommand {
                        id: "command.test.001".to_string(),
                        label: "cargo test".to_string(),
                        command: vec!["cargo".to_string(), "test".to_string()],
                        source: PathBuf::from("Cargo.toml"),
                    },
                    DetectedCommand {
                        id: "command.test.002".to_string(),
                        label: "make test".to_string(),
                        command: vec!["cargo".to_string(), "test".to_string()],
                        source: PathBuf::from("Makefile"),
                    },
                ],
                dev: Vec::new(),
            },
            scan_notes: Vec::new(),
            project_intent: Vec::new(),
            environment_requirements: Vec::new(),
            change_boundaries: ChangeBoundaryProfile::default(),
        };

        let overview = WorkspaceDiscoveryOverview::from_sources(Some(&scan), None, None);
        assert_eq!(overview.tech_stack, vec!["Rust".to_string()]);
        assert_eq!(overview.api_contracts, vec!["HTTP API".to_string()]);
        assert_eq!(overview.api_contract_count, 1);
        assert_eq!(overview.test_commands, vec!["cargo test".to_string()]);
        assert_eq!(overview.test_command_count, 1);

        let profile = synthesize_profile_from_evidence(&scan);
        assert!(
            profile
                .key_concepts
                .iter()
                .any(|entry| entry == "Primary stack: Rust.")
        );
        assert!(
            profile
                .key_concepts
                .iter()
                .any(|entry| entry == "Primary test commands: cargo test.")
        );
    }

    #[test]
    fn scanner_excludes_gitignore_and_dictionary_artifacts_from_discovery_inputs() {
        let temp = tempdir().expect("tempdir");
        fs::create_dir_all(temp.path().join("docs")).expect("docs dir");
        fs::create_dir_all(temp.path().join("config")).expect("config dir");
        fs::create_dir_all(temp.path().join("docs/dictionaries")).expect("dictionaries dir");
        fs::write(
            temp.path().join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
        )
        .expect("write cargo");
        fs::write(temp.path().join("README.md"), "# Fixture\n").expect("write readme");
        fs::write(temp.path().join(".gitignore"), "target\n").expect("write root gitignore");
        fs::write(temp.path().join("docs/.gitignore"), "generated/\n")
            .expect("write nested gitignore");
        fs::write(
            temp.path().join("docs/dictionary.md"),
            "# Domain dictionary\nterm: value\n",
        )
        .expect("write markdown dictionary");
        fs::write(
            temp.path().join("config/dictionaries.yml"),
            "terms:\n  - widget\n",
        )
        .expect("write yaml dictionary");
        fs::write(
            temp.path().join("docs/dictionaries/terms.yml"),
            "terms:\n  - gadget\n",
        )
        .expect("write dictionary directory file");

        let scan = scan_workspace(temp.path()).expect("scan");
        let source_paths = scan
            .source_files
            .iter()
            .map(|file| file.path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert!(source_paths.iter().any(|path| path == "Cargo.toml"));
        assert!(source_paths.iter().any(|path| path == "README.md"));
        assert!(!source_paths.iter().any(|path| path == ".gitignore"));
        assert!(!source_paths.iter().any(|path| path == "docs/.gitignore"));
        assert!(!source_paths.iter().any(|path| path == "docs/dictionary.md"));
        assert!(
            !source_paths
                .iter()
                .any(|path| path == "config/dictionaries.yml")
        );
        assert!(
            !source_paths
                .iter()
                .any(|path| path == "docs/dictionaries/terms.yml")
        );

        let request = WorkspaceDiscoveryRequest {
            scan,
            previous_profile: None,
            previous_inference: None,
        };
        let profile = request.synthesize_profile();
        let request_json =
            serde_json::to_string_pretty(&request).expect("serialize discovery request");
        let profile_json =
            serde_json::to_string_pretty(&profile).expect("serialize workspace profile");
        let prompt_template = temp.path().join("discovery.md");
        let schema_path = temp.path().join("workspace-profile.schema.json");
        fs::write(&prompt_template, "discovery\n").expect("write prompt template");
        fs::write(&schema_path, "{}\n").expect("write schema");
        let prompt = render_discovery_prompt(
            &DiscoveryContext {
                workspace: temp.path().to_path_buf(),
                discovery_prompt: prompt_template,
                workspace_profile_schema: schema_path.clone(),
            },
            &schema_path,
            &request,
        )
        .expect("render prompt");

        assert!(request_json.contains("Cargo.toml"));
        assert!(prompt.contains("Cargo.toml"));
        for ignored_path in [
            ".gitignore",
            "docs/.gitignore",
            "docs/dictionary.md",
            "config/dictionaries.yml",
            "docs/dictionaries/terms.yml",
        ] {
            assert!(!request_json.contains(ignored_path));
            assert!(!profile_json.contains(ignored_path));
            assert!(!prompt.contains(ignored_path));
        }
    }

    #[test]
    fn scanner_respects_gitignore_patterns_for_files_and_directories() {
        let temp = tempdir().expect("tempdir");
        fs::create_dir_all(temp.path().join("src")).expect("src dir");
        fs::create_dir_all(temp.path().join("ignored-dir")).expect("ignored dir");
        fs::create_dir_all(temp.path().join("docs/drafts")).expect("drafts dir");
        fs::write(
            temp.path().join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
        )
        .expect("write cargo");
        fs::write(temp.path().join("src/lib.rs"), "pub fn keep() {}\n").expect("write lib");
        fs::write(temp.path().join("notes.snapshot"), "ignored snapshot\n")
            .expect("write ignored snapshot");
        fs::write(
            temp.path().join("ignored-dir/package.json"),
            r#"{"name":"ignored"}"#,
        )
        .expect("write ignored package");
        fs::write(temp.path().join("docs/guide.md"), "# Keep\n").expect("write guide");
        fs::write(temp.path().join("docs/spec.tmp"), "temporary\n").expect("write tmp");
        fs::write(temp.path().join("docs/drafts/plan.md"), "# Draft\n").expect("write draft");
        fs::write(temp.path().join(".gitignore"), "ignored-dir/\n*.snapshot\n")
            .expect("write root gitignore");
        fs::write(temp.path().join("docs/.gitignore"), "drafts/\n*.tmp\n")
            .expect("write nested gitignore");

        let scan = scan_workspace(temp.path()).expect("scan");
        let source_paths = scan
            .source_files
            .iter()
            .map(|file| file.path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert!(source_paths.iter().any(|path| path == "Cargo.toml"));
        assert!(source_paths.iter().any(|path| path == "src/lib.rs"));
        assert!(source_paths.iter().any(|path| path == "docs/guide.md"));
        for ignored_path in [
            "notes.snapshot",
            "ignored-dir/package.json",
            "docs/spec.tmp",
            "docs/drafts/plan.md",
        ] {
            assert!(!source_paths.iter().any(|path| path == ignored_path));
        }

        let request = WorkspaceDiscoveryRequest {
            scan,
            previous_profile: None,
            previous_inference: None,
        };
        let prompt_template = temp.path().join("discovery.md");
        let schema_path = temp.path().join("workspace-profile.schema.json");
        fs::write(&prompt_template, "discovery\n").expect("write prompt template");
        fs::write(&schema_path, "{}\n").expect("write schema");
        let request_json =
            serde_json::to_string_pretty(&request).expect("serialize discovery request");
        let prompt = render_discovery_prompt(
            &DiscoveryContext {
                workspace: temp.path().to_path_buf(),
                discovery_prompt: prompt_template,
                workspace_profile_schema: schema_path.clone(),
            },
            &schema_path,
            &request,
        )
        .expect("render prompt");

        assert!(request_json.contains("docs/guide.md"));
        assert!(prompt.contains("docs/guide.md"));
        for ignored_path in [
            "notes.snapshot",
            "ignored-dir/package.json",
            "docs/spec.tmp",
            "docs/drafts/plan.md",
        ] {
            assert!(!request_json.contains(ignored_path));
            assert!(!prompt.contains(ignored_path));
        }
    }

    #[test]
    fn discovery_store_reconciles_orphaned_polishing_status_to_fallback() {
        let temp = tempdir().expect("tempdir");
        fs::write(
            temp.path().join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
        )
        .expect("write cargo");
        fs::create_dir_all(temp.path().join("src")).expect("src dir");
        fs::write(temp.path().join("src/lib.rs"), "pub fn fixture() {}\n").expect("write lib");

        let scan = scan_workspace(temp.path()).expect("scan");
        let profile = WorkspaceDiscoveryRequest {
            scan: scan.clone(),
            previous_profile: None,
            previous_inference: None,
        }
        .synthesize_profile();
        let store = WorkspaceDiscoveryStore::new(temp.path());
        store.save_scan(&scan).expect("save scan");
        store.save_profile(&profile).expect("save profile");
        store
            .save_status(&WorkspaceDiscoveryStatus {
                workspace_path: temp.path().to_path_buf(),
                scan_path: store.scan_path(),
                evidence_path: store.evidence_path(),
                profile_path: store.profile_path(),
                inference_path: store.inference_path(),
                workspace_fingerprint: scan.workspace_fingerprint,
                profile_fingerprint: Some(profile_fingerprint(&profile).expect("fingerprint")),
                last_scanned_at: scan.scanned_at,
                last_refreshed_at: Some(profile.generated_at),
                last_refresh_error: None,
                used_fallback_profile: false,
                current_phase: WorkspaceDiscoveryPhase::Polishing,
                phase_heartbeat_at: Some(Utc::now() - Duration::seconds(90)),
            })
            .expect("save status");

        let status = store
            .load_status()
            .expect("load status")
            .expect("status exists");

        assert_eq!(
            status.current_phase,
            WorkspaceDiscoveryPhase::UsingFallbackProfile
        );
        assert!(status.used_fallback_profile);
        assert_eq!(
            status.last_refresh_error.as_deref(),
            Some("previous discovery refresh ended before completion during polishing")
        );
    }

    #[test]
    fn inference_validation_rejects_confidence_ten_without_single_strong_chain() {
        let temp = tempdir().expect("tempdir");
        fs::write(
            temp.path().join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
        )
        .expect("write cargo");
        fs::create_dir_all(temp.path().join("src")).expect("src dir");
        fs::write(temp.path().join("src/lib.rs"), "pub fn fixture() {}\n").expect("write lib");

        let scan = scan_workspace(temp.path()).expect("scan");
        let inference = WorkspaceDiscoveryInference {
            workspace_path: temp.path().to_path_buf(),
            generated_at: Utc::now(),
            summary: "bad inference".to_string(),
            inferences: vec![DiscoveryInference {
                id: "bad.inference".to_string(),
                category: "key_concept".to_string(),
                statement: "Invalid certainty".to_string(),
                confidence: 10,
                rationale: "broken".to_string(),
                evidence_chains: vec![DiscoveryEvidenceChain {
                    label: "not strong enough".to_string(),
                    strength: DiscoveryEvidenceChainStrength::Moderate,
                    evidence_ids: scan.default_supporting_evidence_ids(1),
                }],
                assumptions: Vec::new(),
                contradictions: Vec::new(),
            }],
            risks: Vec::new(),
        };

        let err = inference
            .validate(&scan)
            .expect_err("validation should fail");
        assert!(
            err.to_string()
                .contains("confidence 10 without any strong evidence chain")
        );
    }

    #[test]
    fn layer_candidates_match_exact_path_segments_not_substrings() {
        let mut layers = std::collections::BTreeMap::new();
        super::extend_layer_candidates(&mut layers, std::path::Path::new("build/circuits.rs"));
        assert!(
            !layers.contains_key("ui"),
            "circuits.rs contains 'ui' as a substring but should not match the 'ui' layer"
        );

        super::extend_layer_candidates(&mut layers, std::path::Path::new("src/gui_utils.rs"));
        assert!(
            !layers.contains_key("ui"),
            "gui_utils contains 'ui' as a substring but should not match the 'ui' layer"
        );

        super::extend_layer_candidates(&mut layers, std::path::Path::new("ui/components/App.tsx"));
        assert!(
            layers.contains_key("ui"),
            "ui/ directory should match the 'ui' layer"
        );
    }

    #[test]
    fn detect_auth_ignores_standalone_token_without_auth_context() {
        let mut results = Vec::new();
        super::detect_auth(
            std::path::Path::new("src/tokenizer.rs"),
            "fn tokenize(input: &str) -> Vec<Token> {\n    let token = next_token();\n}\n",
            &mut results,
        );
        assert!(
            results.is_empty(),
            "tokenizer with 'token' keyword should not trigger auth detection"
        );

        let mut results = Vec::new();
        super::detect_auth(
            std::path::Path::new("src/auth.rs"),
            "fn authenticate(user: &str) -> Result<Session> {\n    let token = generate_jwt();\n}\n",
            &mut results,
        );
        assert!(
            !results.is_empty(),
            "file with 'auth' in path should trigger auth detection"
        );
    }

    #[test]
    fn conventions_receive_specification_tier() {
        let temp = tempdir().expect("tempdir");
        let content = "root = true\n\n[*]\nindent_style = space\nindent_size = 2\n";
        fs::write(temp.path().join(".editorconfig"), content).expect("write editorconfig");
        fs::create_dir_all(temp.path().join("src")).expect("src dir");
        fs::write(temp.path().join("src/lib.rs"), "pub fn main() {}\n").expect("write lib");

        let scan = scan_workspace(temp.path()).expect("scan");
        let convention = scan
            .coding_conventions
            .iter()
            .find(|f| f.title == ".editorconfig")
            .expect("should detect .editorconfig");
        assert_eq!(
            convention.tier,
            super::NegentropyTier::Specification,
            "convention files should be classified as Specification tier"
        );
        assert!(
            convention.summary.contains("root = true"),
            "small convention files should have full content in summary"
        );
    }

    #[test]
    fn synthesize_profile_consolidates_duplicate_tech_stack_entries() {
        let scan = WorkspaceDiscoveryScan {
            workspace_path: PathBuf::from("/tmp/test"),
            scanned_at: Utc::now(),
            workspace_fingerprint: "abc".to_string(),
            source_files: Vec::new(),
            tech_stack: vec![
                DiscoveryFact {
                    id: "ts.001".to_string(),
                    title: "Rust".to_string(),
                    summary: "Rust crate from Cargo.toml".to_string(),
                    evidence: vec![PathBuf::from("Cargo.toml")],
                    tier: super::NegentropyTier::Structure,
                },
                DiscoveryFact {
                    id: "ts.002".to_string(),
                    title: "Rust".to_string(),
                    summary: "Another Rust crate".to_string(),
                    evidence: vec![PathBuf::from("sub/Cargo.toml")],
                    tier: super::NegentropyTier::Structure,
                },
            ],
            repositories: Vec::new(),
            dependency_relationships: Vec::new(),
            api_contracts: Vec::new(),
            layering: LayeringProfile {
                summary: "none".to_string(),
                layers: Vec::new(),
                allowed_dependency_directions: Vec::new(),
                unresolved_ambiguities: Vec::new(),
            },
            user_journeys: Vec::new(),
            e2e_test_cases: Vec::new(),
            auth: Vec::new(),
            coding_conventions: Vec::new(),
            commands: CommandCatalog::default(),
            scan_notes: Vec::new(),
            project_intent: Vec::new(),
            environment_requirements: Vec::new(),
            change_boundaries: ChangeBoundaryProfile::default(),
        };

        let profile = synthesize_profile_from_evidence(&scan);
        assert_eq!(
            profile.tech_stack.len(),
            1,
            "duplicate 'Rust' entries should be consolidated into one"
        );
        assert_eq!(profile.tech_stack[0].title, "Rust");
        assert_eq!(
            profile.tech_stack[0].evidence.len(),
            2,
            "evidence paths from both entries should be merged"
        );
    }

    #[test]
    fn prompt_context_outputs_specifications_before_structure() {
        let scan = WorkspaceDiscoveryScan {
            workspace_path: PathBuf::from("/tmp/test"),
            scanned_at: Utc::now(),
            workspace_fingerprint: "abc".to_string(),
            source_files: Vec::new(),
            tech_stack: vec![DiscoveryFact {
                id: "ts.001".to_string(),
                title: "Rust".to_string(),
                summary: "Rust crate".to_string(),
                evidence: vec![PathBuf::from("Cargo.toml")],
                tier: super::NegentropyTier::Structure,
            }],
            repositories: Vec::new(),
            dependency_relationships: Vec::new(),
            api_contracts: vec![DiscoveryFact {
                id: "ac.001".to_string(),
                title: "gRPC schema".to_string(),
                summary: "service API".to_string(),
                evidence: vec![PathBuf::from("api.proto")],
                tier: super::NegentropyTier::Specification,
            }],
            layering: LayeringProfile {
                summary: "none".to_string(),
                layers: Vec::new(),
                allowed_dependency_directions: Vec::new(),
                unresolved_ambiguities: Vec::new(),
            },
            user_journeys: Vec::new(),
            e2e_test_cases: Vec::new(),
            auth: Vec::new(),
            coding_conventions: vec![DiscoveryFact {
                id: "cc.001".to_string(),
                title: "AGENTS.md".to_string(),
                summary: "# Rules\nFollow strict coding standards.".to_string(),
                evidence: vec![PathBuf::from("AGENTS.md")],
                tier: super::NegentropyTier::Specification,
            }],
            commands: CommandCatalog::default(),
            scan_notes: Vec::new(),
            project_intent: Vec::new(),
            environment_requirements: Vec::new(),
            change_boundaries: ChangeBoundaryProfile::default(),
        };

        let profile = synthesize_profile_from_evidence(&scan);
        let context = profile.prompt_context();
        let spec_pos = context
            .find("specifications")
            .expect("should contain specifications section");
        let contract_pos = context
            .find("explicit_api_contracts")
            .expect("should contain api contracts section");
        let auth_pos = context.find("auth");
        assert!(
            spec_pos < contract_pos,
            "specifications should appear before api contracts"
        );
        if let Some(auth_p) = auth_pos {
            assert!(
                spec_pos < auth_p,
                "specifications should appear before auth"
            );
        }
    }

    #[test]
    fn detect_project_intent_extracts_readme() {
        let temp = tempdir().expect("tempdir");
        let readme = "# My Project\n\nA tool that does awesome things for developers.\n";
        fs::write(temp.path().join("README.md"), readme).expect("write readme");
        fs::create_dir_all(temp.path().join("src")).expect("src dir");
        fs::write(temp.path().join("src/lib.rs"), "pub fn main() {}\n").expect("write lib");

        let scan = scan_workspace(temp.path()).expect("scan");
        assert!(
            !scan.project_intent.is_empty(),
            "should detect project intent from README.md"
        );
        let intent = &scan.project_intent[0];
        assert_eq!(intent.tier, super::NegentropyTier::Specification);
        assert!(
            intent.summary.contains("awesome things"),
            "should include README content in summary"
        );
    }

    #[test]
    fn detect_project_intent_extracts_cargo_description() {
        let temp = tempdir().expect("tempdir");
        let cargo = "[package]\nname = \"test-crate\"\nversion = \"0.1.0\"\ndescription = \"A workspace orchestration tool\"\n";
        fs::write(temp.path().join("Cargo.toml"), cargo).expect("write cargo");
        fs::create_dir_all(temp.path().join("src")).expect("src dir");
        fs::write(temp.path().join("src/lib.rs"), "pub fn main() {}\n").expect("write lib");

        let scan = scan_workspace(temp.path()).expect("scan");
        let intent = scan
            .project_intent
            .iter()
            .find(|f| f.title.contains("Cargo description"));
        assert!(
            intent.is_some(),
            "should detect project intent from Cargo.toml description"
        );
        assert!(
            intent
                .unwrap()
                .summary
                .contains("workspace orchestration tool"),
            "should extract the description text"
        );
    }

    #[test]
    fn detect_change_boundaries_classifies_lock_files_as_frozen() {
        let temp = tempdir().expect("tempdir");
        let cargo = "[package]\nname = \"test-crate\"\nversion = \"0.1.0\"\n";
        fs::write(temp.path().join("Cargo.toml"), cargo).expect("write cargo");
        fs::write(temp.path().join("Cargo.lock"), "# lock file content\n").expect("write lock");
        fs::create_dir_all(temp.path().join("src")).expect("src dir");
        fs::write(temp.path().join("src/lib.rs"), "pub fn main() {}\n").expect("write lib");

        let scan = scan_workspace(temp.path()).expect("scan");
        assert!(
            scan.change_boundaries
                .frozen_paths
                .iter()
                .any(|p| p.to_string_lossy().contains("Cargo.lock")),
            "Cargo.lock should be classified as a frozen path"
        );
    }

    #[test]
    fn detect_environment_requirements_extracts_rust_toolchain() {
        let temp = tempdir().expect("tempdir");
        let toolchain =
            "[toolchain]\nchannel = \"1.78.0\"\ncomponents = [\"rustfmt\", \"clippy\"]\n";
        fs::write(temp.path().join("rust-toolchain.toml"), toolchain).expect("write toolchain");
        let cargo = "[package]\nname = \"test-crate\"\nversion = \"0.1.0\"\n";
        fs::write(temp.path().join("Cargo.toml"), cargo).expect("write cargo");
        fs::create_dir_all(temp.path().join("src")).expect("src dir");
        fs::write(temp.path().join("src/lib.rs"), "pub fn main() {}\n").expect("write lib");

        let scan = scan_workspace(temp.path()).expect("scan");
        let req = scan
            .environment_requirements
            .iter()
            .find(|f| f.title.contains("Rust toolchain"));
        assert!(
            req.is_some(),
            "should detect rust-toolchain.toml as environment requirement"
        );
        assert!(
            req.unwrap().summary.contains("1.78.0"),
            "should include toolchain version in summary"
        );
    }

    #[test]
    fn prompt_context_includes_project_intent_before_key_concepts() {
        let scan = WorkspaceDiscoveryScan {
            workspace_path: PathBuf::from("/tmp/test"),
            scanned_at: Utc::now(),
            workspace_fingerprint: "abc".to_string(),
            source_files: Vec::new(),
            tech_stack: Vec::new(),
            repositories: Vec::new(),
            dependency_relationships: Vec::new(),
            api_contracts: Vec::new(),
            layering: LayeringProfile {
                summary: "none".to_string(),
                layers: Vec::new(),
                allowed_dependency_directions: Vec::new(),
                unresolved_ambiguities: Vec::new(),
            },
            user_journeys: Vec::new(),
            e2e_test_cases: Vec::new(),
            auth: Vec::new(),
            coding_conventions: Vec::new(),
            commands: CommandCatalog::default(),
            scan_notes: Vec::new(),
            project_intent: vec![DiscoveryFact {
                id: "pi.001".to_string(),
                title: "Project README".to_string(),
                summary: "A workspace orchestration tool.".to_string(),
                evidence: vec![PathBuf::from("README.md")],
                tier: super::NegentropyTier::Specification,
            }],
            environment_requirements: vec![DiscoveryFact {
                id: "env.001".to_string(),
                title: "Rust toolchain".to_string(),
                summary: "channel = 1.78.0".to_string(),
                evidence: vec![PathBuf::from("rust-toolchain.toml")],
                tier: super::NegentropyTier::Structure,
            }],
            change_boundaries: ChangeBoundaryProfile {
                frozen_paths: vec![PathBuf::from("Cargo.lock")],
                high_risk_paths: vec![PathBuf::from(".github/workflows/ci.yml")],
            },
        };

        let profile = synthesize_profile_from_evidence(&scan);
        let context = profile.prompt_context();
        let intent_pos = context
            .find("project_intent")
            .expect("should contain project_intent");
        if let Some(kc_pos) = context.find("key_concepts") {
            assert!(
                intent_pos < kc_pos,
                "project_intent should appear before key_concepts"
            );
        }
        assert!(
            context.contains("frozen_files"),
            "should contain frozen_files section"
        );
        assert!(
            context.contains("Cargo.lock"),
            "should list Cargo.lock as frozen"
        );
        assert!(
            context.contains("high_risk_files"),
            "should contain high_risk_files section"
        );
        assert!(
            context.contains("environment_requirements"),
            "should contain environment_requirements section"
        );
    }
}
