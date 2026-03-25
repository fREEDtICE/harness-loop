use std::{fs, path::Path, path::PathBuf};

use anyhow::{Context, Result};
use async_trait::async_trait;
use clap::{Parser, Subcommand};
use harness_core::{
    artifacts::{FeatureLayout, FileArtifactStore, StageArtifactSet},
    config::{
        AppConfig, CodexWorkerConfig, PlannerWorkerConfig, ResolvedConfig, SimulationWorkerConfig,
        WorkerKind,
    },
    controller::HarnessController,
    domain::{BuilderHandoff, EvaluationRequest, FeatureContract, QaReport, RunRequest, RunState},
    paths::normalize_path,
    worker::{WorkerAdapter, WorkerContext},
};
use harness_worker_codex::CodexCliWorker;
use harness_worker_simulated::SimulatedWorker;
use tracing_subscriber::{EnvFilter, fmt};

#[derive(Debug, Parser)]
#[command(name = "codex-harness-rs")]
#[command(about = "Rust scaffold for a long-running app development harness")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run {
        #[arg(long, default_value = "config/codex-cli.toml")]
        config: PathBuf,
        #[arg(long)]
        workspace: PathBuf,
        #[arg(long)]
        request_file: PathBuf,
        #[arg(long)]
        feature_limit: Option<usize>,
    },
    Resume {
        #[arg(long, default_value = "config/codex-cli.toml")]
        config: PathBuf,
        #[arg(long)]
        run_root: PathBuf,
    },
    Inspect {
        #[arg(long, default_value = "config/codex-cli.toml")]
        config: PathBuf,
        #[arg(long)]
        run_root: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let cli = Cli::parse();

    match cli.command {
        Command::Run {
            config,
            workspace,
            request_file,
            feature_limit,
        } => {
            let config_path = absolutize(&config)?;
            let source_workspace = absolutize(&workspace)?;
            let request_file = absolutize(&request_file)?;

            let app_config = AppConfig::load(&config_path)?;
            let request = fs::read_to_string(&request_file).with_context(|| {
                format!("failed to read request file {}", request_file.display())
            })?;

            let state = with_selected_worker(app_config, |controller| async move {
                controller
                    .start_run(RunRequest {
                        user_request: request,
                        source_workspace,
                        feature_limit,
                    })
                    .await
            })
            .await?;

            print_run_state(&state);
        }
        Command::Resume { config, run_root } => {
            let config_path = absolutize(&config)?;
            let run_root = absolutize(&run_root)?;
            let app_config = AppConfig::load(&config_path)?;

            let state = with_selected_worker(app_config, |controller| async move {
                controller.resume_run(run_root).await
            })
            .await?;

            print_run_state(&state);
        }
        Command::Inspect { config, run_root } => {
            let config_path = absolutize(&config)?;
            let run_root = absolutize(&run_root)?;
            let app_config = AppConfig::load(&config_path)?;

            let state = with_selected_worker(app_config, |controller| async move {
                controller.inspect_run(run_root)
            })
            .await?;

            print_run_state(&state);
        }
    }

    Ok(())
}

async fn with_selected_worker<F, Fut>(config: ResolvedConfig, f: F) -> Result<RunState>
where
    F: FnOnce(HarnessController<Box<dyn WorkerAdapter>>) -> Fut,
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
    let controller = HarnessController::new(config, artifact_store, worker);
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
        feature: &FeatureLayout,
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
        feature: &FeatureLayout,
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
        feature: &FeatureLayout,
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

fn print_run_state(state: &RunState) {
    println!("run_id: {}", state.run_id);
    println!("run_root: {}", state.run_root.display());
    println!("state_file: {}", state.state_file.display());
    println!("manifest: {}", state.manifest_file.display());
    println!("request: {}", state.request_file.display());
    println!("plan: {}", state.plan_file.display());
    println!("runtime_plan: {}", state.runtime_plan_file.display());
    println!("source_workspace: {}", state.source_workspace.display());
    println!(
        "execution_workspace: {}",
        state.execution_workspace.display()
    );
    println!("lifecycle: {}", state.lifecycle.as_str());
    println!(
        "final_status: {}",
        state.final_status.map_or("-", |status| status.as_str())
    );
    println!("current_feature_index: {}", state.current_feature_index);

    if let Some(plan_stage) = &state.plan_stage {
        println!(
            "stage=plan attempt={} status={} session_id={} artifact={}",
            plan_stage.attempt,
            plan_stage.status.as_str(),
            plan_stage.session_id.as_deref().unwrap_or("-"),
            plan_stage.artifact.display()
        );
    }

    for feature in &state.features {
        println!(
            "feature=index={} id={} status={} phase={} repairs={} qa={} root={}",
            feature.index,
            feature.feature_id,
            feature.status.as_str(),
            feature.phase.as_str(),
            feature.repair_attempts_used,
            feature.last_qa_status.map_or("-", |status| status.as_str()),
            feature.feature_root.display()
        );

        for stage in &feature.stages {
            println!(
                "stage=feature:{} stage={} attempt={} status={} session_id={} artifact={}",
                feature.feature_id,
                stage.stage.as_str(),
                stage.attempt,
                stage.status.as_str(),
                stage.session_id.as_deref().unwrap_or("-"),
                stage.artifact.display()
            );
        }
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(
            "info,harness_core=info,harness_worker_codex=info,harness_worker_simulated=info",
        )
    });

    fmt().with_env_filter(filter).with_target(false).init();
}

fn absolutize(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(normalize_path(path.to_path_buf()))
    } else {
        Ok(std::env::current_dir()
            .context("failed to read current working directory")?
            .join(path))
        .map(normalize_path)
    }
}
