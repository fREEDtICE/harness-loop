use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use dialoguer::{Confirm, Select, theme::ColorfulTheme};
use tracing::info;

use crate::{
    env_probe::{self, EnvironmentReport, ToolStatus},
    home,
};

#[derive(Debug, Clone)]
pub struct SetupResult {
    pub workspace: PathBuf,
    pub config_path: PathBuf,
    pub worker_kind: &'static str,
    pub model: String,
}

struct AcpAgentOption {
    label: &'static str,
    kind_tag: &'static str,
    command: &'static [&'static str],
    default_agent_name: &'static str,
}

const ACP_AGENT_OPTIONS: &[AcpAgentOption] = &[
    AcpAgentOption {
        label: "Codex CLI (via ACP)",
        kind_tag: "acp",
        command: &["codex"],
        default_agent_name: "codex",
    },
    AcpAgentOption {
        label: "Claude Code (via ACP)",
        kind_tag: "acp",
        command: &["claude"],
        default_agent_name: "claude",
    },
    AcpAgentOption {
        label: "Gemini CLI (via ACP)",
        kind_tag: "acp",
        command: &["gemini"],
        default_agent_name: "gemini",
    },
];

pub fn run_interactive_setup() -> Result<SetupResult> {
    let workspace = home::ensure_global_home()?;

    println!();
    println!("🔧 Welcome to LoopSmith! Let's set up your workspace.");
    println!();

    println!("🔍 Scanning your environment for CLI tools...");
    println!();

    let report = env_probe::probe_environment();
    print_environment_report(&report);

    let theme = ColorfulTheme::default();

    let cli_labels: Vec<String> = ACP_AGENT_OPTIONS
        .iter()
        .map(|opt| format_cli_label(opt, &report))
        .collect();

    let default_index = find_recommended_default(&report);

    let cli_index = Select::with_theme(&theme)
        .with_prompt("Which coding CLI do you use?")
        .items(&cli_labels)
        .default(default_index)
        .interact()
        .context("failed to read CLI selection")?;

    let selected = &ACP_AGENT_OPTIONS[cli_index];
    let binary_name = selected.command[0];
    let probe = report.find_tool(binary_name);

    if let Some(probe) = probe {
        if !probe.is_found() {
            println!();
            println!(
                "  ⚠  {} ({}) was not found on your machine.",
                selected.label, binary_name
            );
            println!("     You can still proceed, but runs will fail until it is installed.");

            let proceed = Confirm::with_theme(&theme)
                .with_prompt("Continue anyway?")
                .default(false)
                .interact()
                .context("failed to read confirmation")?;

            if !proceed {
                anyhow::bail!("setup cancelled: selected CLI not found");
            }
        } else if probe.has_warnings() {
            println!();
            if let ToolStatus::Found { warnings, .. } = &probe.status {
                for w in warnings {
                    println!("  ⚠  {w}");
                }
            }
        }
    }

    let resolved_binary = probe
        .and_then(|p| p.resolved_path())
        .unwrap_or(binary_name);

    println!();
    println!("  Workspace:    {}", workspace.display());
    println!(
        "  Agent:        {} ({})",
        selected.label, selected.default_agent_name
    );
    println!("  Binary:       {}", resolved_binary);
    println!(
        "  Prompts:      {}/prompts/ (3 built-in files)",
        workspace.display()
    );
    println!();

    let confirmed = Confirm::with_theme(&theme)
        .with_prompt("Confirm setup?")
        .default(true)
        .interact()
        .context("failed to read confirmation")?;

    if !confirmed {
        anyhow::bail!("setup cancelled by user");
    }

    let config_content = generate_acp_config(resolved_binary, selected.default_agent_name);
    let config_path = workspace.join("config/default.toml");
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&config_path, &config_content)
        .with_context(|| format!("failed to write {}", config_path.display()))?;

    info!(path = %config_path.display(), "wrote default config");

    println!();
    println!("✅ Workspace initialized at {}", workspace.display());
    println!("   Config: {}", config_path.display());

    Ok(SetupResult {
        workspace,
        config_path,
        worker_kind: selected.kind_tag,
        model: selected.default_agent_name.to_string(),
    })
}

fn print_environment_report(report: &EnvironmentReport) {
    match &report.node.status {
        env_probe::RuntimeStatus::Found { path, version } => {
            println!("  ✓ Node.js {version} ({path})");
        }
        env_probe::RuntimeStatus::NotFound => {
            println!("  ✗ Node.js not found (required by most CLI tools)");
        }
    }

    for tool in &report.tools {
        match &tool.status {
            ToolStatus::Found {
                path,
                version,
                warnings,
            } => {
                let ver = version.as_deref().unwrap_or("unknown version");
                println!("  ✓ {} — {ver} ({path})", tool.display_name);
                for w in warnings {
                    println!("    ⚠ {w}");
                }
            }
            ToolStatus::NotFound => {
                println!("  ✗ {} — not found", tool.display_name);
            }
        }
    }
    println!();
}

fn format_cli_label(opt: &AcpAgentOption, report: &EnvironmentReport) -> String {
    let probe = report.find_tool(opt.command[0]);
    match probe.map(|p| &p.status) {
        Some(ToolStatus::Found {
            version, warnings, ..
        }) => {
            let ver = version.as_deref().unwrap_or("");
            let suffix = if warnings.is_empty() {
                " ✓".to_string()
            } else {
                " ⚠".to_string()
            };
            if ver.is_empty() {
                format!("{}{suffix}", opt.label)
            } else {
                format!("{} ({ver}){suffix}", opt.label)
            }
        }
        _ => format!("{} (not installed)", opt.label),
    }
}

fn find_recommended_default(report: &EnvironmentReport) -> usize {
    for (i, opt) in ACP_AGENT_OPTIONS.iter().enumerate() {
        if let Some(probe) = report.find_tool(opt.command[0]) {
            if probe.is_found() && !probe.has_warnings() {
                return i;
            }
        }
    }
    for (i, opt) in ACP_AGENT_OPTIONS.iter().enumerate() {
        if let Some(probe) = report.find_tool(opt.command[0]) {
            if probe.is_found() {
                return i;
            }
        }
    }
    0
}

pub fn has_default_config() -> Result<bool> {
    let workspace = home::loopsmith_home()?;
    Ok(workspace.join("config/default.toml").exists())
}

pub fn default_config_path() -> Result<PathBuf> {
    let workspace = home::loopsmith_home()?;
    Ok(workspace.join("config/default.toml"))
}

fn generate_acp_config(binary: &str, agent_name: &str) -> String {
    let noop_command = if cfg!(windows) {
        r#"  ["cmd", "/c", "echo", "ok"]"#
    } else {
        r#"  ["/usr/bin/env", "true"]"#
    };

    format!(
        r#"[project]
root_dir = ".."

[storage]
runs_dir = ".loopsmith-runs"

[workspace]
isolation = "direct"

[worker]
kind = "acp"

[worker.acp]
command = ["{binary}"]
agent_name = "{agent_name}"
resume_sessions = true

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
confirm_before_build = false

[runtime.supervision]
enabled = false
startup_timeout_secs = 30
readiness_poll_interval_ms = 250
shutdown_grace_period_secs = 5

[evaluator]
dimensions = ["product_depth", "correctness", "ux", "operability"]
require_screenshots = false
commands = [
{noop_command}
]
"#,
        binary = binary,
        agent_name = agent_name,
        noop_command = noop_command,
    )
}

pub fn write_config_non_interactive(
    workspace: &Path,
    kind_tag: &str,
    binary: &str,
    agent_name: &str,
) -> Result<PathBuf> {
    if kind_tag != "acp" {
        anyhow::bail!("unsupported worker kind: {kind_tag}");
    }

    let config_content = generate_acp_config(binary, agent_name);
    let config_path = workspace.join("config/default.toml");
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&config_path, &config_content)
        .with_context(|| format!("failed to write {}", config_path.display()))?;

    Ok(config_path)
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn generate_acp_codex_config_is_valid_toml() {
        let config = generate_acp_config("codex", "codex");
        let parsed: toml::Value = toml::from_str(&config).expect("valid TOML");
        let worker = parsed.get("worker").expect("worker section");
        assert_eq!(
            worker.get("kind").and_then(|v| v.as_str()),
            Some("acp")
        );
        let acp = worker.get("acp").expect("acp section");
        assert_eq!(
            acp.get("agent_name").and_then(|v| v.as_str()),
            Some("codex")
        );
    }

    #[test]
    fn generate_acp_claude_config_is_valid_toml() {
        let config = generate_acp_config("claude", "claude");
        let parsed: toml::Value = toml::from_str(&config).expect("valid TOML");
        let worker = parsed.get("worker").expect("worker section");
        assert_eq!(
            worker.get("kind").and_then(|v| v.as_str()),
            Some("acp")
        );
        let acp = worker.get("acp").expect("acp section");
        assert_eq!(
            acp.get("agent_name").and_then(|v| v.as_str()),
            Some("claude")
        );
    }

    #[test]
    fn generate_acp_gemini_config_is_valid_toml() {
        let config = generate_acp_config("gemini", "gemini");
        let parsed: toml::Value = toml::from_str(&config).expect("valid TOML");
        let worker = parsed.get("worker").expect("worker section");
        assert_eq!(
            worker.get("kind").and_then(|v| v.as_str()),
            Some("acp")
        );
        let acp = worker.get("acp").expect("acp section");
        assert_eq!(
            acp.get("agent_name").and_then(|v| v.as_str()),
            Some("gemini")
        );
    }

    #[test]
    fn write_config_non_interactive_creates_file() {
        let temp = tempdir().expect("tempdir");
        let workspace = temp.path().join(".loopsmith");
        fs::create_dir_all(&workspace).expect("workspace");

        let path = write_config_non_interactive(&workspace, "acp", "codex", "codex")
            .expect("write");

        assert!(path.exists());
        let content = fs::read_to_string(&path).expect("read");
        assert!(content.contains("kind = \"acp\""));
        assert!(content.contains("agent_name = \"codex\""));
    }

    #[test]
    fn generated_config_parses_as_app_config() {
        let temp = tempdir().expect("tempdir");
        let project = temp.path();
        let config_dir = project.join("config");
        let prompts_dir = project.join("prompts");
        let schemas_dir = project.join("schemas");
        fs::create_dir_all(&config_dir).expect("config dir");
        fs::create_dir_all(&prompts_dir).expect("prompts dir");
        fs::create_dir_all(&schemas_dir).expect("schemas dir");
        fs::write(prompts_dir.join("planner.md"), "planner\n").expect("planner");
        fs::write(prompts_dir.join("builder.md"), "builder\n").expect("builder");
        fs::write(prompts_dir.join("evaluator.md"), "evaluator\n").expect("evaluator");
        fs::write(schemas_dir.join("planner-output.json"), "{}\n").expect("schema");
        fs::write(schemas_dir.join("builder-handoff.json"), "{}\n").expect("schema");
        fs::write(schemas_dir.join("qa-report.json"), "{}\n").expect("schema");

        for opt in ACP_AGENT_OPTIONS {
            let config_content = generate_acp_config(opt.command[0], opt.default_agent_name);
            let config_path = config_dir.join(format!("{}.toml", opt.default_agent_name));
            fs::write(&config_path, config_content).expect("write config");

            crate::config::AppConfig::load(&config_path)
                .unwrap_or_else(|e| panic!("failed to load {} config: {e:#}", opt.default_agent_name));
        }
    }
}
