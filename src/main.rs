use std::{fs, path::Path, path::PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use harness_core::{domain::RunState, paths::normalize_path};
use harness_ui::service::{HarnessUiService, LaunchDraft};
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
    let service = HarnessUiService;

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
            let request = fs::read_to_string(&request_file).with_context(|| {
                format!("failed to read request file {}", request_file.display())
            })?;

            let state = service
                .start_run(LaunchDraft {
                    workspace_path: source_workspace,
                    config_path,
                    request_draft: request,
                    prompt_overrides: Default::default(),
                    feature_limit,
                })
                .await?;

            print_run_state(&state);
        }
        Command::Resume { config, run_root } => {
            let config_path = absolutize(&config)?;
            let run_root = absolutize(&run_root)?;
            let state = service.resume_run(config_path, run_root).await?;

            print_run_state(&state);
        }
        Command::Inspect { config, run_root } => {
            let config_path = absolutize(&config)?;
            let run_root = absolutize(&run_root)?;
            let state = service.inspect_run(config_path, run_root)?;

            print_run_state(&state);
        }
    }

    Ok(())
}

fn print_run_state(state: &RunState) {
    println!("run_id: {}", state.run_id);
    println!("run_root: {}", state.run_root.display());
    println!("state_file: {}", state.state_file.display());
    println!("manifest: {}", state.manifest_file.display());
    println!(
        "launch: {}",
        state
            .launch_file
            .as_ref()
            .map_or_else(|| "-".to_string(), |path| path.display().to_string())
    );
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
    if let Some(active_stage) = &state.active_stage {
        println!(
            "active_stage: stage={} attempt={} feature={} since={}",
            active_stage.stage.as_str(),
            active_stage.attempt,
            active_stage.feature_id.as_deref().unwrap_or("-"),
            active_stage.started_at.to_rfc3339(),
        );
    }

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
