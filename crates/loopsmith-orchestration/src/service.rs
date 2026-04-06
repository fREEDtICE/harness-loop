use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use loopsmith_core::{
    artifacts::FileArtifactStore,
    config::{AppConfig, ResolvedConfig},
    discovery::{
        WorkspaceDiscoveryPayload, WorkspaceDiscoveryPhase, WorkspaceDiscoveryRequest,
        WorkspaceDiscoveryStatus, WorkspaceDiscoveryStore, WorkspaceProfile,
        WorkspaceProfileSelection, profile_fingerprint, scan_workspace,
    },
    domain::{PromptOverrides, PromptSnapshot, QaStatus, RunLifecycleStatus, RunRequest, RunState},
    paths::normalize_path,
    worker::{DiscoveryContext, WorkerAdapter},
};
use loopsmith_worker_factory::{build_configured_worker, build_discovery_worker};
use serde::{Deserialize, Serialize};

const DEFAULT_DISCOVERY_PROMPT: &str = include_str!("../../../prompts/discovery.md");
const DEFAULT_WORKSPACE_PROFILE_SCHEMA: &str =
    include_str!("../../../schemas/workspace-profile.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchDraft {
    pub workspace_path: PathBuf,
    pub config_path: PathBuf,
    pub request_draft: String,
    pub prompt_overrides: PromptOverrides,
    pub feature_limit: Option<usize>,
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

        Ok(PreparedLaunch {
            config_path: config_path.clone(),
            prompt_snapshot,
            resolved_config,
            run_request: RunRequest {
                user_request: draft.request_draft,
                source_workspace: workspace_path,
                feature_limit: draft.feature_limit,
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
    let profile_path = store.profile_path();
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
        &profile_path,
        previous_workspace_fingerprint,
        previous_profile_fingerprint,
        Utc::now(),
        previous_last_refreshed_at,
        None,
        false,
        WorkspaceDiscoveryPhase::Scanning,
    ))?;

    let scan = scan_workspace(&workspace_path)?;
    store.save_scan(&scan)?;

    let existing_status = store.load_status()?;
    let existing_profile = store.load_profile()?;
    let needs_refresh = existing_profile.is_none()
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
            &profile_path,
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
            &profile_path,
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
        workspace_profile_schema: config.schemas.workspace_profile.clone(),
    };
    let request = WorkspaceDiscoveryRequest {
        scan: scan.clone(),
        previous_profile: existing_profile.clone(),
    };
    store.save_status(&workspace_discovery_status(
        &workspace_path,
        &scan_path,
        &profile_path,
        scan.workspace_fingerprint.clone(),
        existing_status
            .as_ref()
            .and_then(|status| status.profile_fingerprint.clone()),
        scan.scanned_at,
        existing_status
            .as_ref()
            .and_then(|status| status.last_refreshed_at),
        None,
        false,
        WorkspaceDiscoveryPhase::Polishing,
    ))?;

    match worker.discover(&context, &artifacts, &request).await {
        Ok(result) => {
            let bytes = fs::read(&artifacts.output_file)
                .with_context(|| format!("failed to read {}", artifacts.output_file.display()))?;
            let profile: WorkspaceProfile = serde_json::from_slice(&bytes)
                .with_context(|| format!("failed to parse {}", artifacts.output_file.display()))?;
            let profile_fingerprint = profile_fingerprint(&profile)?;
            store.save_profile(&profile)?;
            fs::write(
                &artifacts.result_file,
                serde_json::to_vec_pretty(&result)
                    .context("failed to serialize discovery result")?,
            )
            .with_context(|| format!("failed to write {}", artifacts.result_file.display()))?;

            let status = workspace_discovery_status(
                &workspace_path,
                &scan_path,
                &profile_path,
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
                canonical_profile_path: profile_path,
                scan_path,
                status_path,
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
                    &profile_path,
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
                &profile_path,
                scan.workspace_fingerprint,
                None,
                scan.scanned_at,
                existing_status
                    .as_ref()
                    .and_then(|status| status.last_refreshed_at),
                Some(error_message),
                false,
                WorkspaceDiscoveryPhase::Failed,
            );
            store.save_status(&status)?;
            Err(error)
        }
    }
}

fn workspace_discovery_status(
    workspace_path: &Path,
    scan_path: &Path,
    profile_path: &Path,
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
        profile_path: profile_path.to_path_buf(),
        workspace_fingerprint,
        profile_fingerprint,
        last_scanned_at,
        last_refreshed_at,
        last_refresh_error,
        used_fallback_profile,
        current_phase,
    }
}

fn ensure_discovery_assets(config: &ResolvedConfig) -> Result<()> {
    ensure_text_file(&config.prompts.discovery, DEFAULT_DISCOVERY_PROMPT)?;
    ensure_text_file(
        &config.schemas.workspace_profile,
        DEFAULT_WORKSPACE_PROFILE_SCHEMA,
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
            BuilderHandoff, EvaluationRequest, FeatureContract, PromptOverrides, QaReport,
            QaStatus, RunLifecycleStatus, RunState, WorkerResult, WorkerStatus,
        },
        worker::{DiscoveryContext, DiscoveryWorkerResult, WorkerAdapter, WorkerContext},
        workspace::WorkspaceIsolation,
    };

    use super::{HarnessUiService, LaunchDraft, refresh_workspace_profile};

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
                planner_output: schemas_dir.join("planner-output.json"),
                builder_handoff: schemas_dir.join("builder-handoff.json"),
                qa_report: schemas_dir.join("qa-report.json"),
            },
            runtime: RuntimeConfig {
                feature_limit: 1,
                max_repair_attempts: 1,
                continue_after_failure: false,
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
