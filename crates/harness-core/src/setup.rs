use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use dialoguer::{Confirm, Input, Select, theme::ColorfulTheme};
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

struct CliOption {
    label: &'static str,
    kind_tag: &'static str,
    binary: &'static str,
    recommended_models: &'static [&'static str],
}

const CLI_OPTIONS: &[CliOption] = &[
    CliOption {
        label: "Codex CLI",
        kind_tag: "codex_cli",
        binary: "codex",
        recommended_models: &[
            "gpt-5.4",
            "gpt-5.4-mini",
            "gpt-5.3-codex",
            "gpt-5.2-codex",
            "gpt-5.2",
            "gpt-5.1-codex-max",
            "gpt-5.1-codex-mini",
        ],
    },
    CliOption {
        label: "Claude Code",
        kind_tag: "claude_cli",
        binary: "claude",
        recommended_models: &[
            "claude-sonnet-4-20250514",
            "claude-opus-4-20250514",
            "claude-sonnet-4.5-20250514",
        ],
    },
    CliOption {
        label: "Gemini CLI",
        kind_tag: "gemini_cli",
        binary: "gemini",
        recommended_models: &[
            "gemini-2.5-pro",
            "gemini-2.5-flash",
        ],
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

    let cli_labels: Vec<String> = CLI_OPTIONS
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

    let selected_cli = &CLI_OPTIONS[cli_index];
    let probe = report.find_tool(selected_cli.kind_tag);

    if let Some(probe) = probe {
        if !probe.is_found() {
            println!();
            println!(
                "  ⚠  {} ({}) was not found on your machine.",
                selected_cli.label, selected_cli.binary
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
        .unwrap_or(selected_cli.binary);

    let selected_model = prompt_model_selection(&theme, selected_cli)?;

    println!();
    println!("  Workspace:    {}", workspace.display());
    println!(
        "  Coding CLI:   {} ({})",
        selected_cli.label, selected_model
    );
    println!("  Binary:       {}", resolved_binary);
    println!("  Prompts:      {}/prompts/ (3 built-in files)", workspace.display());
    println!();

    let confirmed = Confirm::with_theme(&theme)
        .with_prompt("Confirm setup?")
        .default(true)
        .interact()
        .context("failed to read confirmation")?;

    if !confirmed {
        anyhow::bail!("setup cancelled by user");
    }

    let config_content =
        generate_default_config_with_binary(selected_cli, &selected_model, resolved_binary);
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
        worker_kind: selected_cli.kind_tag,
        model: selected_model,
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

fn format_cli_label(opt: &CliOption, report: &EnvironmentReport) -> String {
    let probe = report.find_tool(opt.kind_tag);
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
    for (i, opt) in CLI_OPTIONS.iter().enumerate() {
        if let Some(probe) = report.find_tool(opt.kind_tag) {
            if probe.is_found() && !probe.has_warnings() {
                return i;
            }
        }
    }
    for (i, opt) in CLI_OPTIONS.iter().enumerate() {
        if let Some(probe) = report.find_tool(opt.kind_tag) {
            if probe.is_found() {
                return i;
            }
        }
    }
    0
}

fn prompt_model_selection(theme: &ColorfulTheme, cli: &CliOption) -> Result<String> {
    let mut items: Vec<String> = cli
        .recommended_models
        .iter()
        .enumerate()
        .map(|(i, m)| {
            if i == 0 {
                format!("{m} (recommended)")
            } else {
                m.to_string()
            }
        })
        .collect();
    items.push("Other (enter model name)".to_string());

    let index = Select::with_theme(theme)
        .with_prompt("Select model")
        .items(&items)
        .default(0)
        .interact()
        .context("failed to read model selection")?;

    if index < cli.recommended_models.len() {
        Ok(cli.recommended_models[index].to_string())
    } else {
        let custom: String = Input::with_theme(theme)
            .with_prompt("Enter model name")
            .interact_text()
            .context("failed to read custom model name")?;
        let custom = custom.trim().to_string();
        if custom.is_empty() {
            anyhow::bail!("model name must not be empty");
        }
        Ok(custom)
    }
}

pub fn has_default_config() -> Result<bool> {
    let workspace = home::loopsmith_home()?;
    Ok(workspace.join("config/default.toml").exists())
}

pub fn default_config_path() -> Result<PathBuf> {
    let workspace = home::loopsmith_home()?;
    Ok(workspace.join("config/default.toml"))
}

fn generate_default_config(cli: &CliOption, model: &str) -> String {
    generate_default_config_with_binary(cli, model, cli.binary)
}

fn generate_default_config_with_binary(cli: &CliOption, model: &str, binary: &str) -> String {
    let noop_command = if cfg!(windows) {
        r#"  ["cmd", "/c", "echo", "ok"]"#
    } else {
        r#"  ["/usr/bin/env", "true"]"#
    };

    let worker_section = match cli.kind_tag {
        "codex_cli" => format!(
            r#"[worker]
kind = "codex_cli"

[worker.codex]
binary = "{binary}"
model = "{model}"
sandbox = "workspace-write"
full_auto = true
skip_git_repo_check = true
resume_sessions = true"#,
            binary = binary,
            model = model,
        ),
        "claude_cli" => format!(
            r#"[worker]
kind = "claude_cli"

[worker.claude]
binary = "{binary}"
model = "{model}"
dangerously_skip_permissions = true
resume_sessions = true"#,
            binary = binary,
            model = model,
        ),
        "gemini_cli" => format!(
            r#"[worker]
kind = "gemini_cli"

[worker.gemini]
binary = "{binary}"
model = "{model}"
sandbox = "workspace-write"
resume_sessions = false"#,
            binary = binary,
            model = model,
        ),
        _ => unreachable!(),
    };

    format!(
        r#"[project]
root_dir = ".."

[storage]
runs_dir = ".loopsmith-runs"

[workspace]
isolation = "direct"

{worker_section}

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
"#
    )
}

pub fn write_config_non_interactive(
    workspace: &Path,
    kind_tag: &str,
    binary: &str,
    model: &str,
) -> Result<PathBuf> {
    let cli = CLI_OPTIONS
        .iter()
        .find(|opt| opt.kind_tag == kind_tag)
        .with_context(|| format!("unknown worker kind: {kind_tag}"))?;

    let effective_cli = CliOption {
        label: cli.label,
        kind_tag: cli.kind_tag,
        binary: cli.binary,
        recommended_models: cli.recommended_models,
    };

    let config_content = generate_default_config(&effective_cli, model);
    let config_content = config_content.replace(
        &format!("binary = \"{}\"", cli.binary),
        &format!("binary = \"{binary}\""),
    );
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
    fn generate_codex_config_is_valid_toml() {
        let cli = &CLI_OPTIONS[0];
        let config = generate_default_config(cli, "gpt-5.4");
        let parsed: toml::Value = toml::from_str(&config).expect("valid TOML");
        let worker = parsed.get("worker").expect("worker section");
        assert_eq!(
            worker.get("kind").and_then(|v| v.as_str()),
            Some("codex_cli")
        );
    }

    #[test]
    fn generate_claude_config_is_valid_toml() {
        let cli = &CLI_OPTIONS[1];
        let config = generate_default_config(cli, "claude-sonnet-4-20250514");
        let parsed: toml::Value = toml::from_str(&config).expect("valid TOML");
        let worker = parsed.get("worker").expect("worker section");
        assert_eq!(
            worker.get("kind").and_then(|v| v.as_str()),
            Some("claude_cli")
        );
    }

    #[test]
    fn generate_gemini_config_is_valid_toml() {
        let cli = &CLI_OPTIONS[2];
        let config = generate_default_config(cli, "gemini-2.5-pro");
        let parsed: toml::Value = toml::from_str(&config).expect("valid TOML");
        let worker = parsed.get("worker").expect("worker section");
        assert_eq!(
            worker.get("kind").and_then(|v| v.as_str()),
            Some("gemini_cli")
        );
    }

    #[test]
    fn write_config_non_interactive_creates_file() {
        let temp = tempdir().expect("tempdir");
        let workspace = temp.path().join(".loopsmith");
        fs::create_dir_all(&workspace).expect("workspace");

        let path = write_config_non_interactive(&workspace, "codex_cli", "codex", "gpt-5.4")
            .expect("write");

        assert!(path.exists());
        let content = fs::read_to_string(&path).expect("read");
        assert!(content.contains("codex_cli"));
        assert!(content.contains("gpt-5.4"));
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

        for cli in CLI_OPTIONS {
            let config_content = generate_default_config(cli, cli.recommended_models[0]);
            let config_path = config_dir.join(format!("{}.toml", cli.kind_tag));
            fs::write(&config_path, config_content).expect("write config");

            crate::config::AppConfig::load(&config_path)
                .unwrap_or_else(|e| panic!("failed to load {} config: {e:#}", cli.kind_tag));
        }
    }
}
