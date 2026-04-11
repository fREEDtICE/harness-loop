use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use loopsmith_core::{
    artifacts::FileArtifactStore,
    config::{AppConfig, ResolvedConfig},
    discovery::{
        WorkspaceDiscoveryInference, WorkspaceDiscoveryPayload, WorkspaceDiscoveryPhase,
        WorkspaceDiscoveryRequest, WorkspaceDiscoveryStatus, WorkspaceDiscoveryStore,
        WorkspaceProfile, WorkspaceProfileSelection, profile_fingerprint, scan_workspace,
    },
    domain::{
        PlannerConversationRequest, PlannerConversationResponse, PlannerConversationTurn,
        PromptOverrides, PromptSnapshot, QaStatus, RunLifecycleStatus, RunRequest, RunState,
    },
    paths::normalize_path,
    worker::{
        DiscoveryContext, PlannerConversationArtifactSet, PlannerConversationContext, WorkerAdapter,
    },
};
use loopsmith_worker_factory::{build_configured_worker, build_discovery_worker};
use serde::{Deserialize, Serialize};
use tempfile::tempdir;
use tokio::time::{Duration, interval};

const DEFAULT_DISCOVERY_PROMPT: &str = include_str!("../../../prompts/discovery.md");
const DEFAULT_WORKSPACE_PROFILE_SCHEMA: &str =
    include_str!("../../../schemas/workspace-profile.json");
const DEFAULT_WORKSPACE_INFERENCE_SCHEMA: &str =
    include_str!("../../../schemas/workspace-inference.json");
const DEFAULT_PLANNER_CONVERSATION_PROMPT: &str = include_str!("../../../prompts/planner-chat.md");
const DEFAULT_PLANNER_CONVERSATION_SCHEMA: &str =
    include_str!("../../../schemas/planner-chat.json");
const DISCOVERY_PHASE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchDraft {
    pub workspace_path: PathBuf,
    pub config_path: PathBuf,
    pub request_draft: String,
    pub prompt_overrides: PromptOverrides,
    pub feature_limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerConversationDraft {
    pub workspace_path: PathBuf,
    pub config_path: PathBuf,
    pub request_draft: String,
    pub prompt_overrides: PromptOverrides,
    pub feature_limit: Option<usize>,
    #[serde(default)]
    pub conversation: Vec<PlannerConversationTurn>,
}

#[derive(Debug, Clone)]
pub struct PreparedLaunch {
    pub config_path: PathBuf,
    pub prompt_snapshot: PromptSnapshot,
    resolved_config: ResolvedConfig,
    run_request: RunRequest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceRunSummary {
    pub run_root: PathBuf,
    pub run_title: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub lifecycle: RunLifecycleStatus,
    pub final_status: Option<QaStatus>,
    pub current_feature_index: usize,
    pub total_features: usize,
    #[serde(default)]
    pub awaiting_feature_confirmation: bool,
    pub active_stage: Option<loopsmith_core::domain::ActiveRunStage>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct HarnessUiService;

impl HarnessUiService {
    pub fn load_effective_prompts(
        &self,
        config_path: impl AsRef<Path>,
        overrides: &PromptOverrides,
    ) -> Result<PromptSnapshot> {
        let config_path = normalize_path(config_path.as_ref().to_path_buf());
        let resolved_config = AppConfig::load(&config_path)?;
        resolve_effective_prompts(&resolved_config, overrides)
    }

    pub fn validate_workspace_path(&self, workspace_path: impl AsRef<Path>) -> Result<PathBuf> {
        let workspace_path = normalize_path(workspace_path.as_ref().to_path_buf());
        if !workspace_path.exists() {
            bail!("workspace {} does not exist", workspace_path.display());
        }
        if !workspace_path.is_dir() {
            bail!("workspace {} is not a directory", workspace_path.display());
        }
        Ok(workspace_path)
    }

    pub fn prepare_launch(&self, draft: LaunchDraft) -> Result<PreparedLaunch> {
        let workspace_path = self.validate_workspace_path(draft.workspace_path)?;
        if draft.request_draft.trim().is_empty() {
            bail!("request draft must not be empty");
        }

        let config_path = normalize_path(draft.config_path);
        let resolved_config = AppConfig::load(&config_path)?;
        let prompt_snapshot = resolve_effective_prompts(&resolved_config, &draft.prompt_overrides)?;
        let confirm_before_build = resolved_config.runtime.confirm_before_build;

        Ok(PreparedLaunch {
            config_path: config_path.clone(),
            prompt_snapshot,
            resolved_config,
            run_request: RunRequest {
                user_request: draft.request_draft,
                source_workspace: workspace_path,
                feature_limit: draft.feature_limit,
                confirm_before_build,
                selected_config: Some(config_path),
                prompt_overrides: draft.prompt_overrides,
                workspace_profile: None,
            },
        })
    }

    pub async fn start_run(&self, draft: LaunchDraft) -> Result<RunState> {
        let prepared = self.prepare_launch(draft)?;
        self.start_prepared_run(prepared).await
    }

    pub async fn discover_workspace(
        &self,
        config_path: impl AsRef<Path>,
        workspace_path: impl AsRef<Path>,
    ) -> Result<WorkspaceProfileSelection> {
        let config_path = normalize_path(config_path.as_ref().to_path_buf());
        let workspace_path = self.validate_workspace_path(workspace_path)?;
        let resolved_config = AppConfig::load(&config_path)?;
        let discovery_worker = build_discovery_worker(&resolved_config)?;
        refresh_workspace_profile(&resolved_config, discovery_worker.as_ref(), &workspace_path)
            .await
    }

    pub async fn consult_planner(
        &self,
        draft: PlannerConversationDraft,
    ) -> Result<PlannerConversationResponse> {
        let workspace_path = self.validate_workspace_path(draft.workspace_path)?;
        let config_path = normalize_path(draft.config_path);
        let resolved_config = AppConfig::load(&config_path)?;
        let prompt_snapshot = resolve_effective_prompts(&resolved_config, &draft.prompt_overrides)?;
        let discovery_worker = build_discovery_worker(&resolved_config)?;
        let workspace_profile =
            refresh_workspace_profile(&resolved_config, discovery_worker.as_ref(), &workspace_path)
                .await?;
        let planner_worker = build_discovery_worker(&resolved_config)?;
        let request = PlannerConversationRequest {
            request_draft: draft.request_draft,
            feature_limit: draft.feature_limit,
            conversation: draft.conversation,
        };
        let temp = tempdir().context("failed to create planner consultation temp directory")?;
        let prompt_file = temp.path().join("planner-consult.md");
        let schema_file = temp.path().join("planner-consult-schema.json");
        let output_file = temp.path().join("planner-consult-output.json");
        let stdout_log = temp.path().join("planner-consult-stdout.log");
        let stderr_log = temp.path().join("planner-consult-stderr.log");
        let wrapped_prompt = planner_conversation_prompt(&prompt_snapshot.planner);
        fs::write(&prompt_file, wrapped_prompt)
            .with_context(|| format!("failed to write {}", prompt_file.display()))?;
        fs::write(&schema_file, DEFAULT_PLANNER_CONVERSATION_SCHEMA)
            .with_context(|| format!("failed to write {}", schema_file.display()))?;
        let artifacts = PlannerConversationArtifactSet {
            prompt_file: prompt_file.clone(),
            output_file: output_file.clone(),
            stdout_log: stdout_log.clone(),
            stderr_log: stderr_log.clone(),
        };
        let context = PlannerConversationContext {
            workspace: workspace_path,
            planner_conversation_prompt: prompt_file,
            planner_conversation_schema: schema_file,
            workspace_profile_artifact: Some(workspace_profile.canonical_profile_path.clone()),
            workspace_profile_context: Some(workspace_profile.profile.prompt_context()),
        };
        planner_worker
            .consult_planner(&context, &artifacts, &request)
            .await?;
        let bytes = fs::read(&output_file)
            .with_context(|| format!("failed to read {}", output_file.display()))?;
        serde_json::from_slice(&bytes)
            .with_context(|| format!("failed to parse {}", output_file.display()))
    }

    pub async fn start_prepared_run(&self, mut prepared: PreparedLaunch) -> Result<RunState> {
        let discovery_worker = build_discovery_worker(&prepared.resolved_config)?;
        let workspace_profile = refresh_workspace_profile(
            &prepared.resolved_config,
            discovery_worker.as_ref(),
            &prepared.run_request.source_workspace,
        )
        .await?;
        prepared.run_request.workspace_profile = Some(workspace_profile);

        let artifact_store =
            FileArtifactStore::new(prepared.resolved_config.storage.runs_dir.clone());
        let worker = build_configured_worker(&prepared.resolved_config)?;
        let controller = loopsmith_core::controller::HarnessController::new(
            prepared.resolved_config,
            artifact_store,
            worker,
        );
        controller.start_run(prepared.run_request).await
    }

    pub async fn resume_run(
        &self,
        config_path: impl AsRef<Path>,
        run_root: impl AsRef<Path>,
    ) -> Result<RunState> {
        let config_path = normalize_path(config_path.as_ref().to_path_buf());
        let run_root = normalize_path(run_root.as_ref().to_path_buf());
        let resolved_config = AppConfig::load(&config_path)?;
        with_selected_worker(resolved_config, |controller| async move {
            controller.resume_run(run_root).await
        })
        .await
    }

    pub fn inspect_run(
        &self,
        config_path: impl AsRef<Path>,
        run_root: impl AsRef<Path>,
    ) -> Result<RunState> {
        let config_path = normalize_path(config_path.as_ref().to_path_buf());
        let run_root = normalize_path(run_root.as_ref().to_path_buf());
        let resolved_config = AppConfig::load(&config_path)?;
        let artifact_store = FileArtifactStore::new(resolved_config.storage.runs_dir.clone());
        let worker = build_configured_worker(&resolved_config)?;
        let controller = loopsmith_core::controller::HarnessController::new(
            resolved_config,
            artifact_store,
            worker,
        );
        controller.inspect_run(run_root)
    }

    pub fn list_runs_for_workspace(
        &self,
        config_path: impl AsRef<Path>,
        workspace_path: impl AsRef<Path>,
    ) -> Result<Vec<WorkspaceRunSummary>> {
        let config_path = normalize_path(config_path.as_ref().to_path_buf());
        let workspace_path = normalize_path(workspace_path.as_ref().to_path_buf());
        let resolved_config = AppConfig::load(&config_path)?;
        let mut runs = Vec::new();

        if !resolved_config.storage.runs_dir.exists() {
            return Ok(runs);
        }

        for entry in fs::read_dir(&resolved_config.storage.runs_dir).with_context(|| {
            format!(
                "failed to read runs directory {}",
                resolved_config.storage.runs_dir.display()
            )
        })? {
            let entry = entry.with_context(|| {
                format!(
                    "failed to read entry under {}",
                    resolved_config.storage.runs_dir.display()
                )
            })?;
            let run_root = entry.path();
            if !entry
                .file_type()
                .with_context(|| format!("failed to stat {}", run_root.display()))?
                .is_dir()
            {
                continue;
            }

            let manifest_path = run_root.join("manifest.json");
            if !manifest_path.exists() {
                continue;
            }

            let bytes = fs::read(&manifest_path)
                .with_context(|| format!("failed to read {}", manifest_path.display()))?;
            let state: RunState = serde_json::from_slice(&bytes)
                .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
            if normalize_path(state.source_workspace.clone()) != workspace_path {
                continue;
            }

            let title = if state.run_title.is_empty() {
                let plan_path = run_root.join("plan.json");
                if let Ok(plan_bytes) = fs::read(&plan_path) {
                    if let Ok(plan) = serde_json::from_slice::<serde_json::Value>(&plan_bytes) {
                        let goal = plan.get("goal").and_then(|g| g.as_str()).unwrap_or("");
                        if !goal.is_empty() {
                            truncate_str(goal, 50)
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                }
            } else {
                state.run_title.clone()
            };

            runs.push(WorkspaceRunSummary {
                run_root: run_root.clone(),
                run_title: title,
                created_at: state.created_at,
                updated_at: state.updated_at,
                lifecycle: state.lifecycle,
                final_status: state.final_status,
                current_feature_index: state.current_feature_index,
                total_features: state.features.len(),
                awaiting_feature_confirmation: state.awaiting_feature_confirmation,
                active_stage: state.active_stage,
            });
        }

        runs.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        Ok(runs)
    }

    pub fn load_discovery_payload(
        &self,
        workspace_path: impl AsRef<Path>,
    ) -> Result<Option<WorkspaceDiscoveryPayload>> {
        let workspace_path = normalize_path(workspace_path.as_ref().to_path_buf());
        WorkspaceDiscoveryStore::new(workspace_path).load_payload()
    }
}

fn truncate_str(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    let first_line = trimmed.lines().next().unwrap_or(trimmed);
    if first_line.chars().count() <= max_chars {
        first_line.to_string()
    } else {
        let truncated: String = first_line.chars().take(max_chars).collect();
        format!("{truncated}…")
    }
}

fn planner_conversation_prompt(planner_prompt: &str) -> String {
    format!(
        "{DEFAULT_PLANNER_CONVERSATION_PROMPT}\n\nPlanner System Prompt Reference:\n{planner_prompt}\n"
    )
}

async fn refresh_workspace_profile(
    config: &ResolvedConfig,
    worker: &dyn WorkerAdapter,
    workspace_path: &Path,
) -> Result<WorkspaceProfileSelection> {
    let workspace_path = normalize_path(workspace_path.to_path_buf());
    ensure_discovery_assets(config)?;
    let store = WorkspaceDiscoveryStore::new(&workspace_path);
    store.ensure_dirs()?;

    let scan_path = store.scan_path();
    let evidence_path = store.evidence_path();
    let profile_path = store.profile_path();
    let inference_path = store.inference_path();
    let status_path = store.status_path();
    let existing_status = store.load_status()?;
    let previous_workspace_fingerprint = existing_status
        .as_ref()
        .map(|status| status.workspace_fingerprint.clone())
        .unwrap_or_default();
    let previous_profile_fingerprint = existing_status
        .as_ref()
        .and_then(|status| status.profile_fingerprint.clone());
    let previous_last_refreshed_at = existing_status
        .as_ref()
        .and_then(|status| status.last_refreshed_at);

    store.save_status(&workspace_discovery_status(
        &workspace_path,
        &scan_path,
        &evidence_path,
        &profile_path,
        &inference_path,
        previous_workspace_fingerprint,
        previous_profile_fingerprint,
        Utc::now(),
        previous_last_refreshed_at,
        None,
        false,
        WorkspaceDiscoveryPhase::Scanning,
    ))?;

    let scan = scan_workspace(&workspace_path)?;
    store.save_evidence(&scan)?;

    let existing_status = store.load_status()?;
    let existing_profile = store.load_profile()?;
    let existing_inference = store.load_inference()?;
    let needs_refresh = existing_profile.is_none()
        || existing_inference.is_none()
        || existing_status
            .as_ref()
            .map(|status| status.workspace_fingerprint.as_str())
            != Some(scan.workspace_fingerprint.as_str());

    if !needs_refresh {
        let profile = existing_profile.expect("profile checked above");
        let profile_fingerprint = profile_fingerprint(&profile)?;
        let last_refreshed_at = existing_status
            .as_ref()
            .and_then(|status| status.last_refreshed_at)
            .unwrap_or(profile.generated_at);
        store.save_status(&workspace_discovery_status(
            &workspace_path,
            &scan_path,
            &evidence_path,
            &profile_path,
            &inference_path,
            scan.workspace_fingerprint.clone(),
            Some(profile_fingerprint.clone()),
            scan.scanned_at,
            Some(last_refreshed_at),
            None,
            false,
            WorkspaceDiscoveryPhase::ReusingCachedProfile,
        ))?;
        let status = workspace_discovery_status(
            &workspace_path,
            &scan_path,
            &evidence_path,
            &profile_path,
            &inference_path,
            scan.workspace_fingerprint.clone(),
            Some(profile_fingerprint.clone()),
            scan.scanned_at,
            Some(last_refreshed_at),
            None,
            false,
            WorkspaceDiscoveryPhase::Ready,
        );
        store.save_status(&status)?;

        return Ok(WorkspaceProfileSelection {
            profile,
            canonical_profile_path: profile_path,
            scan_path,
            evidence_path,
            inference_path,
            status_path,
            workspace_fingerprint: status.workspace_fingerprint,
            profile_fingerprint,
            last_scanned_at: status.last_scanned_at,
            last_refreshed_at,
            refresh_error: None,
            used_fallback_profile: false,
        });
    }

    let artifacts = store.worker_artifacts();
    let context = DiscoveryContext {
        workspace: workspace_path.clone(),
        discovery_prompt: config.prompts.discovery.clone(),
        workspace_profile_schema: config.schemas.workspace_inference.clone(),
    };
    let request = WorkspaceDiscoveryRequest {
        scan: scan.clone(),
        previous_profile: existing_profile.clone(),
        previous_inference: existing_inference.clone(),
    };
    let polishing_profile_fingerprint = existing_status
        .as_ref()
        .and_then(|status| status.profile_fingerprint.clone());
    let polishing_last_refreshed_at = existing_status
        .as_ref()
        .and_then(|status| status.last_refreshed_at);
    store.reset_worker_artifacts()?;
    store.save_status(&workspace_discovery_status(
        &workspace_path,
        &scan_path,
        &evidence_path,
        &profile_path,
        &inference_path,
        scan.workspace_fingerprint.clone(),
        polishing_profile_fingerprint.clone(),
        scan.scanned_at,
        polishing_last_refreshed_at,
        None,
        false,
        WorkspaceDiscoveryPhase::Polishing,
    ))?;
    let heartbeat_stop = Arc::new(AtomicBool::new(false));
    let heartbeat_task = tokio::spawn({
        let store = store.clone();
        let workspace_path = workspace_path.clone();
        let scan_path = scan_path.clone();
        let evidence_path = evidence_path.clone();
        let profile_path = profile_path.clone();
        let inference_path = inference_path.clone();
        let workspace_fingerprint = scan.workspace_fingerprint.clone();
        let profile_fingerprint = polishing_profile_fingerprint.clone();
        let heartbeat_stop = heartbeat_stop.clone();
        async move {
            let mut ticker = interval(DISCOVERY_PHASE_HEARTBEAT_INTERVAL);
            ticker.tick().await;
            loop {
                if heartbeat_stop.load(Ordering::Relaxed) {
                    break;
                }
                ticker.tick().await;
                if heartbeat_stop.load(Ordering::Relaxed) {
                    break;
                }
                let status = workspace_discovery_status(
                    &workspace_path,
                    &scan_path,
                    &evidence_path,
                    &profile_path,
                    &inference_path,
                    workspace_fingerprint.clone(),
                    profile_fingerprint.clone(),
                    scan.scanned_at,
                    polishing_last_refreshed_at,
                    None,
                    false,
                    WorkspaceDiscoveryPhase::Polishing,
                );
                let _ = store.save_status(&status);
            }
        }
    });

    let refresh_result: Result<WorkspaceProfileSelection> = async {
        let result = worker.discover(&context, &artifacts, &request).await?;
        let bytes = fs::read(&artifacts.output_file)
            .with_context(|| format!("failed to read {}", artifacts.output_file.display()))?;
        let (inference, profile) = parse_discovery_worker_output(&bytes, &scan)
            .with_context(|| format!("failed to parse {}", artifacts.output_file.display()))?;
        inference.validate(&scan)?;
        let profile_fingerprint = profile_fingerprint(&profile)?;
        store.save_inference(&inference)?;
        store.save_profile(&profile)?;
        fs::write(
            &artifacts.result_file,
            serde_json::to_vec_pretty(&result).context("failed to serialize discovery result")?,
        )
        .with_context(|| format!("failed to write {}", artifacts.result_file.display()))?;

        let status = workspace_discovery_status(
            &workspace_path,
            &scan_path,
            &evidence_path,
            &profile_path,
            &inference_path,
            scan.workspace_fingerprint.clone(),
            Some(profile_fingerprint.clone()),
            scan.scanned_at,
            Some(profile.generated_at),
            None,
            false,
            WorkspaceDiscoveryPhase::Ready,
        );
        store.save_status(&status)?;

        Ok(WorkspaceProfileSelection {
            profile,
            canonical_profile_path: profile_path.clone(),
            scan_path: scan_path.clone(),
            evidence_path: evidence_path.clone(),
            inference_path: inference_path.clone(),
            status_path: status_path.clone(),
            workspace_fingerprint: status.workspace_fingerprint,
            profile_fingerprint,
            last_scanned_at: status.last_scanned_at,
            last_refreshed_at: status
                .last_refreshed_at
                .expect("freshly written discovery status should have refresh time"),
            refresh_error: None,
            used_fallback_profile: false,
        })
    }
    .await;

    heartbeat_stop.store(true, Ordering::Relaxed);
    let _ = heartbeat_task.await;

    match refresh_result {
        Ok(selection) => Ok(selection),
        Err(error) => {
            let error_message = error.to_string();
            if let Some(profile) = existing_profile {
                let profile_fingerprint = profile_fingerprint(&profile)?;
                let last_refreshed_at = existing_status
                    .as_ref()
                    .and_then(|status| status.last_refreshed_at)
                    .unwrap_or(profile.generated_at);
                let status = workspace_discovery_status(
                    &workspace_path,
                    &scan_path,
                    &evidence_path,
                    &profile_path,
                    &inference_path,
                    scan.workspace_fingerprint.clone(),
                    Some(profile_fingerprint.clone()),
                    scan.scanned_at,
                    Some(last_refreshed_at),
                    Some(error_message.clone()),
                    true,
                    WorkspaceDiscoveryPhase::UsingFallbackProfile,
                );
                store.save_status(&status)?;

                return Ok(WorkspaceProfileSelection {
                    profile,
                    canonical_profile_path: profile_path,
                    scan_path,
                    evidence_path,
                    inference_path,
                    status_path,
                    workspace_fingerprint: status.workspace_fingerprint,
                    profile_fingerprint,
                    last_scanned_at: status.last_scanned_at,
                    last_refreshed_at,
                    refresh_error: Some(error_message),
                    used_fallback_profile: true,
                });
            }

            let status = workspace_discovery_status(
                &workspace_path,
                &scan_path,
                &evidence_path,
                &profile_path,
                &inference_path,
                scan.workspace_fingerprint,
                None,
                scan.scanned_at,
                polishing_last_refreshed_at,
                Some(error_message),
                false,
                WorkspaceDiscoveryPhase::Failed,
            );
            store.save_status(&status)?;
            Err(error)
        }
    }
}

fn parse_discovery_worker_output(
    bytes: &[u8],
    evidence: &loopsmith_core::discovery::WorkspaceDiscoveryEvidence,
) -> Result<(WorkspaceDiscoveryInference, WorkspaceProfile)> {
    if let Ok(inference) = serde_json::from_slice::<WorkspaceDiscoveryInference>(bytes) {
        let profile = inference.assemble_profile(evidence);
        return Ok((inference, profile));
    }

    let profile: WorkspaceProfile = serde_json::from_slice(bytes)
        .context("worker output matched neither inference nor legacy profile schema")?;
    let inference = WorkspaceDiscoveryInference::from_legacy_profile(&profile, evidence);
    Ok((inference, profile))
}

fn workspace_discovery_status(
    workspace_path: &Path,
    scan_path: &Path,
    evidence_path: &Path,
    profile_path: &Path,
    inference_path: &Path,
    workspace_fingerprint: String,
    profile_fingerprint: Option<String>,
    last_scanned_at: DateTime<Utc>,
    last_refreshed_at: Option<DateTime<Utc>>,
    last_refresh_error: Option<String>,
    used_fallback_profile: bool,
    current_phase: WorkspaceDiscoveryPhase,
) -> WorkspaceDiscoveryStatus {
    WorkspaceDiscoveryStatus {
        workspace_path: workspace_path.to_path_buf(),
        scan_path: scan_path.to_path_buf(),
        evidence_path: evidence_path.to_path_buf(),
        profile_path: profile_path.to_path_buf(),
        inference_path: inference_path.to_path_buf(),
        workspace_fingerprint,
        profile_fingerprint,
        last_scanned_at,
        last_refreshed_at,
        last_refresh_error,
        used_fallback_profile,
        current_phase,
        phase_heartbeat_at: Some(Utc::now()),
    }
}

fn ensure_discovery_assets(config: &ResolvedConfig) -> Result<()> {
    ensure_text_file(&config.prompts.discovery, DEFAULT_DISCOVERY_PROMPT)?;
    ensure_text_file(
        &config.schemas.workspace_profile,
        DEFAULT_WORKSPACE_PROFILE_SCHEMA,
    )?;
    ensure_text_file(
        &config.schemas.workspace_inference,
        DEFAULT_WORKSPACE_INFERENCE_SCHEMA,
    )?;
    Ok(())
}

fn ensure_text_file(path: &Path, contents: &str) -> Result<()> {
    if path.exists() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }
    fs::write(path, contents).with_context(|| format!("failed to write {}", path.display()))
}

fn resolve_effective_prompts(
    config: &ResolvedConfig,
    overrides: &PromptOverrides,
) -> Result<PromptSnapshot> {
    let planner = overrides
        .planner
        .clone()
        .map_or_else(|| read_prompt_file(&config.prompts.planner), Ok)?;
    let builder = overrides
        .builder
        .clone()
        .map_or_else(|| read_prompt_file(&config.prompts.builder), Ok)?;
    let evaluator = overrides
        .evaluator
        .clone()
        .map_or_else(|| read_prompt_file(&config.prompts.evaluator), Ok)?;

    for (name, value) in [
        ("planner", planner.as_str()),
        ("builder", builder.as_str()),
        ("evaluator", evaluator.as_str()),
    ] {
        if value.trim().is_empty() {
            bail!("{name} prompt must not be empty");
        }
    }

    Ok(PromptSnapshot {
        planner,
        builder,
        evaluator,
    })
}

fn read_prompt_file(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("failed to read prompt {}", path.display()))
}

async fn with_selected_worker<F, Fut>(config: ResolvedConfig, f: F) -> Result<RunState>
where
    F: FnOnce(loopsmith_core::controller::HarnessController<Box<dyn WorkerAdapter>>) -> Fut,
    Fut: std::future::Future<Output = Result<RunState>>,
{
    let artifact_store = FileArtifactStore::new(config.storage.runs_dir.clone());
    let worker = build_configured_worker(&config)?;
    let controller =
        loopsmith_core::controller::HarnessController::new(config, artifact_store, worker);
    f(controller).await
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::{Arc, Mutex, mpsc},
        time::Duration,
    };

    use anyhow::{Result, anyhow};
    use async_trait::async_trait;
    use chrono::Utc;
    use tempfile::tempdir;
    use uuid::Uuid;

    use loopsmith_core::{
        artifacts::{FeatureLayout, StageArtifactSet},
        config::{
            EvaluatorConfig, ResolvedConfig, ResolvedPromptConfig, ResolvedSchemaConfig,
            ResolvedStorageConfig, RuntimeConfig, RuntimeSupervisionConfig, WorkerConfig,
            WorkerSelection, WorkspaceConfig,
        },
        discovery::{
            DiscoveryArtifactSet, WorkspaceDiscoveryPhase, WorkspaceDiscoveryRequest,
            WorkspaceDiscoveryStore, WorkspaceProfile,
        },
        domain::{
            BuilderHandoff, EvaluationRequest, FeatureContract, PlannerConversationRequest,
            PromptOverrides, QaReport, QaStatus, RunLifecycleStatus, RunState, WorkerResult,
            WorkerStatus,
        },
        worker::{
            DiscoveryContext, DiscoveryWorkerResult, PlannerConversationArtifactSet,
            PlannerConversationContext, PlannerConversationWorkerResult, WorkerAdapter,
            WorkerContext,
        },
        workspace::WorkspaceIsolation,
    };

    use super::{
        HarnessUiService, LaunchDraft, PlannerConversationDraft, refresh_workspace_profile,
    };

    #[derive(Default)]
    struct FakeDiscoveryState {
        discover_calls: usize,
        fail_message: Option<String>,
    }

    struct FakeDiscoveryWorker {
        state: Arc<Mutex<FakeDiscoveryState>>,
    }

    fn write_fake_discovery_output(
        artifacts: &DiscoveryArtifactSet,
        profile: &WorkspaceProfile,
        discover_calls: usize,
    ) -> Result<()> {
        fs::write(&artifacts.prompt_file, "fake discovery prompt\n").expect("write prompt");
        fs::write(&artifacts.stdout_log, "fake discovery stdout\n").expect("write stdout");
        fs::write(&artifacts.stderr_log, "").expect("write stderr");

        let mut profile = profile.clone();
        profile.summary = format!("{} [refresh={}]", profile.summary, discover_calls);
        fs::write(
            &artifacts.output_file,
            serde_json::to_vec_pretty(&profile).expect("serialize profile"),
        )
        .expect("write output");
        Ok(())
    }

    #[async_trait]
    impl WorkerAdapter for FakeDiscoveryWorker {
        async fn discover(
            &self,
            _context: &DiscoveryContext,
            artifacts: &DiscoveryArtifactSet,
            request: &WorkspaceDiscoveryRequest,
        ) -> Result<DiscoveryWorkerResult> {
            let mut state = self.state.lock().expect("lock");
            state.discover_calls += 1;
            if let Some(message) = state.fail_message.clone() {
                return Err(anyhow!(message));
            }

            let profile = request.synthesize_profile();
            write_fake_discovery_output(artifacts, &profile, state.discover_calls)?;

            Ok(DiscoveryWorkerResult {
                status: WorkerStatus::Prepared,
                command: vec!["fake".to_string(), "discover".to_string()],
                prompt_file: artifacts.prompt_file.clone(),
                output_file: artifacts.output_file.clone(),
                stdout_log: artifacts.stdout_log.clone(),
                stderr_log: artifacts.stderr_log.clone(),
                notes: Vec::new(),
                session_id: Some(format!("fake-discover-{:02}", state.discover_calls)),
            })
        }

        async fn plan(
            &self,
            _context: &WorkerContext,
            _artifacts: &StageArtifactSet,
            _request: &loopsmith_core::domain::PlanningRequest,
        ) -> Result<WorkerResult> {
            Err(anyhow!("unexpected plan call in discovery test"))
        }

        async fn consult_planner(
            &self,
            _context: &PlannerConversationContext,
            _artifacts: &PlannerConversationArtifactSet,
            _request: &PlannerConversationRequest,
        ) -> Result<PlannerConversationWorkerResult> {
            Err(anyhow!("unexpected planner consult call in discovery test"))
        }

        async fn build(
            &self,
            _context: &WorkerContext,
            _feature: &FeatureLayout,
            _artifacts: &StageArtifactSet,
            _contract: &FeatureContract,
        ) -> Result<WorkerResult> {
            Err(anyhow!("unexpected build call in discovery test"))
        }

        async fn evaluate(
            &self,
            _context: &WorkerContext,
            _feature: &FeatureLayout,
            _artifacts: &StageArtifactSet,
            _request: &EvaluationRequest,
        ) -> Result<WorkerResult> {
            Err(anyhow!("unexpected evaluate call in discovery test"))
        }

        async fn repair(
            &self,
            _context: &WorkerContext,
            _feature: &FeatureLayout,
            _artifacts: &StageArtifactSet,
            _contract: &FeatureContract,
            _builder_handoff: &BuilderHandoff,
            _qa_report: &QaReport,
            _previous_session_id: Option<&str>,
        ) -> Result<WorkerResult> {
            Err(anyhow!("unexpected repair call in discovery test"))
        }
    }

    struct DelayedDiscoveryWorker {
        state: Arc<Mutex<FakeDiscoveryState>>,
        entered: mpsc::Sender<()>,
        pause: Duration,
    }

    #[async_trait]
    impl WorkerAdapter for DelayedDiscoveryWorker {
        async fn discover(
            &self,
            _context: &DiscoveryContext,
            artifacts: &DiscoveryArtifactSet,
            request: &WorkspaceDiscoveryRequest,
        ) -> Result<DiscoveryWorkerResult> {
            let discover_calls = {
                let mut state = self.state.lock().expect("lock");
                state.discover_calls += 1;
                state.discover_calls
            };
            self.entered.send(()).expect("notify discovery start");
            tokio::time::sleep(self.pause).await;

            let profile = request.synthesize_profile();
            write_fake_discovery_output(artifacts, &profile, discover_calls)?;

            Ok(DiscoveryWorkerResult {
                status: WorkerStatus::Prepared,
                command: vec!["fake".to_string(), "discover".to_string()],
                prompt_file: artifacts.prompt_file.clone(),
                output_file: artifacts.output_file.clone(),
                stdout_log: artifacts.stdout_log.clone(),
                stderr_log: artifacts.stderr_log.clone(),
                notes: Vec::new(),
                session_id: Some(format!("fake-discover-{discover_calls:02}")),
            })
        }

        async fn plan(
            &self,
            _context: &WorkerContext,
            _artifacts: &StageArtifactSet,
            _request: &loopsmith_core::domain::PlanningRequest,
        ) -> Result<WorkerResult> {
            Err(anyhow!("unexpected plan call in discovery test"))
        }

        async fn consult_planner(
            &self,
            _context: &PlannerConversationContext,
            _artifacts: &PlannerConversationArtifactSet,
            _request: &PlannerConversationRequest,
        ) -> Result<PlannerConversationWorkerResult> {
            Err(anyhow!("unexpected planner consult call in discovery test"))
        }

        async fn build(
            &self,
            _context: &WorkerContext,
            _feature: &FeatureLayout,
            _artifacts: &StageArtifactSet,
            _contract: &FeatureContract,
        ) -> Result<WorkerResult> {
            Err(anyhow!("unexpected build call in discovery test"))
        }

        async fn evaluate(
            &self,
            _context: &WorkerContext,
            _feature: &FeatureLayout,
            _artifacts: &StageArtifactSet,
            _request: &EvaluationRequest,
        ) -> Result<WorkerResult> {
            Err(anyhow!("unexpected evaluate call in discovery test"))
        }

        async fn repair(
            &self,
            _context: &WorkerContext,
            _feature: &FeatureLayout,
            _artifacts: &StageArtifactSet,
            _contract: &FeatureContract,
            _builder_handoff: &BuilderHandoff,
            _qa_report: &QaReport,
            _previous_session_id: Option<&str>,
        ) -> Result<WorkerResult> {
            Err(anyhow!("unexpected repair call in discovery test"))
        }
    }

    struct MissingDiscoveryOutputWorker {
        state: Arc<Mutex<FakeDiscoveryState>>,
    }

    #[async_trait]
    impl WorkerAdapter for MissingDiscoveryOutputWorker {
        async fn discover(
            &self,
            _context: &DiscoveryContext,
            artifacts: &DiscoveryArtifactSet,
            _request: &WorkspaceDiscoveryRequest,
        ) -> Result<DiscoveryWorkerResult> {
            let mut state = self.state.lock().expect("lock");
            state.discover_calls += 1;
            fs::write(&artifacts.prompt_file, "fake discovery prompt\n").expect("write prompt");
            fs::write(&artifacts.stdout_log, "fake discovery stdout\n").expect("write stdout");
            fs::write(&artifacts.stderr_log, "").expect("write stderr");

            Ok(DiscoveryWorkerResult {
                status: WorkerStatus::Prepared,
                command: vec!["fake".to_string(), "discover".to_string()],
                prompt_file: artifacts.prompt_file.clone(),
                output_file: artifacts.output_file.clone(),
                stdout_log: artifacts.stdout_log.clone(),
                stderr_log: artifacts.stderr_log.clone(),
                notes: Vec::new(),
                session_id: Some(format!("fake-discover-missing-{:02}", state.discover_calls)),
            })
        }

        async fn plan(
            &self,
            _context: &WorkerContext,
            _artifacts: &StageArtifactSet,
            _request: &loopsmith_core::domain::PlanningRequest,
        ) -> Result<WorkerResult> {
            Err(anyhow!("unexpected plan call in discovery test"))
        }

        async fn consult_planner(
            &self,
            _context: &PlannerConversationContext,
            _artifacts: &PlannerConversationArtifactSet,
            _request: &PlannerConversationRequest,
        ) -> Result<PlannerConversationWorkerResult> {
            Err(anyhow!("unexpected planner consult call in discovery test"))
        }

        async fn build(
            &self,
            _context: &WorkerContext,
            _feature: &FeatureLayout,
            _artifacts: &StageArtifactSet,
            _contract: &FeatureContract,
        ) -> Result<WorkerResult> {
            Err(anyhow!("unexpected build call in discovery test"))
        }

        async fn evaluate(
            &self,
            _context: &WorkerContext,
            _feature: &FeatureLayout,
            _artifacts: &StageArtifactSet,
            _request: &EvaluationRequest,
        ) -> Result<WorkerResult> {
            Err(anyhow!("unexpected evaluate call in discovery test"))
        }

        async fn repair(
            &self,
            _context: &WorkerContext,
            _feature: &FeatureLayout,
            _artifacts: &StageArtifactSet,
            _contract: &FeatureContract,
            _builder_handoff: &BuilderHandoff,
            _qa_report: &QaReport,
            _previous_session_id: Option<&str>,
        ) -> Result<WorkerResult> {
            Err(anyhow!("unexpected repair call in discovery test"))
        }
    }

    #[test]
    fn validate_workspace_path_accepts_hidden_directories() {
        let temp = tempdir().expect("tempdir");
        let hidden_workspace = temp.path().join(".workspace");
        fs::create_dir_all(&hidden_workspace).expect("hidden workspace");

        let service = HarnessUiService;
        let validated = service
            .validate_workspace_path(&hidden_workspace)
            .expect("validate hidden workspace");
        assert_eq!(validated, hidden_workspace);
    }

    #[test]
    fn prepare_launch_applies_prompt_overrides() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path();
        let config_dir = project_root.join("config");
        let prompts_dir = project_root.join("prompts");
        let schemas_dir = project_root.join("schemas");
        fs::create_dir_all(&config_dir).expect("config dir");
        fs::create_dir_all(&prompts_dir).expect("prompts dir");
        fs::create_dir_all(&schemas_dir).expect("schemas dir");
        fs::write(prompts_dir.join("discovery.md"), "discovery\n").expect("discovery");
        fs::write(prompts_dir.join("planner.md"), "planner\n").expect("planner");
        fs::write(prompts_dir.join("builder.md"), "builder\n").expect("builder");
        fs::write(prompts_dir.join("evaluator.md"), "evaluator\n").expect("evaluator");
        fs::write(schemas_dir.join("workspace-profile.json"), "{}\n").expect("schema");
        fs::write(schemas_dir.join("planner-output.json"), "{}\n").expect("schema");
        fs::write(schemas_dir.join("builder-handoff.json"), "{}\n").expect("schema");
        fs::write(schemas_dir.join("qa-report.json"), "{}\n").expect("schema");
        let workspace = project_root.join("workspace");
        fs::create_dir_all(&workspace).expect("workspace dir");
        let config_path = config_dir.join("ui.toml");
        fs::write(
            &config_path,
            r#"
[project]
root_dir = ".."

[storage]
runs_dir = ".loopsmith-runs"

[workspace]
isolation = "direct"

[worker]
kind = "simulated"

[worker.simulation]
evaluator_statuses = ["pass"]
session_prefix = "sim"

[prompts]
discovery = "prompts/discovery.md"
planner = "prompts/planner.md"
builder = "prompts/builder.md"
evaluator = "prompts/evaluator.md"

[schemas]
workspace_profile = "schemas/workspace-profile.json"
planner_output = "schemas/planner-output.json"
builder_handoff = "schemas/builder-handoff.json"
qa_report = "schemas/qa-report.json"

[runtime]
feature_limit = 1
max_repair_attempts = 1
confirm_before_build = true
services = []
stacks = []

[evaluator]
dimensions = ["correctness"]
require_screenshots = false
commands = []
"#,
        )
        .expect("write config");

        let service = HarnessUiService;
        let prepared = service
            .prepare_launch(LaunchDraft {
                workspace_path: workspace,
                config_path,
                request_draft: "Ship the dashboard".to_string(),
                prompt_overrides: PromptOverrides {
                    planner: None,
                    builder: Some("custom builder\n".to_string()),
                    evaluator: None,
                },
                feature_limit: Some(2),
            })
            .expect("prepare launch");

        assert_eq!(prepared.prompt_snapshot.planner, "planner\n");
        assert_eq!(prepared.prompt_snapshot.builder, "custom builder\n");
        assert_eq!(prepared.prompt_snapshot.evaluator, "evaluator\n");
        assert!(prepared.run_request.confirm_before_build);
    }

    #[test]
    fn list_runs_for_workspace_filters_and_sorts_runs() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path();
        let config_dir = project_root.join("config");
        let prompts_dir = project_root.join("prompts");
        let schemas_dir = project_root.join("schemas");
        let runs_dir = project_root.join(".loopsmith-runs");
        let workspace = project_root.join("workspace");
        let other_workspace = project_root.join("workspace-other");
        fs::create_dir_all(&config_dir).expect("config dir");
        fs::create_dir_all(&prompts_dir).expect("prompts dir");
        fs::create_dir_all(&schemas_dir).expect("schemas dir");
        fs::create_dir_all(&runs_dir).expect("runs dir");
        fs::create_dir_all(&workspace).expect("workspace dir");
        fs::create_dir_all(&other_workspace).expect("other workspace dir");
        fs::write(prompts_dir.join("discovery.md"), "discovery\n").expect("discovery");
        fs::write(prompts_dir.join("planner.md"), "planner\n").expect("planner");
        fs::write(prompts_dir.join("builder.md"), "builder\n").expect("builder");
        fs::write(prompts_dir.join("evaluator.md"), "evaluator\n").expect("evaluator");
        fs::write(schemas_dir.join("workspace-profile.json"), "{}\n").expect("schema");
        fs::write(schemas_dir.join("planner-output.json"), "{}\n").expect("schema");
        fs::write(schemas_dir.join("builder-handoff.json"), "{}\n").expect("schema");
        fs::write(schemas_dir.join("qa-report.json"), "{}\n").expect("schema");
        let config_path = config_dir.join("ui.toml");
        fs::write(
            &config_path,
            r#"
[project]
root_dir = ".."

[storage]
runs_dir = ".loopsmith-runs"

[workspace]
isolation = "direct"

[worker]
kind = "simulated"

[worker.simulation]
evaluator_statuses = ["pass"]
session_prefix = "sim"

[prompts]
discovery = "prompts/discovery.md"
planner = "prompts/planner.md"
builder = "prompts/builder.md"
evaluator = "prompts/evaluator.md"

[schemas]
workspace_profile = "schemas/workspace-profile.json"
planner_output = "schemas/planner-output.json"
builder_handoff = "schemas/builder-handoff.json"
qa_report = "schemas/qa-report.json"

[runtime]
feature_limit = 1
max_repair_attempts = 1
services = []
stacks = []

[evaluator]
dimensions = ["correctness"]
require_screenshots = false
commands = []
"#,
        )
        .expect("write config");

        for (index, source_workspace) in [workspace.clone(), other_workspace, workspace.clone()]
            .into_iter()
            .enumerate()
        {
            let run_root = runs_dir.join(format!("run-{index}"));
            fs::create_dir_all(&run_root).expect("run root");
            let state = RunState {
                run_id: Uuid::new_v4(),
                run_title: format!("Test run {index}"),
                created_at: Utc::now(),
                updated_at: Utc::now() + chrono::TimeDelta::seconds(index as i64),
                run_root: run_root.clone(),
                state_file: run_root.join("run-state.json"),
                manifest_file: run_root.join("manifest.json"),
                launch_file: Some(run_root.join("launch.json")),
                request_file: run_root.join("request.md"),
                plan_file: run_root.join("plan.json"),
                runtime_plan_file: run_root.join("runtime-plan.json"),
                source_workspace,
                execution_workspace: run_root.join("exec"),
                lifecycle: RunLifecycleStatus::Running,
                final_status: None,
                current_feature_index: index,
                awaiting_feature_confirmation: false,
                active_stage: None,
                plan_stage: None,
                features: Vec::new(),
            };
            let bytes = serde_json::to_vec_pretty(&state).expect("serialize state");
            fs::write(run_root.join("manifest.json"), bytes).expect("write manifest");
        }

        let service = HarnessUiService;
        let runs = service
            .list_runs_for_workspace(&config_path, &workspace)
            .expect("list runs");
        assert_eq!(runs.len(), 2);
        assert!(runs[0].updated_at >= runs[1].updated_at);
        assert_eq!(
            runs[0].run_root.file_name().and_then(|name| name.to_str()),
            Some("run-2")
        );
    }

    #[tokio::test]
    async fn consult_planner_returns_structured_response() -> Result<()> {
        let temp = tempdir()?;
        let project_root = temp.path();
        let config_dir = project_root.join("config");
        let prompts_dir = project_root.join("prompts");
        let schemas_dir = project_root.join("schemas");
        fs::create_dir_all(&config_dir)?;
        fs::create_dir_all(&prompts_dir)?;
        fs::create_dir_all(&schemas_dir)?;
        fs::write(prompts_dir.join("discovery.md"), "discovery\n")?;
        fs::write(prompts_dir.join("planner.md"), "planner\n")?;
        fs::write(prompts_dir.join("builder.md"), "builder\n")?;
        fs::write(prompts_dir.join("evaluator.md"), "evaluator\n")?;
        fs::write(schemas_dir.join("workspace-profile.json"), "{}\n")?;
        fs::write(schemas_dir.join("planner-output.json"), "{}\n")?;
        fs::write(schemas_dir.join("builder-handoff.json"), "{}\n")?;
        fs::write(schemas_dir.join("qa-report.json"), "{}\n")?;
        let config_path = config_dir.join("ui.toml");
        fs::write(
            &config_path,
            r#"
[project]
root_dir = ".."

[storage]
runs_dir = ".loopsmith-runs"

[workspace]
isolation = "direct"

[worker]
kind = "simulated"

[worker.simulation]
evaluator_statuses = ["pass"]
session_prefix = "sim"

[prompts]
discovery = "prompts/discovery.md"
planner = "prompts/planner.md"
builder = "prompts/builder.md"
evaluator = "prompts/evaluator.md"

[schemas]
workspace_profile = "schemas/workspace-profile.json"
planner_output = "schemas/planner-output.json"
builder_handoff = "schemas/builder-handoff.json"
qa_report = "schemas/qa-report.json"

[runtime]
feature_limit = 2
max_repair_attempts = 1
services = []
stacks = []

[evaluator]
dimensions = ["correctness"]
require_screenshots = false
commands = []
"#,
        )?;
        let workspace = discovery_test_workspace(project_root)?;

        let service = HarnessUiService;
        let response = service
            .consult_planner(PlannerConversationDraft {
                workspace_path: workspace,
                config_path,
                request_draft:
                    "Add a planner chat box and pause before build until the plan is confirmed."
                        .to_string(),
                prompt_overrides: PromptOverrides::default(),
                feature_limit: Some(2),
                conversation: vec![loopsmith_core::domain::PlannerConversationTurn {
                    role: loopsmith_core::domain::PlannerConversationRole::User,
                    content: "What should I confirm before build starts?".to_string(),
                }],
            })
            .await?;

        assert!(!response.reply_markdown.trim().is_empty());
        assert_eq!(
            response.readiness.as_str(),
            loopsmith_core::domain::PlannerConversationReadiness::ReadyToBuild.as_str()
        );
        assert!(!response.suggested_features.is_empty());
        assert!(!response.confirmation_points.is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn refresh_workspace_profile_persists_initial_profile() -> Result<()> {
        let temp = tempdir()?;
        let config = discovery_test_config(temp.path());
        let workspace = discovery_test_workspace(temp.path())?;
        let state = Arc::new(Mutex::new(FakeDiscoveryState::default()));
        let worker = FakeDiscoveryWorker {
            state: state.clone(),
        };

        let selection = refresh_workspace_profile(&config, &worker, &workspace).await?;
        let store = WorkspaceDiscoveryStore::new(&workspace);
        let payload = store.load_payload()?.expect("payload");

        assert_eq!(state.lock().expect("lock").discover_calls, 1);
        assert_eq!(selection.canonical_profile_path, store.profile_path());
        assert!(store.scan_path().exists());
        assert!(store.profile_path().exists());
        assert!(store.status_path().exists());
        assert!(config.prompts.discovery.exists());
        assert!(config.schemas.workspace_profile.exists());
        assert!(payload.profile_summary.is_some());
        assert!(payload.overview.source_file_count > 0);
        assert!(!payload.overview.key_concepts.is_empty());
        assert!(payload.status.last_refresh_error.is_none());
        assert!(!payload.status.used_fallback_profile);
        assert_eq!(payload.status.current_phase, WorkspaceDiscoveryPhase::Ready);

        Ok(())
    }

    #[tokio::test]
    async fn refresh_workspace_profile_reuses_existing_profile_when_fingerprint_is_unchanged()
    -> Result<()> {
        let temp = tempdir()?;
        let config = discovery_test_config(temp.path());
        let workspace = discovery_test_workspace(temp.path())?;
        let state = Arc::new(Mutex::new(FakeDiscoveryState::default()));
        let worker = FakeDiscoveryWorker {
            state: state.clone(),
        };

        let first = refresh_workspace_profile(&config, &worker, &workspace).await?;
        let second = refresh_workspace_profile(&config, &worker, &workspace).await?;

        assert_eq!(state.lock().expect("lock").discover_calls, 1);
        assert_eq!(first.profile.summary, second.profile.summary);
        assert_eq!(first.profile_fingerprint, second.profile_fingerprint);
        assert!(!second.used_fallback_profile);
        assert!(second.refresh_error.is_none());
        assert_eq!(
            WorkspaceDiscoveryStore::new(&workspace)
                .load_status()?
                .expect("status")
                .current_phase,
            WorkspaceDiscoveryPhase::Ready
        );

        Ok(())
    }

    #[tokio::test]
    async fn refresh_workspace_profile_refreshes_after_workspace_changes() -> Result<()> {
        let temp = tempdir()?;
        let config = discovery_test_config(temp.path());
        let workspace = discovery_test_workspace(temp.path())?;
        let state = Arc::new(Mutex::new(FakeDiscoveryState::default()));
        let worker = FakeDiscoveryWorker {
            state: state.clone(),
        };

        let first = refresh_workspace_profile(&config, &worker, &workspace).await?;
        fs::write(
            workspace.join("package.json"),
            r#"{"name":"discovery-fixture","scripts":{"test":"vitest"},"dependencies":{"react":"18.3.0"}}"#,
        )?;
        let second = refresh_workspace_profile(&config, &worker, &workspace).await?;

        assert_eq!(state.lock().expect("lock").discover_calls, 2);
        assert_ne!(first.workspace_fingerprint, second.workspace_fingerprint);
        assert_ne!(first.profile_fingerprint, second.profile_fingerprint);
        assert!(second.profile.summary.contains("[refresh=2]"));
        assert_eq!(
            WorkspaceDiscoveryStore::new(&workspace)
                .load_status()?
                .expect("status")
                .current_phase,
            WorkspaceDiscoveryPhase::Ready
        );

        Ok(())
    }

    #[tokio::test]
    async fn refresh_workspace_profile_persists_polishing_phase_while_worker_is_running()
    -> Result<()> {
        let temp = tempdir()?;
        let config = discovery_test_config(temp.path());
        let workspace = discovery_test_workspace(temp.path())?;
        let state = Arc::new(Mutex::new(FakeDiscoveryState::default()));
        let (entered_tx, entered_rx) = mpsc::channel();
        let store = WorkspaceDiscoveryStore::new(&workspace);

        let refresh = tokio::spawn({
            let config = config.clone();
            let workspace = workspace.clone();
            let state = state.clone();
            async move {
                let worker = DelayedDiscoveryWorker {
                    state,
                    entered: entered_tx,
                    pause: Duration::from_millis(250),
                };
                refresh_workspace_profile(&config, &worker, &workspace).await
            }
        });

        tokio::task::spawn_blocking(move || entered_rx.recv_timeout(Duration::from_secs(1)))
            .await
            .expect("join entered wait")
            .expect("wait for discovery worker");
        let status = store.load_status()?.expect("status");
        assert_eq!(status.current_phase, WorkspaceDiscoveryPhase::Polishing);

        let selection = refresh.await.expect("task join")?;
        assert!(selection.refresh_error.is_none());
        assert_eq!(
            store.load_status()?.expect("status").current_phase,
            WorkspaceDiscoveryPhase::Ready
        );

        Ok(())
    }

    #[tokio::test]
    async fn refresh_workspace_profile_falls_back_to_existing_profile_after_refresh_failure()
    -> Result<()> {
        let temp = tempdir()?;
        let config = discovery_test_config(temp.path());
        let workspace = discovery_test_workspace(temp.path())?;
        let state = Arc::new(Mutex::new(FakeDiscoveryState::default()));
        let worker = FakeDiscoveryWorker {
            state: state.clone(),
        };

        let first = refresh_workspace_profile(&config, &worker, &workspace).await?;
        fs::write(
            workspace.join("Makefile"),
            "test:\n\tcargo test\nbuild:\n\tcargo build\n",
        )?;
        state.lock().expect("lock").fail_message = Some("discovery worker failed".to_string());

        let fallback = refresh_workspace_profile(&config, &worker, &workspace).await?;
        let status = WorkspaceDiscoveryStore::new(&workspace)
            .load_status()?
            .expect("status");

        assert_eq!(state.lock().expect("lock").discover_calls, 2);
        assert!(fallback.used_fallback_profile);
        assert_eq!(fallback.profile.summary, first.profile.summary);
        assert_eq!(
            fallback.refresh_error.as_deref(),
            Some("discovery worker failed")
        );
        assert_eq!(
            status.last_refresh_error.as_deref(),
            Some("discovery worker failed")
        );
        assert!(status.used_fallback_profile);
        assert_eq!(
            status.current_phase,
            WorkspaceDiscoveryPhase::UsingFallbackProfile
        );

        Ok(())
    }

    #[tokio::test]
    async fn refresh_workspace_profile_falls_back_when_worker_returns_without_output() -> Result<()>
    {
        let temp = tempdir()?;
        let config = discovery_test_config(temp.path());
        let workspace = discovery_test_workspace(temp.path())?;
        let initial_state = Arc::new(Mutex::new(FakeDiscoveryState::default()));
        let initial_worker = FakeDiscoveryWorker {
            state: initial_state,
        };

        let first = refresh_workspace_profile(&config, &initial_worker, &workspace).await?;
        fs::write(
            workspace.join("Makefile"),
            "build:\n\tcargo build\n\ntest:\n\tcargo test\n",
        )?;
        let fallback_state = Arc::new(Mutex::new(FakeDiscoveryState::default()));
        let fallback_worker = MissingDiscoveryOutputWorker {
            state: fallback_state.clone(),
        };

        let fallback = refresh_workspace_profile(&config, &fallback_worker, &workspace).await?;
        let store = WorkspaceDiscoveryStore::new(&workspace);
        let status = store.load_status()?.expect("status");
        let worker_artifacts = store.worker_artifacts();

        assert_eq!(fallback_state.lock().expect("lock").discover_calls, 1);
        assert!(fallback.used_fallback_profile);
        assert_eq!(fallback.profile.summary, first.profile.summary);
        assert!(
            fallback
                .refresh_error
                .as_deref()
                .unwrap_or_default()
                .contains("failed to read")
        );
        assert_eq!(
            status.current_phase,
            WorkspaceDiscoveryPhase::UsingFallbackProfile
        );
        assert!(status.used_fallback_profile);
        assert!(
            status
                .last_refresh_error
                .as_deref()
                .unwrap_or_default()
                .contains("failed to read")
        );
        assert!(!worker_artifacts.result_file.exists());

        Ok(())
    }

    #[tokio::test]
    async fn refresh_workspace_profile_blocks_when_no_profile_exists_and_refresh_fails()
    -> Result<()> {
        let temp = tempdir()?;
        let config = discovery_test_config(temp.path());
        let workspace = discovery_test_workspace(temp.path())?;
        let state = Arc::new(Mutex::new(FakeDiscoveryState {
            discover_calls: 0,
            fail_message: Some("discovery worker failed".to_string()),
        }));
        let worker = FakeDiscoveryWorker {
            state: state.clone(),
        };

        let err = refresh_workspace_profile(&config, &worker, &workspace)
            .await
            .expect_err("refresh should fail");
        let store = WorkspaceDiscoveryStore::new(&workspace);
        let status = store.load_status()?.expect("status");

        assert_eq!(state.lock().expect("lock").discover_calls, 1);
        assert!(store.load_profile()?.is_none());
        assert_eq!(err.to_string(), "discovery worker failed");
        assert_eq!(
            status.last_refresh_error.as_deref(),
            Some("discovery worker failed")
        );
        assert!(!status.used_fallback_profile);
        assert_eq!(status.current_phase, WorkspaceDiscoveryPhase::Failed);

        Ok(())
    }

    fn discovery_test_config(project_root: &std::path::Path) -> ResolvedConfig {
        let prompts_dir = project_root.join("prompts");
        let schemas_dir = project_root.join("schemas");
        fs::create_dir_all(&prompts_dir).expect("prompts dir");
        fs::create_dir_all(&schemas_dir).expect("schemas dir");

        ResolvedConfig {
            project_root: project_root.to_path_buf(),
            storage: ResolvedStorageConfig {
                runs_dir: project_root.join(".loopsmith-runs"),
            },
            workspace: WorkspaceConfig {
                isolation: WorkspaceIsolation::Direct,
            },
            worker: WorkerConfig {
                selection: WorkerSelection::Simulated {
                    simulation: loopsmith_core::config::SimulationWorkerConfig {
                        evaluator_statuses: vec![QaStatus::Pass],
                        session_prefix: "sim".to_string(),
                    },
                },
                planner: None,
            },
            prompts: ResolvedPromptConfig {
                discovery: prompts_dir.join("discovery.md"),
                planner: prompts_dir.join("planner.md"),
                builder: prompts_dir.join("builder.md"),
                evaluator: prompts_dir.join("evaluator.md"),
            },
            schemas: ResolvedSchemaConfig {
                workspace_profile: schemas_dir.join("workspace-profile.json"),
                workspace_inference: schemas_dir.join("workspace-inference.json"),
                planner_output: schemas_dir.join("planner-output.json"),
                builder_handoff: schemas_dir.join("builder-handoff.json"),
                qa_report: schemas_dir.join("qa-report.json"),
            },
            runtime: RuntimeConfig {
                feature_limit: 1,
                max_repair_attempts: 1,
                continue_after_failure: false,
                confirm_before_build: false,
                supervision: RuntimeSupervisionConfig::default(),
                services: Vec::new(),
                stacks: Vec::new(),
            },
            evaluator: EvaluatorConfig {
                dimensions: vec!["correctness".to_string()],
                require_screenshots: false,
                commands: vec![vec!["/usr/bin/env".to_string(), "true".to_string()]],
                screenshots: Vec::new(),
            },
        }
    }

    fn discovery_test_workspace(project_root: &std::path::Path) -> Result<std::path::PathBuf> {
        let workspace = project_root.join("workspace");
        fs::create_dir_all(workspace.join("src"))?;
        fs::write(
            workspace.join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n[dependencies]\nserde = \"1\"\n",
        )?;
        fs::write(workspace.join("src/lib.rs"), "pub fn fixture() {}\n")?;
        fs::write(
            workspace.join(".editorconfig"),
            "root = true\n[*]\nindent_style = space\n",
        )?;
        Ok(workspace)
    }
}
