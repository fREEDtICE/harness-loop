use std::{path::PathBuf, sync::Mutex};

use chrono::Utc;
use loopsmith_core::{
    discovery::WorkspaceDiscoveryPayload,
    domain::{PromptOverrides, PromptSnapshot, RunState},
    home,
    paths::normalize_path,
    storage::{LoopSmithStore, WorkspaceRecord},
};
use loopsmith_desktop_shell::{
    StageLogSseServer, validate_run_artifact_path, validate_stage_stdout_log_path,
};
use loopsmith_orchestration::{HarnessUiService, LaunchDraft, WorkspaceRunSummary};
use rfd::FileDialog;
use serde::Serialize;
use tauri::{Manager, State};
use tracing::debug;

struct AppState {
    service: HarnessUiService,
    store: Mutex<LoopSmithStore>,
    stage_log_sse: StageLogSseServer,
}

#[derive(Debug, Clone, Serialize)]
struct PromptBundle {
    defaults: PromptSnapshot,
    effective: PromptSnapshot,
}

#[derive(Debug, Clone, Serialize)]
struct WorkspacePayload {
    record: WorkspaceRecord,
    config_path: String,
    prompts: Option<PromptBundle>,
    discovery: Option<WorkspaceDiscoveryPayload>,
    runs: Vec<WorkspaceRunSummary>,
    current_run: Option<RunState>,
    config_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct GlobalPaths {
    home: String,
    config: String,
    prompts_dir: String,
}

impl AppState {
    fn new() -> Self {
        let db_path = home::loopsmith_db_path().expect("failed to resolve database path");
        let store = LoopSmithStore::open(db_path).expect("failed to open database");
        Self {
            service: HarnessUiService,
            store: Mutex::new(store),
            stage_log_sse: StageLogSseServer::bind_loopback()
                .expect("failed to start stage stdout SSE server"),
        }
    }
}

#[tauri::command]
fn pick_workspace_folder() -> Option<String> {
    FileDialog::new()
        .pick_folder()
        .map(normalize_path)
        .map(|path| path.display().to_string())
}

#[tauri::command]
fn load_workspaces(state: State<'_, AppState>) -> Result<Vec<WorkspaceRecord>, String> {
    let store = state.store.lock().map_err(|_| "lock poisoned")?;
    store.list_workspaces().map_err(render_error)
}

#[tauri::command]
fn remove_workspace(
    state: State<'_, AppState>,
    workspace_path: String,
) -> Result<Vec<WorkspaceRecord>, String> {
    let store = state.store.lock().map_err(|_| "lock poisoned")?;
    let ws = normalize_path(PathBuf::from(workspace_path));
    store.remove_workspace(&ws).map_err(render_error)?;
    store.list_workspaces().map_err(render_error)
}

#[tauri::command]
fn load_workspace(
    state: State<'_, AppState>,
    workspace_path: String,
) -> Result<WorkspacePayload, String> {
    let workspace_path = state
        .service
        .validate_workspace_path(PathBuf::from(workspace_path))
        .map_err(render_error)?;

    let config_path = home::ensure_workspace_config(&workspace_path).map_err(render_error)?;

    let display_name = workspace_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| workspace_path.display().to_string());

    let record = WorkspaceRecord {
        workspace_path: workspace_path.clone(),
        display_name,
        last_opened_at: Utc::now(),
        pinned: false,
    };

    {
        let store = state.store.lock().map_err(|_| "lock poisoned")?;
        let existing = store.list_workspaces().map_err(render_error)?;
        let mut upsert_record = record.clone();
        if let Some(prev) = existing.iter().find(|r| r.workspace_path == workspace_path) {
            upsert_record.pinned = prev.pinned;
        }
        store
            .upsert_workspace(&upsert_record)
            .map_err(render_error)?;
    }

    let mut prompts = None;
    let discovery = state
        .service
        .load_discovery_payload(&workspace_path)
        .map_err(render_error)?;
    let mut runs = Vec::new();
    let mut current_run = None;
    let mut config_error = None;

    match build_prompt_bundle(&state.service, &config_path, &PromptOverrides::default()) {
        Ok(bundle) => prompts = Some(bundle),
        Err(err) => config_error = Some(err),
    }

    match state
        .service
        .list_runs_for_workspace(&config_path, &workspace_path)
    {
        Ok(found_runs) => runs = found_runs,
        Err(err) => {
            if config_error.is_none() {
                config_error = Some(render_error(err));
            }
        }
    }

    if let Some(run_root) = runs.first().map(|r| r.run_root.clone()) {
        current_run = state.service.inspect_run(&config_path, &run_root).ok();
    }

    debug!(
        workspace_path = %workspace_path.display(),
        config_path = %config_path.display(),
        run_count = runs.len(),
        current_run_root = current_run
            .as_ref()
            .map(|run| run.run_root.display().to_string())
            .unwrap_or_else(|| "-".to_string()),
        current_run_lifecycle = current_run
            .as_ref()
            .map(|run| run.lifecycle.as_str())
            .unwrap_or("-"),
        "loaded workspace payload"
    );

    Ok(WorkspacePayload {
        record,
        config_path: config_path.display().to_string(),
        prompts,
        discovery,
        runs,
        current_run,
        config_error,
    })
}

#[tauri::command]
fn load_prompt_bundle_for_workspace(
    state: State<'_, AppState>,
    workspace_path: String,
    overrides: PromptOverrides,
) -> Result<PromptBundle, String> {
    let workspace_path = normalize_path(PathBuf::from(workspace_path));
    let config_path = home::workspace_config_path(&workspace_path);
    let defaults = state
        .service
        .load_effective_prompts(&config_path, &PromptOverrides::default())
        .map_err(render_error)?;
    let effective = state
        .service
        .load_effective_prompts(&config_path, &overrides)
        .map_err(render_error)?;
    Ok(PromptBundle {
        defaults,
        effective,
    })
}

#[tauri::command]
fn list_runs(
    state: State<'_, AppState>,
    workspace_path: String,
) -> Result<Vec<WorkspaceRunSummary>, String> {
    let workspace_path = normalize_path(PathBuf::from(workspace_path));
    let config_path = home::workspace_config_path(&workspace_path);
    state
        .service
        .list_runs_for_workspace(config_path, &workspace_path)
        .map_err(render_error)
}

#[tauri::command]
fn inspect_run(
    state: State<'_, AppState>,
    workspace_path: String,
    run_root: String,
) -> Result<RunState, String> {
    let workspace_path = normalize_path(PathBuf::from(workspace_path));
    let config_path = home::workspace_config_path(&workspace_path);
    let run_root = normalize_path(PathBuf::from(run_root));
    let run = state
        .service
        .inspect_run(config_path, run_root.clone())
        .map_err(render_error)?;
    debug!(
        run_root = %run_root.display(),
        lifecycle = ?run.lifecycle,
        active_stage = run
            .active_stage
            .as_ref()
            .map(|stage| stage.stage.as_str())
            .unwrap_or("-"),
        active_attempt = run.active_stage.as_ref().map(|stage| stage.attempt).unwrap_or(0),
        feature_count = run.features.len(),
        "inspected run for UI"
    );
    Ok(run)
}

#[tauri::command]
async fn start_run(state: State<'_, AppState>, draft: LaunchDraft) -> Result<RunState, String> {
    state.service.start_run(draft).await.map_err(render_error)
}

#[tauri::command]
async fn resume_run(
    state: State<'_, AppState>,
    workspace_path: String,
    run_root: String,
) -> Result<RunState, String> {
    let workspace_path = normalize_path(PathBuf::from(workspace_path));
    let config_path = home::workspace_config_path(&workspace_path);
    let run_root = normalize_path(PathBuf::from(run_root));
    debug!(
        workspace_path = %workspace_path.display(),
        run_root = %run_root.display(),
        "resume_run command invoked from UI"
    );
    let run = state
        .service
        .resume_run(config_path, run_root.clone())
        .await
        .map_err(render_error)?;
    debug!(
        run_root = %run_root.display(),
        lifecycle = ?run.lifecycle,
        active_stage = run
            .active_stage
            .as_ref()
            .map(|stage| stage.stage.as_str())
            .unwrap_or("-"),
        "resume_run command completed"
    );
    Ok(run)
}

#[tauri::command]
fn read_global_config() -> Result<String, String> {
    let path = home::global_config_path().map_err(render_error)?;
    if !path.exists() {
        return Ok(String::new());
    }
    std::fs::read_to_string(&path).map_err(render_error)
}

#[tauri::command]
fn write_global_config(state: State<'_, AppState>, content: String) -> Result<(), String> {
    let path = home::global_config_path().map_err(render_error)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(render_error)?;
    }
    std::fs::write(&path, content.as_bytes()).map_err(render_error)?;

    let store = state.store.lock().map_err(render_error)?;
    let workspaces = store.list_workspaces().map_err(render_error)?;
    drop(store);

    for record in &workspaces {
        let ws_path = PathBuf::from(&record.workspace_path);
        if let Err(err) = home::patch_workspace_from_global(&ws_path, &content) {
            eprintln!(
                "warn: failed to patch workspace config for {}: {err}",
                ws_path.display()
            );
        }
    }

    Ok(())
}

#[tauri::command]
fn read_global_prompt(name: String) -> Result<String, String> {
    let home = home::loopsmith_home().map_err(render_error)?;
    let path = home.join("prompts").join(&name);
    if !path.exists() {
        return Ok(String::new());
    }
    std::fs::read_to_string(&path).map_err(render_error)
}

#[tauri::command]
fn write_global_prompt(name: String, content: String) -> Result<(), String> {
    let home = home::loopsmith_home().map_err(render_error)?;
    let path = home.join("prompts").join(&name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(render_error)?;
    }
    std::fs::write(&path, content.as_bytes()).map_err(render_error)
}

#[tauri::command]
fn read_workspace_config(workspace_path: String) -> Result<String, String> {
    let workspace_path = normalize_path(PathBuf::from(workspace_path));
    let config_path = home::workspace_config_path(&workspace_path);
    if !config_path.exists() {
        return Ok(String::new());
    }
    std::fs::read_to_string(&config_path).map_err(render_error)
}

#[tauri::command]
fn write_workspace_config(workspace_path: String, content: String) -> Result<(), String> {
    let workspace_path = normalize_path(PathBuf::from(workspace_path));
    let config_path = home::workspace_config_path(&workspace_path);
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).map_err(render_error)?;
    }
    std::fs::write(&config_path, content.as_bytes()).map_err(render_error)
}

#[tauri::command]
fn get_global_paths() -> Result<GlobalPaths, String> {
    let home = home::loopsmith_home().map_err(render_error)?;
    let config = home::global_config_path().map_err(render_error)?;
    Ok(GlobalPaths {
        home: home.display().to_string(),
        config: config.display().to_string(),
        prompts_dir: home.join("prompts").display().to_string(),
    })
}

#[tauri::command]
fn probe_environment() -> Result<loopsmith_core::env_probe::EnvironmentReport, String> {
    Ok(loopsmith_core::env_probe::probe_environment())
}

#[tauri::command]
fn has_default_config() -> Result<bool, String> {
    loopsmith_core::setup::has_default_config().map_err(render_error)
}

#[tauri::command]
fn save_setup_config(kind: String, binary: String, model: String) -> Result<(), String> {
    let home = home::loopsmith_home().map_err(render_error)?;
    loopsmith_core::setup::write_config_non_interactive(&home, &kind, &binary, &model)
        .map_err(render_error)?;
    Ok(())
}

fn build_prompt_bundle(
    service: &HarnessUiService,
    config_path: &std::path::Path,
    overrides: &PromptOverrides,
) -> Result<PromptBundle, String> {
    let defaults = service
        .load_effective_prompts(config_path, &PromptOverrides::default())
        .map_err(render_error)?;
    let effective = service
        .load_effective_prompts(config_path, overrides)
        .map_err(render_error)?;
    Ok(PromptBundle {
        defaults,
        effective,
    })
}

#[tauri::command]
fn read_stage_log(path: String) -> Result<String, String> {
    let path = validate_stage_stdout_log_path(&PathBuf::from(path)).map_err(render_error)?;
    if !path.exists() {
        debug!(path = %path.display(), "read_stage_log requested missing path");
        return Ok(String::new());
    }
    let content = std::fs::read_to_string(&path).map_err(render_error)?;
    debug!(
        path = %path.display(),
        bytes = content.len(),
        "read_stage_log served stage output"
    );
    Ok(content)
}

#[tauri::command]
fn read_run_artifact(path: String) -> Result<String, String> {
    let path = validate_run_artifact_path(&PathBuf::from(path)).map_err(render_error)?;
    if !path.exists() {
        return Ok(String::new());
    }
    std::fs::read_to_string(&path).map_err(render_error)
}

#[tauri::command]
fn stage_log_stream_url(state: State<'_, AppState>, path: String) -> Result<String, String> {
    state
        .stage_log_sse
        .stream_url(PathBuf::from(path))
        .map_err(render_error)
}

#[tauri::command]
fn open_file(path: String) -> Result<(), String> {
    let path = PathBuf::from(&path);
    if !path.exists() {
        return Err(format!("file not found: {}", path.display()));
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(render_error)?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(render_error)?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &path.display().to_string()])
            .spawn()
            .map_err(render_error)?;
    }
    Ok(())
}

#[tauri::command]
fn reveal_in_finder(path: String) -> Result<(), String> {
    let path = PathBuf::from(&path);
    if !path.exists() {
        return Err(format!("path not found: {}", path.display()));
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(&path)
            .spawn()
            .map_err(render_error)?;
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(parent) = path.parent() {
            std::process::Command::new("xdg-open")
                .arg(parent)
                .spawn()
                .map_err(render_error)?;
        }
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg("/select,")
            .arg(&path)
            .spawn()
            .map_err(render_error)?;
    }
    Ok(())
}

fn render_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[tauri::command]
fn write_ui_state(json: String) -> Result<(), String> {
    let path = ui_state_snapshot_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(render_error)?;
    }
    std::fs::write(&path, json.as_bytes()).map_err(render_error)
}

fn ui_state_snapshot_path() -> PathBuf {
    home::loopsmith_home()
        .unwrap_or_else(|_| PathBuf::from(".loopsmith"))
        .join("ui")
        .join("ui-state.json")
}

fn main() {
    let _log_guard = loopsmith_core::logging::init_logging().expect("failed to initialize logging");

    if let Err(err) = loopsmith_core::shell_env::inherit_shell_env() {
        eprintln!("warn: failed to inherit shell environment: {err}");
    }

    home::ensure_global_home().expect("failed to initialize LoopSmith global home");

    tauri::Builder::default()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            pick_workspace_folder,
            load_workspaces,
            remove_workspace,
            load_workspace,
            load_prompt_bundle_for_workspace,
            list_runs,
            inspect_run,
            start_run,
            resume_run,
            read_global_config,
            write_global_config,
            read_global_prompt,
            write_global_prompt,
            read_workspace_config,
            write_workspace_config,
            get_global_paths,
            probe_environment,
            has_default_config,
            save_setup_config,
            write_ui_state,
            read_stage_log,
            read_run_artifact,
            stage_log_stream_url,
            open_file,
            reveal_in_finder,
        ])
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_title("LoopSmith");
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run tauri application");
}
