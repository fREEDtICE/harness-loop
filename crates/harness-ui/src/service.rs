use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use harness_core::{
    artifacts::{FileArtifactStore, StageArtifactSet},
    config::{
        AppConfig, CodexWorkerConfig, PlannerWorkerConfig, ResolvedConfig, SimulationWorkerConfig,
        WorkerKind,
    },
    domain::{
        BuilderHandoff, EvaluationRequest, FeatureContract, PromptOverrides, PromptSnapshot,
        QaReport, QaStatus, RunLifecycleStatus, RunRequest, RunState,
    },
    paths::normalize_path,
    worker::{WorkerAdapter, WorkerContext},
};
use harness_worker_codex::CodexCliWorker;
use harness_worker_simulated::SimulatedWorker;
use serde::{Deserialize, Serialize};

/// Draft launch inputs captured by the UI before a run starts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchDraft {
    pub workspace_path: PathBuf,
    pub config_path: PathBuf,
    pub request_draft: String,
    pub prompt_overrides: PromptOverrides,
    pub feature_limit: Option<usize>,
}

/// Launch inputs after validation and prompt resolution.
#[derive(Debug, Clone)]
pub struct PreparedLaunch {
    pub config_path: PathBuf,
    pub prompt_snapshot: PromptSnapshot,
    resolved_config: ResolvedConfig,
    run_request: RunRequest,
}

/// Run summary model used by workspace and history views.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceRunSummary {
    pub run_root: PathBuf,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub lifecycle: RunLifecycleStatus,
    pub final_status: Option<QaStatus>,
    pub current_feature_index: usize,
    pub total_features: usize,
    pub active_stage: Option<harness_core::domain::ActiveRunStage>,
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

    pub fn prepare_launch(&self, draft: LaunchDraft) -> Result<PreparedLaunch> {
        let workspace_path = normalize_path(draft.workspace_path);
        if !workspace_path.exists() {
            bail!("workspace {} does not exist", workspace_path.display());
        }
        if !workspace_path.is_dir() {
            bail!("workspace {} is not a directory", workspace_path.display());
        }
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
            },
        })
    }

    pub async fn start_run(&self, draft: LaunchDraft) -> Result<RunState> {
        let prepared = self.prepare_launch(draft)?;
        self.start_prepared_run(prepared).await
    }

    pub async fn start_prepared_run(&self, prepared: PreparedLaunch) -> Result<RunState> {
        with_selected_worker(prepared.resolved_config, |controller| async move {
            controller.start_run(prepared.run_request).await
        })
        .await
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
        let default_worker = build_worker_from_selection(
            resolved_config.worker.kind,
            resolved_config.worker.codex.as_ref(),
            resolved_config.worker.simulation.as_ref(),
        )?;
        let worker: Box<dyn WorkerAdapter> = if let Some(planner) = resolved_config.planner_worker()
        {
            let planner_worker = build_worker_from_planner_config(planner)?;
            Box::new(PlannerRoutedWorker::new(planner_worker, default_worker))
        } else {
            default_worker
        };
        let controller = harness_core::controller::HarnessController::new(
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

            runs.push(WorkspaceRunSummary {
                run_root: state.run_root,
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
    F: FnOnce(harness_core::controller::HarnessController<Box<dyn WorkerAdapter>>) -> Fut,
    Fut: std::future::Future<Output = Result<RunState>>,
{
    let artifact_store = FileArtifactStore::new(config.storage.runs_dir.clone());
    let default_worker = build_worker_from_selection(
        config.worker.kind,
        config.worker.codex.as_ref(),
        config.worker.simulation.as_ref(),
    )?;
    let worker: Box<dyn WorkerAdapter> = if let Some(planner) = config.planner_worker() {
        let planner_worker = build_worker_from_planner_config(planner)?;
        Box::new(PlannerRoutedWorker::new(planner_worker, default_worker))
    } else {
        default_worker
    };
    let controller =
        harness_core::controller::HarnessController::new(config, artifact_store, worker);
    f(controller).await
}

struct PlannerRoutedWorker {
    planner: Box<dyn WorkerAdapter>,
    default: Box<dyn WorkerAdapter>,
}

impl PlannerRoutedWorker {
    fn new(planner: Box<dyn WorkerAdapter>, default: Box<dyn WorkerAdapter>) -> Self {
        Self { planner, default }
    }
}

#[async_trait]
impl WorkerAdapter for PlannerRoutedWorker {
    async fn plan(
        &self,
        context: &WorkerContext,
        artifacts: &StageArtifactSet,
        request: &harness_core::domain::PlanningRequest,
    ) -> Result<harness_core::domain::WorkerResult> {
        self.planner.plan(context, artifacts, request).await
    }

    async fn build(
        &self,
        context: &WorkerContext,
        feature: &harness_core::artifacts::FeatureLayout,
        artifacts: &StageArtifactSet,
        contract: &FeatureContract,
    ) -> Result<harness_core::domain::WorkerResult> {
        self.default
            .build(context, feature, artifacts, contract)
            .await
    }

    async fn evaluate(
        &self,
        context: &WorkerContext,
        feature: &harness_core::artifacts::FeatureLayout,
        artifacts: &StageArtifactSet,
        request: &EvaluationRequest,
    ) -> Result<harness_core::domain::WorkerResult> {
        self.default
            .evaluate(context, feature, artifacts, request)
            .await
    }

    async fn repair(
        &self,
        context: &WorkerContext,
        feature: &harness_core::artifacts::FeatureLayout,
        artifacts: &StageArtifactSet,
        contract: &FeatureContract,
        builder_handoff: &BuilderHandoff,
        qa_report: &QaReport,
        previous_session_id: Option<&str>,
    ) -> Result<harness_core::domain::WorkerResult> {
        self.default
            .repair(
                context,
                feature,
                artifacts,
                contract,
                builder_handoff,
                qa_report,
                previous_session_id,
            )
            .await
    }
}

fn build_worker_from_planner_config(
    config: &PlannerWorkerConfig,
) -> Result<Box<dyn WorkerAdapter>> {
    build_worker_from_selection(
        config.kind,
        config.codex.as_ref(),
        config.simulation.as_ref(),
    )
}

fn build_worker_from_selection(
    kind: WorkerKind,
    codex: Option<&CodexWorkerConfig>,
    simulation: Option<&SimulationWorkerConfig>,
) -> Result<Box<dyn WorkerAdapter>> {
    Ok(match kind {
        WorkerKind::CodexCli => Box::new(CodexCliWorker::new(
            codex
                .context("codex worker config missing for selected worker")?
                .clone(),
        )),
        WorkerKind::Simulated => Box::new(SimulatedWorker::new(
            simulation
                .context("simulation worker config missing for selected worker")?
                .clone(),
        )),
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::Utc;
    use tempfile::tempdir;
    use uuid::Uuid;

    use harness_core::domain::{PromptOverrides, RunLifecycleStatus, RunState};

    use super::{HarnessUiService, LaunchDraft};

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
        fs::write(prompts_dir.join("planner.md"), "planner\n").expect("planner");
        fs::write(prompts_dir.join("builder.md"), "builder\n").expect("builder");
        fs::write(prompts_dir.join("evaluator.md"), "evaluator\n").expect("evaluator");
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
runs_dir = "runs"

[workspace]
isolation = "direct"

[worker]
kind = "simulated"

[worker.simulation]
evaluator_statuses = ["pass"]
session_prefix = "sim"

[prompts]
planner = "prompts/planner.md"
builder = "prompts/builder.md"
evaluator = "prompts/evaluator.md"

[schemas]
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
        let runs_dir = project_root.join("runs");
        let workspace = project_root.join("workspace");
        let other_workspace = project_root.join("workspace-other");
        fs::create_dir_all(&config_dir).expect("config dir");
        fs::create_dir_all(&prompts_dir).expect("prompts dir");
        fs::create_dir_all(&schemas_dir).expect("schemas dir");
        fs::create_dir_all(&runs_dir).expect("runs dir");
        fs::create_dir_all(&workspace).expect("workspace dir");
        fs::create_dir_all(&other_workspace).expect("other workspace dir");
        fs::write(prompts_dir.join("planner.md"), "planner\n").expect("planner");
        fs::write(prompts_dir.join("builder.md"), "builder\n").expect("builder");
        fs::write(prompts_dir.join("evaluator.md"), "evaluator\n").expect("evaluator");
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
runs_dir = "runs"

[workspace]
isolation = "direct"

[worker]
kind = "simulated"

[worker.simulation]
evaluator_statuses = ["pass"]
session_prefix = "sim"

[prompts]
planner = "prompts/planner.md"
builder = "prompts/builder.md"
evaluator = "prompts/evaluator.md"

[schemas]
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
}
