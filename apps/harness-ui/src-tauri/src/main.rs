use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

use chrono::Utc;
use harness_core::{
    domain::{PromptOverrides, PromptSnapshot, RunState},
    paths::normalize_path,
};
use harness_ui::{
    HarnessUiService, LaunchDraft, WorkspaceProfile, WorkspaceProfileStore, WorkspaceRunSummary,
};
use rfd::FileDialog;
use serde::Serialize;
use tauri::{Manager, State};

struct AppState {
    service: HarnessUiService,
    profile_store: WorkspaceProfileStore,
    profile_lock: Mutex<()>,
}

#[derive(Debug, Clone, Serialize)]
struct PromptBundle {
    defaults: PromptSnapshot,
    effective: PromptSnapshot,
}

#[derive(Debug, Clone, Serialize)]
struct WorkspacePayload {
    profile: WorkspaceProfile,
    prompts: Option<PromptBundle>,
    runs: Vec<WorkspaceRunSummary>,
    current_run: Option<RunState>,
    config_error: Option<String>,
}

impl AppState {
    fn new() -> Self {
        Self {
            service: HarnessUiService,
            profile_store: WorkspaceProfileStore::new(default_profile_store_path()),
            profile_lock: Mutex::new(()),
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
fn pick_config_file() -> Option<String> {
    FileDialog::new()
        .add_filter("TOML", &["toml"])
        .pick_file()
        .map(normalize_path)
        .map(|path| path.display().to_string())
}

#[tauri::command]
fn load_profiles(state: State<'_, AppState>) -> Result<Vec<WorkspaceProfile>, String> {
    let _guard = state
        .profile_lock
        .lock()
        .map_err(|_| "failed to lock workspace profiles".to_string())?;
    state.profile_store.load().map_err(render_error)
}

#[tauri::command]
fn save_profile(
    state: State<'_, AppState>,
    mut profile: WorkspaceProfile,
) -> Result<Vec<WorkspaceProfile>, String> {
    let _guard = state
        .profile_lock
        .lock()
        .map_err(|_| "failed to lock workspace profiles".to_string())?;
    profile.workspace_path = normalize_path(profile.workspace_path);
    profile.preferred_config_path = profile.preferred_config_path.map(normalize_path);
    profile.last_run_root = profile.last_run_root.map(normalize_path);
    profile.last_opened_at = Utc::now();
    state.profile_store.upsert(profile).map_err(render_error)
}

#[tauri::command]
fn remove_profile(
    state: State<'_, AppState>,
    workspace_path: String,
) -> Result<Vec<WorkspaceProfile>, String> {
    let _guard = state
        .profile_lock
        .lock()
        .map_err(|_| "failed to lock workspace profiles".to_string())?;
    let workspace_path = normalize_path(PathBuf::from(workspace_path));
    state
        .profile_store
        .remove(&workspace_path)
        .map_err(render_error)
}

#[tauri::command]
fn load_prompt_bundle(
    state: State<'_, AppState>,
    config_path: String,
    overrides: PromptOverrides,
) -> Result<PromptBundle, String> {
    let config_path = normalize_path(PathBuf::from(config_path));
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
fn load_workspace(
    state: State<'_, AppState>,
    workspace_path: String,
) -> Result<WorkspacePayload, String> {
    let workspace_path = normalize_path(PathBuf::from(workspace_path));
    let _guard = state
        .profile_lock
        .lock()
        .map_err(|_| "failed to lock workspace profiles".to_string())?;
    let existing_profiles = state.profile_store.load().map_err(render_error)?;
    let mut profile = existing_profiles
        .into_iter()
        .find(|profile| profile.workspace_path == workspace_path)
        .unwrap_or_else(|| WorkspaceProfile::new(workspace_path.clone()));
    profile.last_opened_at = Utc::now();
    let profiles = state
        .profile_store
        .upsert(profile.clone())
        .map_err(render_error)?;
    profile = profiles
        .into_iter()
        .find(|entry| entry.workspace_path == workspace_path)
        .unwrap_or(profile);

    let mut prompts = None;
    let mut runs = Vec::new();
    let mut current_run = None;
    let mut config_error = None;

    if let Some(config_path) = profile.preferred_config_path.clone() {
        match build_prompt_bundle(&state.service, &config_path, &profile.prompt_overrides) {
            Ok(bundle) => prompts = Some(bundle),
            Err(err) => config_error = Some(err),
        }

        match state
            .service
            .list_runs_for_workspace(&config_path, &profile.workspace_path)
        {
            Ok(found_runs) => {
                runs = found_runs;
            }
            Err(err) => config_error = Some(render_error(err)),
        }

        if let Some(run_root) = profile
            .last_run_root
            .clone()
            .or_else(|| runs.first().map(|run| run.run_root.clone()))
        {
            current_run = state.service.inspect_run(&config_path, &run_root).ok();
        }
    }

    Ok(WorkspacePayload {
        profile,
        prompts,
        runs,
        current_run,
        config_error,
    })
}

#[tauri::command]
fn list_runs(
    state: State<'_, AppState>,
    config_path: String,
    workspace_path: String,
) -> Result<Vec<WorkspaceRunSummary>, String> {
    state
        .service
        .list_runs_for_workspace(
            normalize_path(PathBuf::from(config_path)),
            normalize_path(PathBuf::from(workspace_path)),
        )
        .map_err(render_error)
}

#[tauri::command]
fn inspect_run(
    state: State<'_, AppState>,
    config_path: String,
    run_root: String,
) -> Result<RunState, String> {
    state
        .service
        .inspect_run(
            normalize_path(PathBuf::from(config_path)),
            normalize_path(PathBuf::from(run_root)),
        )
        .map_err(render_error)
}

#[tauri::command]
async fn start_run(state: State<'_, AppState>, draft: LaunchDraft) -> Result<RunState, String> {
    state.service.start_run(draft).await.map_err(render_error)
}

#[tauri::command]
async fn resume_run(
    state: State<'_, AppState>,
    config_path: String,
    run_root: String,
) -> Result<RunState, String> {
    state
        .service
        .resume_run(
            normalize_path(PathBuf::from(config_path)),
            normalize_path(PathBuf::from(run_root)),
        )
        .await
        .map_err(render_error)
}

fn build_prompt_bundle(
    service: &HarnessUiService,
    config_path: &Path,
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

fn render_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn default_profile_store_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".codex-harness-rs")
        .join("ui")
        .join("workspace-profiles.json")
}

fn main() {
    tauri::Builder::default()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            pick_workspace_folder,
            pick_config_file,
            load_profiles,
            save_profile,
            remove_profile,
            load_prompt_bundle,
            load_workspace,
            list_runs,
            inspect_run,
            start_run,
            resume_run,
        ])
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_title("Codex Harness UI");
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run tauri application");
}
