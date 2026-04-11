use std::{fs, path::Path, path::PathBuf, process};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use loopsmith_core::{
    discovery::{WorkspaceDiscoveryPayload, WorkspaceDiscoveryPhase, WorkspaceProfileSelection},
    discovery_quality::{
        DiscoveryQualityGate, discovery_quality_report_path, evaluate_discovery_store,
        print_discovery_quality_report, save_discovery_quality_report,
    },
    domain::RunState,
    home, logging,
    paths::normalize_path,
    setup, shell_env,
};
use loopsmith_orchestration::service::{HarnessUiService, LaunchDraft};

#[derive(Debug, Parser)]
#[command(name = "loopsmith")]
#[command(about = "LoopSmith – a long-running application development harness")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[arg(
        long,
        global = true,
        help = "Disable the GUI and run in headless CLI mode"
    )]
    no_ui: bool,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run {
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        workspace: PathBuf,
        #[arg(long)]
        request_file: PathBuf,
        #[arg(long)]
        feature_limit: Option<usize>,
    },
    Resume {
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        run_root: PathBuf,
    },
    Inspect {
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        run_root: PathBuf,
    },
    Discover {
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        workspace: PathBuf,
    },
    DiscoverEval {
        #[arg(long)]
        workspace: PathBuf,
    },
    Init {
        #[arg(long, help = "Overwrite existing workspace templates")]
        force: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let _log_guard = logging::init_logging()?;

    if let Err(err) = shell_env::inherit_shell_env() {
        eprintln!("warn: failed to inherit shell environment: {err}");
    }

    let cli = Cli::parse();
    let service = HarnessUiService;

    home::ensure_global_home()?;

    match cli.command {
        None => {
            if cli.no_ui {
                eprintln!("loopsmith: no subcommand specified. Run `loopsmith --help` for usage.");
                process::exit(1);
            }

            if !setup::has_default_config()? {
                setup::run_interactive_setup()?;
            }

            launch_gui()?;
        }
        Some(command) => {
            ensure_ready_for_cli(&command)?;
            run_command(command).await?;
        }
    }

    Ok(())
}

fn ensure_ready_for_cli(command: &Command) -> Result<()> {
    match command {
        Command::Init { .. } => {}
        Command::Run {
            config: Some(_), ..
        }
        | Command::Discover {
            config: Some(_), ..
        }
        | Command::Resume {
            config: Some(_), ..
        }
        | Command::Inspect {
            config: Some(_), ..
        }
        | Command::DiscoverEval { .. } => {}
        _ => {
            if !setup::has_default_config()? {
                setup::run_interactive_setup()?;
            }
        }
    }
    Ok(())
}

async fn run_command(command: Command) -> Result<()> {
    match command {
        Command::Init { force } => {
            if force {
                home::init_global_home_force()?;
            }
            setup::run_interactive_setup()?;
        }
        Command::Run {
            config,
            workspace,
            request_file,
            feature_limit,
        } => {
            let config_path = resolve_config(config)?;
            let source_workspace = absolutize(&workspace)?;
            let request_file = absolutize(&request_file)?;
            let request = fs::read_to_string(&request_file).with_context(|| {
                format!("failed to read request file {}", request_file.display())
            })?;

            let service = HarnessUiService;
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
            let config_path = resolve_config(config)?;
            let run_root = absolutize(&run_root)?;
            let service = HarnessUiService;
            let state = service.resume_run(config_path, run_root).await?;

            print_run_state(&state);
        }
        Command::Inspect { config, run_root } => {
            let config_path = resolve_config(config)?;
            let run_root = absolutize(&run_root)?;
            let service = HarnessUiService;
            let state = service.inspect_run(config_path, run_root)?;

            print_run_state(&state);
        }
        Command::Discover { config, workspace } => {
            let config_path = resolve_config(config)?;
            let workspace = absolutize(&workspace)?;
            let service = HarnessUiService;
            let selection = service.discover_workspace(&config_path, &workspace).await?;
            let payload = service.load_discovery_payload(&workspace)?;

            print_discovery_selection(&selection, payload.as_ref());
        }
        Command::DiscoverEval { workspace } => {
            let workspace = absolutize(&workspace)?;
            let store = loopsmith_core::discovery::WorkspaceDiscoveryStore::new(&workspace);
            let report = evaluate_discovery_store(&store)?;
            let report_path = discovery_quality_report_path(&store);
            save_discovery_quality_report(&store, &report)?;
            println!("{}", print_discovery_quality_report(&report_path, &report));
            if report.gate == DiscoveryQualityGate::Fail {
                process::exit(2);
            }
        }
    }

    Ok(())
}

fn launch_gui() -> Result<()> {
    println!("launching LoopSmith GUI...");
    // TODO: integrate Tauri GUI launch here
    // For now, this is a placeholder. The actual implementation will
    // spawn the Tauri window from the loopsmith-gui binary/crate.
    Ok(())
}

fn resolve_config(explicit: Option<PathBuf>) -> Result<PathBuf> {
    match explicit {
        Some(path) => absolutize(&path),
        None => setup::default_config_path(),
    }
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

fn print_discovery_selection(
    selection: &WorkspaceProfileSelection,
    payload: Option<&WorkspaceDiscoveryPayload>,
) {
    println!("workspace: {}", selection.profile.workspace_path.display());
    println!(
        "discovery_root: {}",
        selection
            .canonical_profile_path
            .parent()
            .map_or_else(|| "-".to_string(), |path| path.display().to_string())
    );
    println!("scan: {}", selection.scan_path.display());
    println!("evidence: {}", selection.evidence_path.display());
    println!("inference: {}", selection.inference_path.display());
    println!("profile: {}", selection.canonical_profile_path.display());
    println!("status: {}", selection.status_path.display());
    println!(
        "phase: {}",
        payload
            .map(|item| discovery_phase_str(item.status.current_phase))
            .unwrap_or("unknown")
    );
    println!("workspace_fingerprint: {}", selection.workspace_fingerprint);
    println!("profile_fingerprint: {}", selection.profile_fingerprint);
    println!(
        "last_scanned_at: {}",
        selection.last_scanned_at.to_rfc3339()
    );
    println!(
        "last_refreshed_at: {}",
        selection.last_refreshed_at.to_rfc3339()
    );
    println!("used_fallback_profile: {}", selection.used_fallback_profile);
    println!(
        "refresh_error: {}",
        selection.refresh_error.as_deref().unwrap_or("-")
    );
    println!("summary: {}", selection.profile.summary);

    if let Some(payload) = payload {
        println!(
            "inference_summary: {}",
            payload.inference_summary.as_deref().unwrap_or("-")
        );
        println!("source_files: {}", payload.overview.source_file_count);
        println!("repositories: {}", payload.overview.repository_count);
        println!("api_contracts: {}", payload.overview.api_contract_count);
        println!("layer_rules: {}", payload.overview.layering_rules.len());
        println!("inferences: {}", payload.overview.inference_count);
        println!(
            "average_inference_confidence: {}",
            payload
                .overview
                .average_inference_confidence
                .map(|value| format!("{value:.2}"))
                .unwrap_or_else(|| "-".to_string())
        );
    }
}

fn discovery_phase_str(phase: WorkspaceDiscoveryPhase) -> &'static str {
    match phase {
        WorkspaceDiscoveryPhase::Idle => "idle",
        WorkspaceDiscoveryPhase::Scanning => "scanning",
        WorkspaceDiscoveryPhase::ReusingCachedProfile => "reusing_cached_profile",
        WorkspaceDiscoveryPhase::Polishing => "polishing",
        WorkspaceDiscoveryPhase::UsingFallbackProfile => "using_fallback_profile",
        WorkspaceDiscoveryPhase::Ready => "ready",
        WorkspaceDiscoveryPhase::Failed => "failed",
    }
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
