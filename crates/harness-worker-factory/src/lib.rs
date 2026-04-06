use anyhow::{Result, bail};
use async_trait::async_trait;
use loopsmith_core::{
    artifacts::{FeatureLayout, StageArtifactSet},
    config::{PlannerWorkerConfig, ResolvedConfig, WorkerSelection},
    discovery::{DiscoveryArtifactSet, WorkspaceDiscoveryRequest},
    domain::{
        BuilderHandoff, EvaluationRequest, FeatureContract, PlanningRequest, QaReport, WorkerResult,
    },
    worker::{DiscoveryContext, DiscoveryWorkerResult, WorkerAdapter, WorkerContext},
};
use loopsmith_worker_claude::ClaudeCliWorker;
use loopsmith_worker_codex::CodexCliWorker;
use loopsmith_worker_gemini::GeminiCliWorker;
use loopsmith_worker_simulated::SimulatedWorker;

pub fn build_worker_from_selection(selection: &WorkerSelection) -> Result<Box<dyn WorkerAdapter>> {
    match selection {
        WorkerSelection::CodexCli { codex } => {
            verify_worker_binary(&codex.binary, "codex", "npm install -g @openai/codex")?;
            Ok(Box::new(CodexCliWorker::new(codex.clone())))
        }
        WorkerSelection::ClaudeCli { claude } => {
            verify_worker_binary(
                &claude.binary,
                "claude",
                "npm install -g @anthropic-ai/claude-code",
            )?;
            Ok(Box::new(ClaudeCliWorker::new(claude.clone())))
        }
        WorkerSelection::GeminiCli { gemini } => {
            verify_worker_binary(
                &gemini.binary,
                "gemini",
                "npm install -g @anthropic-ai/gemini-cli",
            )?;
            Ok(Box::new(GeminiCliWorker::new(gemini.clone())))
        }
        WorkerSelection::Simulated { simulation } => {
            Ok(Box::new(SimulatedWorker::new(simulation.clone())))
        }
    }
}

pub fn build_discovery_worker(config: &ResolvedConfig) -> Result<Box<dyn WorkerAdapter>> {
    if let Some(planner) = config.planner_worker() {
        build_worker_from_planner_config(planner)
    } else {
        build_worker_from_selection(&config.worker.selection)
    }
}

pub fn build_configured_worker(config: &ResolvedConfig) -> Result<Box<dyn WorkerAdapter>> {
    let default_worker = build_worker_from_selection(&config.worker.selection)?;
    let worker: Box<dyn WorkerAdapter> = if let Some(planner) = config.planner_worker() {
        let planner_worker = build_worker_from_planner_config(planner)?;
        Box::new(PlannerRoutedWorker::new(planner_worker, default_worker))
    } else {
        default_worker
    };
    Ok(worker)
}

fn verify_worker_binary(binary: &str, name: &str, install_hint: &str) -> Result<()> {
    if let Some(diagnostic) = loopsmith_core::shell_env::check_worker_binary(binary) {
        bail!(
            "{name} CLI: {diagnostic}\n\n\
             Troubleshooting:\n\
             1. Install {name} CLI: {install_hint}\n\
             2. Or specify the full path in your config file:\n\
                [worker.{name}]\n\
                binary = \"/full/path/to/{binary}\"\n\
             3. Verify it is accessible: which {binary}"
        );
    }

    Ok(())
}

fn build_worker_from_planner_config(
    config: &PlannerWorkerConfig,
) -> Result<Box<dyn WorkerAdapter>> {
    build_worker_from_selection(&config.selection)
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
    async fn discover(
        &self,
        context: &DiscoveryContext,
        artifacts: &DiscoveryArtifactSet,
        request: &WorkspaceDiscoveryRequest,
    ) -> Result<DiscoveryWorkerResult> {
        self.planner.discover(context, artifacts, request).await
    }

    async fn plan(
        &self,
        context: &WorkerContext,
        artifacts: &StageArtifactSet,
        request: &PlanningRequest,
    ) -> Result<WorkerResult> {
        self.planner.plan(context, artifacts, request).await
    }

    async fn build(
        &self,
        context: &WorkerContext,
        feature: &FeatureLayout,
        artifacts: &StageArtifactSet,
        contract: &FeatureContract,
    ) -> Result<WorkerResult> {
        self.default
            .build(context, feature, artifacts, contract)
            .await
    }

    async fn evaluate(
        &self,
        context: &WorkerContext,
        feature: &FeatureLayout,
        artifacts: &StageArtifactSet,
        request: &EvaluationRequest,
    ) -> Result<WorkerResult> {
        self.default
            .evaluate(context, feature, artifacts, request)
            .await
    }

    async fn repair(
        &self,
        context: &WorkerContext,
        feature: &FeatureLayout,
        artifacts: &StageArtifactSet,
        contract: &FeatureContract,
        builder_handoff: &BuilderHandoff,
        qa_report: &QaReport,
        previous_session_id: Option<&str>,
    ) -> Result<WorkerResult> {
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
