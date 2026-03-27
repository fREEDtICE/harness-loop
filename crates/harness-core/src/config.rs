use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{domain::QaStatus, paths::normalize_path, workspace::WorkspaceIsolation};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub project: ProjectConfig,
    pub storage: StorageConfig,
    pub workspace: WorkspaceConfig,
    pub worker: WorkerConfig,
    pub prompts: PromptConfig,
    pub schemas: SchemaConfig,
    pub runtime: RuntimeConfig,
    pub evaluator: EvaluatorConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub root_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub runs_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    #[serde(default)]
    pub isolation: WorkspaceIsolation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerConfig {
    pub kind: WorkerKind,
    pub codex: Option<CodexWorkerConfig>,
    pub simulation: Option<SimulationWorkerConfig>,
    #[serde(default)]
    pub planner: Option<PlannerWorkerConfig>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkerKind {
    CodexCli,
    Simulated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexWorkerConfig {
    pub binary: String,
    pub model: String,
    pub sandbox: String,
    pub full_auto: bool,
    pub skip_git_repo_check: bool,
    pub resume_sessions: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationWorkerConfig {
    #[serde(default = "default_simulation_evaluator_statuses")]
    pub evaluator_statuses: Vec<QaStatus>,
    #[serde(default = "default_simulation_session_prefix")]
    pub session_prefix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerWorkerConfig {
    pub kind: WorkerKind,
    pub codex: Option<CodexWorkerConfig>,
    pub simulation: Option<SimulationWorkerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptConfig {
    pub planner: PathBuf,
    pub builder: PathBuf,
    pub evaluator: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaConfig {
    pub planner_output: PathBuf,
    pub builder_handoff: PathBuf,
    pub qa_report: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub feature_limit: usize,
    pub max_repair_attempts: usize,
    #[serde(default)]
    pub continue_after_failure: bool,
    #[serde(default)]
    pub supervision: RuntimeSupervisionConfig,
    #[serde(default)]
    pub services: Vec<ServiceConfig>,
    #[serde(default)]
    pub stacks: Vec<StackConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSupervisionConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_startup_timeout_secs")]
    pub startup_timeout_secs: u64,
    #[serde(default = "default_readiness_poll_interval_ms")]
    pub readiness_poll_interval_ms: u64,
    #[serde(default = "default_shutdown_grace_period_secs")]
    pub shutdown_grace_period_secs: u64,
}

impl Default for RuntimeSupervisionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            startup_timeout_secs: default_startup_timeout_secs(),
            readiness_poll_interval_ms: default_readiness_poll_interval_ms(),
            shutdown_grace_period_secs: default_shutdown_grace_period_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceConfig {
    pub name: String,
    pub start: Vec<String>,
    pub working_dir: Option<PathBuf>,
    pub ready_url: Option<String>,
    pub ready_command: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackConfig {
    pub name: String,
    pub up: Vec<String>,
    pub down: Vec<String>,
    pub working_dir: Option<PathBuf>,
    pub ready_url: Option<String>,
    pub ready_command: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluatorConfig {
    pub dimensions: Vec<String>,
    pub require_screenshots: bool,
    pub commands: Vec<Vec<String>>,
    #[serde(default)]
    pub screenshots: Vec<ScreenshotConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenshotConfig {
    pub name: String,
    pub command: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub project_root: PathBuf,
    pub storage: ResolvedStorageConfig,
    pub workspace: WorkspaceConfig,
    pub worker: WorkerConfig,
    pub prompts: ResolvedPromptConfig,
    pub schemas: ResolvedSchemaConfig,
    pub runtime: RuntimeConfig,
    pub evaluator: EvaluatorConfig,
}

#[derive(Debug, Clone)]
pub struct ResolvedStorageConfig {
    pub runs_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ResolvedPromptConfig {
    pub planner: PathBuf,
    pub builder: PathBuf,
    pub evaluator: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ResolvedSchemaConfig {
    pub planner_output: PathBuf,
    pub builder_handoff: PathBuf,
    pub qa_report: PathBuf,
}

impl AppConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<ResolvedConfig> {
        let path = path.as_ref();
        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed to read config file {}", path.display()))?;

        let config: AppConfig = toml::from_str(&contents)
            .with_context(|| format!("failed to parse config file {}", path.display()))?;
        config.validate()?;

        let config_dir = path.parent().unwrap_or_else(|| Path::new("."));
        let project_root = resolve_path(config_dir, &config.project.root_dir);

        Ok(ResolvedConfig {
            project_root: project_root.clone(),
            storage: ResolvedStorageConfig {
                runs_dir: resolve_path(&project_root, &config.storage.runs_dir),
            },
            workspace: config.workspace,
            worker: config.worker,
            prompts: ResolvedPromptConfig {
                planner: resolve_path(&project_root, &config.prompts.planner),
                builder: resolve_path(&project_root, &config.prompts.builder),
                evaluator: resolve_path(&project_root, &config.prompts.evaluator),
            },
            schemas: ResolvedSchemaConfig {
                planner_output: resolve_path(&project_root, &config.schemas.planner_output),
                builder_handoff: resolve_path(&project_root, &config.schemas.builder_handoff),
                qa_report: resolve_path(&project_root, &config.schemas.qa_report),
            },
            runtime: config.runtime,
            evaluator: config.evaluator,
        })
    }
}

impl ResolvedConfig {
    pub fn codex_worker(&self) -> Result<&CodexWorkerConfig> {
        codex_worker_for(
            self.worker.kind,
            &self.worker.codex,
            "worker.kind is codex_cli but [worker.codex] is missing",
        )
    }

    pub fn simulation_worker(&self) -> Result<&SimulationWorkerConfig> {
        simulation_worker_for(
            self.worker.kind,
            &self.worker.simulation,
            "worker.kind is simulated but [worker.simulation] is missing",
        )
    }

    pub fn planner_worker(&self) -> Option<&PlannerWorkerConfig> {
        self.worker.planner.as_ref()
    }
}

impl WorkerConfig {
    fn validate(&self) -> Result<()> {
        validate_worker_selection(
            self.kind,
            &self.codex,
            &self.simulation,
            "worker.kind is codex_cli but [worker.codex] is missing",
            "worker.kind is simulated but [worker.simulation] is missing",
        )?;

        if let Some(planner) = &self.planner {
            planner.validate()?;
        }

        Ok(())
    }
}

impl PlannerWorkerConfig {
    pub fn codex_worker(&self) -> Result<&CodexWorkerConfig> {
        codex_worker_for(
            self.kind,
            &self.codex,
            "worker.planner.kind is codex_cli but [worker.planner.codex] is missing",
        )
    }

    pub fn simulation_worker(&self) -> Result<&SimulationWorkerConfig> {
        simulation_worker_for(
            self.kind,
            &self.simulation,
            "worker.planner.kind is simulated but [worker.planner.simulation] is missing",
        )
    }

    fn validate(&self) -> Result<()> {
        validate_worker_selection(
            self.kind,
            &self.codex,
            &self.simulation,
            "worker.planner.kind is codex_cli but [worker.planner.codex] is missing",
            "worker.planner.kind is simulated but [worker.planner.simulation] is missing",
        )
    }
}

impl EvaluatorConfig {
    fn validate(&self) -> Result<()> {
        if self.require_screenshots && self.screenshots.is_empty() {
            bail!("evaluator.require_screenshots is true but [[evaluator.screenshots]] is empty");
        }

        let mut seen = HashSet::new();
        for screenshot in &self.screenshots {
            let name = screenshot.name.trim();
            if name.is_empty() {
                bail!("evaluator.screenshots contains an empty name");
            }
            if screenshot.command.is_empty() {
                bail!("evaluator.screenshots `{name}` has an empty command");
            }
            if !seen.insert(name.to_string()) {
                bail!("evaluator.screenshots contains a duplicate name `{name}`");
            }
        }

        Ok(())
    }
}

impl AppConfig {
    fn validate(&self) -> Result<()> {
        self.worker.validate()?;
        self.evaluator.validate()
    }
}

fn default_simulation_evaluator_statuses() -> Vec<QaStatus> {
    vec![QaStatus::Pass]
}

fn default_simulation_session_prefix() -> String {
    "simulated".to_string()
}

fn default_startup_timeout_secs() -> u64 {
    30
}

fn default_readiness_poll_interval_ms() -> u64 {
    250
}

fn default_shutdown_grace_period_secs() -> u64 {
    5
}

fn validate_worker_selection(
    kind: WorkerKind,
    codex: &Option<CodexWorkerConfig>,
    simulation: &Option<SimulationWorkerConfig>,
    missing_codex_message: &str,
    missing_simulation_message: &str,
) -> Result<()> {
    match kind {
        WorkerKind::CodexCli => {
            if codex.is_none() {
                bail!("{missing_codex_message}");
            }
        }
        WorkerKind::Simulated => {
            if simulation.is_none() {
                bail!("{missing_simulation_message}");
            }
        }
    }

    Ok(())
}

fn codex_worker_for<'a>(
    kind: WorkerKind,
    codex: &'a Option<CodexWorkerConfig>,
    missing_message: &str,
) -> Result<&'a CodexWorkerConfig> {
    if kind != WorkerKind::CodexCli {
        bail!("requested codex worker config for a non-codex worker");
    }

    codex.as_ref().with_context(|| missing_message.to_string())
}

fn simulation_worker_for<'a>(
    kind: WorkerKind,
    simulation: &'a Option<SimulationWorkerConfig>,
    missing_message: &str,
) -> Result<&'a SimulationWorkerConfig> {
    if kind != WorkerKind::Simulated {
        bail!("requested simulation worker config for a non-simulated worker");
    }

    simulation
        .as_ref()
        .with_context(|| missing_message.to_string())
}

fn resolve_path(base_dir: &Path, value: &Path) -> PathBuf {
    if value.is_absolute() {
        normalize_path(value.to_path_buf())
    } else {
        normalize_path(base_dir.join(value))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::{domain::QaStatus, workspace::WorkspaceIsolation};

    use super::{AppConfig, WorkerKind};

    #[test]
    fn config_resolves_paths_from_project_root() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path();
        let config_dir = project_root.join("config");
        fs::create_dir_all(&config_dir).expect("create config dir");

        let config_file = config_dir.join("harness.toml");
        fs::write(
            &config_file,
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
planner_output = "schemas/planner.json"
builder_handoff = "schemas/builder.json"
qa_report = "schemas/qa.json"

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

        let resolved = AppConfig::load(&config_file).expect("load config");
        assert_eq!(resolved.project_root, project_root);
        assert_eq!(resolved.storage.runs_dir, project_root.join("runs"));
        assert_eq!(resolved.workspace.isolation, WorkspaceIsolation::Direct);
        assert_eq!(resolved.worker.kind, WorkerKind::Simulated);
        assert_eq!(
            resolved
                .simulation_worker()
                .expect("simulation config")
                .evaluator_statuses,
            vec![QaStatus::Pass]
        );
        assert_eq!(
            resolved.prompts.planner,
            project_root.join("prompts/planner.md")
        );
        assert_eq!(
            resolved.schemas.builder_handoff,
            project_root.join("schemas/builder.json")
        );
    }

    #[test]
    fn config_rejects_required_screenshots_without_capture_commands() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path();
        let config_dir = project_root.join("config");
        fs::create_dir_all(&config_dir).expect("create config dir");

        let config_file = config_dir.join("harness.toml");
        fs::write(
            &config_file,
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
planner_output = "schemas/planner.json"
builder_handoff = "schemas/builder.json"
qa_report = "schemas/qa.json"

[runtime]
feature_limit = 1
max_repair_attempts = 1
services = []
stacks = []

[evaluator]
dimensions = ["correctness"]
require_screenshots = true
commands = []
"#,
        )
        .expect("write config");

        let err = AppConfig::load(&config_file).expect_err("config should fail");
        assert!(
            err.to_string()
                .contains("evaluator.require_screenshots is true"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn config_rejects_missing_planner_override_payload() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path();
        let config_dir = project_root.join("config");
        fs::create_dir_all(&config_dir).expect("create config dir");

        let config_file = config_dir.join("harness.toml");
        fs::write(
            &config_file,
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

[worker.planner]
kind = "codex_cli"

[prompts]
planner = "prompts/planner.md"
builder = "prompts/builder.md"
evaluator = "prompts/evaluator.md"

[schemas]
planner_output = "schemas/planner.json"
builder_handoff = "schemas/builder.json"
qa_report = "schemas/qa.json"

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

        let err = AppConfig::load(&config_file).expect_err("config should fail");
        assert!(
            err.to_string().contains("worker.planner.kind is codex_cli"),
            "unexpected error: {err:#}"
        );
    }
}
