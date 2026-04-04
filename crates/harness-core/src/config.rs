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
    #[serde(flatten)]
    pub selection: WorkerSelection,
    #[serde(default)]
    pub planner: Option<PlannerWorkerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkerSelection {
    CodexCli { codex: CodexWorkerConfig },
    ClaudeCli { claude: ClaudeWorkerConfig },
    GeminiCli { gemini: GeminiWorkerConfig },
    Simulated { simulation: SimulationWorkerConfig },
}

impl WorkerSelection {
    pub fn kind(&self) -> WorkerKind {
        match self {
            Self::CodexCli { .. } => WorkerKind::CodexCli,
            Self::ClaudeCli { .. } => WorkerKind::ClaudeCli,
            Self::GeminiCli { .. } => WorkerKind::GeminiCli,
            Self::Simulated { .. } => WorkerKind::Simulated,
        }
    }

    pub fn codex(&self) -> Option<&CodexWorkerConfig> {
        match self {
            Self::CodexCli { codex } => Some(codex),
            _ => None,
        }
    }

    pub fn claude(&self) -> Option<&ClaudeWorkerConfig> {
        match self {
            Self::ClaudeCli { claude } => Some(claude),
            _ => None,
        }
    }

    pub fn gemini(&self) -> Option<&GeminiWorkerConfig> {
        match self {
            Self::GeminiCli { gemini } => Some(gemini),
            _ => None,
        }
    }

    pub fn simulation(&self) -> Option<&SimulationWorkerConfig> {
        match self {
            Self::Simulated { simulation } => Some(simulation),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkerKind {
    CodexCli,
    ClaudeCli,
    GeminiCli,
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
pub struct ClaudeWorkerConfig {
    pub binary: String,
    pub model: String,
    pub dangerously_skip_permissions: bool,
    pub resume_sessions: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiWorkerConfig {
    pub binary: String,
    pub model: String,
    pub sandbox: String,
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
    #[serde(flatten)]
    pub selection: WorkerSelection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptConfig {
    #[serde(default = "default_discovery_prompt_path")]
    pub discovery: PathBuf,
    pub planner: PathBuf,
    pub builder: PathBuf,
    pub evaluator: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaConfig {
    #[serde(default = "default_workspace_profile_schema_path")]
    pub workspace_profile: PathBuf,
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
    pub discovery: PathBuf,
    pub planner: PathBuf,
    pub builder: PathBuf,
    pub evaluator: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ResolvedSchemaConfig {
    pub workspace_profile: PathBuf,
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
                discovery: resolve_path(&project_root, &config.prompts.discovery),
                planner: resolve_path(&project_root, &config.prompts.planner),
                builder: resolve_path(&project_root, &config.prompts.builder),
                evaluator: resolve_path(&project_root, &config.prompts.evaluator),
            },
            schemas: ResolvedSchemaConfig {
                workspace_profile: resolve_path(&project_root, &config.schemas.workspace_profile),
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
    pub fn planner_worker(&self) -> Option<&PlannerWorkerConfig> {
        self.worker.planner.as_ref()
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
        self.evaluator.validate()
    }
}

fn default_simulation_evaluator_statuses() -> Vec<QaStatus> {
    vec![QaStatus::Pass]
}

fn default_discovery_prompt_path() -> PathBuf {
    PathBuf::from("prompts/discovery.md")
}

fn default_workspace_profile_schema_path() -> PathBuf {
    PathBuf::from("schemas/workspace-profile.json")
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
        assert_eq!(
            resolved.storage.runs_dir,
            project_root.join(".loopsmith-runs")
        );
        assert_eq!(resolved.workspace.isolation, WorkspaceIsolation::Direct);
        assert_eq!(resolved.worker.selection.kind(), WorkerKind::Simulated);
        assert_eq!(
            resolved
                .worker
                .selection
                .simulation()
                .expect("simulation config")
                .evaluator_statuses,
            vec![QaStatus::Pass]
        );
        assert_eq!(
            resolved.prompts.discovery,
            project_root.join("prompts/discovery.md")
        );
        assert_eq!(
            resolved.prompts.planner,
            project_root.join("prompts/planner.md")
        );
        assert_eq!(
            resolved.schemas.workspace_profile,
            project_root.join("schemas/workspace-profile.json")
        );
        assert_eq!(
            resolved.schemas.builder_handoff,
            project_root.join("schemas/builder.json")
        );
    }

    #[test]
    fn config_defaults_discovery_assets_to_conventional_paths() {
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
runs_dir = ".loopsmith-runs"

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
        assert_eq!(
            resolved.prompts.discovery,
            project_root.join("prompts/discovery.md")
        );
        assert_eq!(
            resolved.schemas.workspace_profile,
            project_root.join("schemas/workspace-profile.json")
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
runs_dir = ".loopsmith-runs"

[workspace]
isolation = "direct"

[worker]
kind = "simulated"

[worker.simulation]
evaluator_statuses = ["pass"]
session_prefix = "sim"

[worker.planner]
kind = "codex_cli"

[worker.planner.codex]
binary = "codex"
model = "o3"
sandbox = "workspace-write"
full_auto = true
skip_git_repo_check = true
resume_sessions = false

[prompts]
discovery = "prompts/discovery.md"
planner = "prompts/planner.md"
builder = "prompts/builder.md"
evaluator = "prompts/evaluator.md"

[schemas]
workspace_profile = "schemas/workspace-profile.json"
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

        let resolved = AppConfig::load(&config_file).expect("load config with planner override");
        let planner = resolved.planner_worker().expect("planner config");
        assert_eq!(planner.selection.kind(), WorkerKind::CodexCli);
        let codex = planner.selection.codex().expect("codex config");
        assert_eq!(codex.model, "o3");
    }
}
