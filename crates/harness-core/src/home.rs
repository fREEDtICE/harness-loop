use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use tracing::info;

const LOOPSMITH_DIR: &str = ".loopsmith";

const TEMPLATE_CONFIG: &str = include_str!("../../../config/example.toml");
const TEMPLATE_PLANNER: &str = include_str!("../../../prompts/planner.md");
const TEMPLATE_BUILDER: &str = include_str!("../../../prompts/builder.md");
const TEMPLATE_EVALUATOR: &str = include_str!("../../../prompts/evaluator.md");

const SCHEMA_PLANNER_OUTPUT: &str = include_str!("../../../schemas/planner-output.json");
const SCHEMA_BUILDER_HANDOFF: &str = include_str!("../../../schemas/builder-handoff.json");
const SCHEMA_QA_REPORT: &str = include_str!("../../../schemas/qa-report.json");

struct TemplateFile {
    relative_path: &'static str,
    content: TemplateContent,
}

enum TemplateContent {
    Static(&'static str),
    PlatformAdaptive(fn() -> String),
}

fn platform_config_template() -> String {
    let noop_command = if cfg!(windows) {
        r#"  ["cmd", "/c", "echo", "ok"]"#
    } else {
        r#"  ["/usr/bin/env", "true"]"#
    };

    TEMPLATE_CONFIG.replace(
        r#"  ["/usr/bin/env", "true"]"#,
        noop_command,
    )
}

const GLOBAL_TEMPLATES: &[TemplateFile] = &[
    TemplateFile {
        relative_path: "config/default.toml",
        content: TemplateContent::PlatformAdaptive(platform_config_template),
    },
    TemplateFile {
        relative_path: "prompts/planner.md",
        content: TemplateContent::Static(TEMPLATE_PLANNER),
    },
    TemplateFile {
        relative_path: "prompts/builder.md",
        content: TemplateContent::Static(TEMPLATE_BUILDER),
    },
    TemplateFile {
        relative_path: "prompts/evaluator.md",
        content: TemplateContent::Static(TEMPLATE_EVALUATOR),
    },
    TemplateFile {
        relative_path: "schemas/planner-output.json",
        content: TemplateContent::Static(SCHEMA_PLANNER_OUTPUT),
    },
    TemplateFile {
        relative_path: "schemas/builder-handoff.json",
        content: TemplateContent::Static(SCHEMA_BUILDER_HANDOFF),
    },
    TemplateFile {
        relative_path: "schemas/qa-report.json",
        content: TemplateContent::Static(SCHEMA_QA_REPORT),
    },
];

/// Returns the global LoopSmith home directory.
///
/// Resolution order:
/// 1. `LOOPSMITH_HOME` environment variable (if set and non-empty)
/// 2. `~/.loopsmith` (default)
pub fn loopsmith_home() -> Result<PathBuf> {
    if let Ok(override_path) = std::env::var("LOOPSMITH_HOME") {
        if !override_path.is_empty() {
            return Ok(PathBuf::from(override_path));
        }
    }
    let home = dirs::home_dir().context("unable to determine home directory")?;
    Ok(home.join(LOOPSMITH_DIR))
}

/// Returns the path to the SQLite database inside the global home.
pub fn loopsmith_db_path() -> Result<PathBuf> {
    Ok(loopsmith_home()?.join("loopsmith.db"))
}

/// Returns the path to the global default config template.
pub fn global_config_path() -> Result<PathBuf> {
    Ok(loopsmith_home()?.join("config/default.toml"))
}

/// Ensures the global `~/.loopsmith` directory exists with all template files.
/// Writes any missing template files without overwriting existing ones.
/// Returns the path to the global home.
pub fn ensure_global_home() -> Result<PathBuf> {
    let home = loopsmith_home()?;

    if !home.exists() {
        info!(path = %home.display(), "initializing LoopSmith global home");
        write_templates(&home, GLOBAL_TEMPLATES)?;
        return Ok(home);
    }

    for template in GLOBAL_TEMPLATES {
        let target = home.join(template.relative_path);
        if !target.exists() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).with_context(|| {
                    format!("failed to create directory {}", parent.display())
                })?;
            }
            let content = match &template.content {
                TemplateContent::Static(s) => (*s).to_string(),
                TemplateContent::PlatformAdaptive(f) => f(),
            };
            fs::write(&target, content).with_context(|| {
                format!("failed to write template {}", target.display())
            })?;
            info!(file = %target.display(), "wrote missing template");
        }
    }

    Ok(home)
}

/// Explicitly initialize global home (errors if already exists).
pub fn init_global_home_explicit() -> Result<PathBuf> {
    let home = loopsmith_home()?;

    if home.exists() {
        bail!(
            "workspace already exists at {}. Use --force to reinitialize.",
            home.display()
        );
    }

    write_templates(&home, GLOBAL_TEMPLATES)?;
    Ok(home)
}

/// Force-reinitialize global home (overwrites existing files).
pub fn init_global_home_force() -> Result<PathBuf> {
    let home = loopsmith_home()?;
    write_templates(&home, GLOBAL_TEMPLATES)?;
    Ok(home)
}

/// Returns the workspace-local LoopSmith directory for the given project path.
///
/// ```text
/// {workspace}/.loopsmith/
/// ├── config.toml
/// ├── prompts/
/// │   ├── planner.md
/// │   ├── builder.md
/// │   └── evaluator.md
/// └── schemas/
///     ├── planner-output.json
///     ├── builder-handoff.json
///     └── qa-report.json
/// ```
pub fn workspace_loopsmith_dir(workspace_path: &Path) -> PathBuf {
    workspace_path.join(LOOPSMITH_DIR)
}

/// Returns the workspace-local config file path.
pub fn workspace_config_path(workspace_path: &Path) -> PathBuf {
    workspace_loopsmith_dir(workspace_path).join("config.toml")
}

/// Ensures the workspace-local `.loopsmith/` directory is initialized.
///
/// If the config file does not exist, copies all configuration files from the
/// global `~/.loopsmith` directory. If files already exist but some templates
/// are missing (e.g. schemas/), writes them from embedded defaults.
///
/// Returns the path to the workspace-local config file.
pub fn ensure_workspace_config(workspace_path: &Path) -> Result<PathBuf> {
    let ws_dir = workspace_loopsmith_dir(workspace_path);
    let ws_config = ws_dir.join("config.toml");

    if !ws_config.exists() {
        info!(workspace = %workspace_path.display(), "initializing workspace-local .loopsmith");
        let global_home = ensure_global_home()?;
        copy_global_to_workspace(&global_home, &ws_dir)?;
        return Ok(ws_config);
    }

    ensure_workspace_templates(&ws_dir)?;
    Ok(ws_config)
}

/// Writes any missing template files (prompts, schemas) into an existing workspace .loopsmith/ dir.
fn ensure_workspace_templates(ws_dir: &Path) -> Result<()> {
    let expected: &[(&str, &str)] = &[
        ("prompts/planner.md", TEMPLATE_PLANNER),
        ("prompts/builder.md", TEMPLATE_BUILDER),
        ("prompts/evaluator.md", TEMPLATE_EVALUATOR),
        ("schemas/planner-output.json", SCHEMA_PLANNER_OUTPUT),
        ("schemas/builder-handoff.json", SCHEMA_BUILDER_HANDOFF),
        ("schemas/qa-report.json", SCHEMA_QA_REPORT),
    ];

    for (rel_path, content) in expected {
        let target = ws_dir.join(rel_path);
        if !target.exists() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).with_context(|| {
                    format!("failed to create directory {}", parent.display())
                })?;
            }
            fs::write(&target, content).with_context(|| {
                format!("failed to write {}", target.display())
            })?;
            info!(file = %target.display(), "wrote missing workspace template");
        }
    }
    Ok(())
}

/// Copies config, prompts, and schemas from the global home to a workspace-local directory.
fn copy_global_to_workspace(global_home: &Path, ws_dir: &Path) -> Result<()> {
    fs::create_dir_all(ws_dir)
        .with_context(|| format!("failed to create {}", ws_dir.display()))?;

    let global_config = global_home.join("config/default.toml");
    let ws_config = ws_dir.join("config.toml");

    if global_config.exists() {
        let content = fs::read_to_string(&global_config)
            .with_context(|| format!("failed to read {}", global_config.display()))?;

        let adjusted = adjust_config_for_workspace(&content);
        fs::write(&ws_config, adjusted)
            .with_context(|| format!("failed to write {}", ws_config.display()))?;
        info!(file = %ws_config.display(), "wrote workspace config");
    } else {
        let content = platform_config_template();
        let adjusted = adjust_config_for_workspace(&content);
        fs::write(&ws_config, adjusted)
            .with_context(|| format!("failed to write {}", ws_config.display()))?;
        info!(file = %ws_config.display(), "wrote workspace config from embedded template");
    }

    copy_dir_contents(
        &global_home.join("prompts"),
        &ws_dir.join("prompts"),
        &["planner.md", "builder.md", "evaluator.md"],
    )?;

    copy_dir_contents(
        &global_home.join("schemas"),
        &ws_dir.join("schemas"),
        &[
            "planner-output.json",
            "builder-handoff.json",
            "qa-report.json",
        ],
    )?;

    Ok(())
}

/// Adjusts a global config template for workspace-local use.
///
/// The key adjustment: `root_dir` is set to `".."` so that from
/// `{workspace}/.loopsmith/config.toml`, it points to `{workspace}/`.
/// Prompt and schema paths are relative to the project root, which is `{workspace}/`,
/// so they become `.loopsmith/prompts/...` and `.loopsmith/schemas/...`.
fn adjust_config_for_workspace(content: &str) -> String {
    let mut result = content.to_string();

    result = result.replace(
        "root_dir = \"..\"",
        "root_dir = \"..\"",
    );

    result = result.replace(
        "planner = \"prompts/planner.md\"",
        "planner = \".loopsmith/prompts/planner.md\"",
    );
    result = result.replace(
        "builder = \"prompts/builder.md\"",
        "builder = \".loopsmith/prompts/builder.md\"",
    );
    result = result.replace(
        "evaluator = \"prompts/evaluator.md\"",
        "evaluator = \".loopsmith/prompts/evaluator.md\"",
    );

    result = result.replace(
        "planner_output = \"schemas/planner-output.json\"",
        "planner_output = \".loopsmith/schemas/planner-output.json\"",
    );
    result = result.replace(
        "builder_handoff = \"schemas/builder-handoff.json\"",
        "builder_handoff = \".loopsmith/schemas/builder-handoff.json\"",
    );
    result = result.replace(
        "qa_report = \"schemas/qa-report.json\"",
        "qa_report = \".loopsmith/schemas/qa-report.json\"",
    );

    result
}

fn copy_dir_contents(src_dir: &Path, dst_dir: &Path, files: &[&str]) -> Result<()> {
    fs::create_dir_all(dst_dir)
        .with_context(|| format!("failed to create {}", dst_dir.display()))?;

    for file in files {
        let src = src_dir.join(file);
        let dst = dst_dir.join(file);
        if src.exists() {
            fs::copy(&src, &dst).with_context(|| {
                format!("failed to copy {} → {}", src.display(), dst.display())
            })?;
            info!(file = %dst.display(), "copied template");
        }
    }
    Ok(())
}

fn write_templates(target_dir: &Path, templates: &[TemplateFile]) -> Result<()> {
    for template in templates {
        let target = target_dir.join(template.relative_path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create directory {}", parent.display())
            })?;
        }
        let content = match &template.content {
            TemplateContent::Static(s) => (*s).to_string(),
            TemplateContent::PlatformAdaptive(f) => f(),
        };
        fs::write(&target, content).with_context(|| {
            format!("failed to write template {}", target.display())
        })?;
        info!(file = %target.display(), "wrote template");
    }

    Ok(())
}

const GLOBAL_PATCH_KEYS: &[&str] = &["worker", "workspace"];
const RUNTIME_PATCH_KEYS: &[&str] = &["feature_limit", "max_repair_attempts", "continue_after_failure"];

/// Patches a workspace config with values from the global config.
///
/// Overwrites `[worker]` and `[workspace]` sections entirely.
/// For `[runtime]`, only patches `feature_limit`, `max_repair_attempts`, and
/// `continue_after_failure` — preserving workspace-specific sub-tables like
/// `[runtime.supervision]`, `[[runtime.services]]`, and `[[runtime.stacks]]`.
pub fn patch_workspace_from_global(workspace_path: &Path, global_toml: &str) -> Result<()> {
    let ws_config_path = workspace_config_path(workspace_path);
    if !ws_config_path.exists() {
        return Ok(());
    }

    let ws_content = fs::read_to_string(&ws_config_path)
        .with_context(|| format!("failed to read {}", ws_config_path.display()))?;

    let global: toml::Table = toml::from_str(global_toml)
        .context("failed to parse global config as TOML")?;
    let mut ws: toml::Table = toml::from_str(&ws_content)
        .with_context(|| format!("failed to parse {}", ws_config_path.display()))?;

    for &key in GLOBAL_PATCH_KEYS {
        if let Some(value) = global.get(key) {
            ws.insert(key.to_string(), value.clone());
        }
    }

    if let Some(toml::Value::Table(global_runtime)) = global.get("runtime") {
        let ws_runtime = ws
            .entry("runtime")
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        if let toml::Value::Table(ws_rt) = ws_runtime {
            for &key in RUNTIME_PATCH_KEYS {
                if let Some(value) = global_runtime.get(key) {
                    ws_rt.insert(key.to_string(), value.clone());
                }
            }
        }
    }

    let output = toml::to_string_pretty(&ws)
        .context("failed to serialize patched workspace config")?;
    fs::write(&ws_config_path, output)
        .with_context(|| format!("failed to write {}", ws_config_path.display()))?;

    info!(workspace = %workspace_path.display(), "patched workspace config from global");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn write_global_templates_creates_all_files() {
        let temp = tempdir().expect("tempdir");
        let home = temp.path().join(".loopsmith");

        write_templates(&home, GLOBAL_TEMPLATES).expect("init");

        assert!(home.join("config/default.toml").exists());
        assert!(home.join("prompts/planner.md").exists());
        assert!(home.join("prompts/builder.md").exists());
        assert!(home.join("prompts/evaluator.md").exists());
        assert!(home.join("schemas/planner-output.json").exists());
        assert!(home.join("schemas/builder-handoff.json").exists());
        assert!(home.join("schemas/qa-report.json").exists());

        let config = fs::read_to_string(home.join("config/default.toml")).expect("read");
        assert!(config.contains("[project]"));
    }

    #[test]
    fn write_global_templates_is_idempotent() {
        let temp = tempdir().expect("tempdir");
        let home = temp.path().join(".loopsmith");

        write_templates(&home, GLOBAL_TEMPLATES).expect("first init");
        write_templates(&home, GLOBAL_TEMPLATES).expect("second init");

        assert!(home.join("config/default.toml").exists());
    }

    #[test]
    fn copy_global_to_workspace_creates_correct_structure() {
        let temp = tempdir().expect("tempdir");
        let global_home = temp.path().join("global");
        write_templates(&global_home, GLOBAL_TEMPLATES).expect("init global");

        let ws_dir = temp.path().join("project/.loopsmith");
        copy_global_to_workspace(&global_home, &ws_dir).expect("copy");

        assert!(ws_dir.join("config.toml").exists());
        assert!(ws_dir.join("prompts/planner.md").exists());
        assert!(ws_dir.join("prompts/builder.md").exists());
        assert!(ws_dir.join("prompts/evaluator.md").exists());
        assert!(ws_dir.join("schemas/planner-output.json").exists());
        assert!(ws_dir.join("schemas/builder-handoff.json").exists());
        assert!(ws_dir.join("schemas/qa-report.json").exists());

        let config = fs::read_to_string(ws_dir.join("config.toml")).expect("read config");
        assert!(config.contains("root_dir = \"..\""));
        assert!(config.contains(".loopsmith/prompts/planner.md"));
        assert!(config.contains(".loopsmith/schemas/planner-output.json"));
        assert!(!config.contains("planner = \"prompts/planner.md\""));
    }

    #[test]
    fn adjust_config_updates_prompt_and_schema_paths() {
        let input = r#"
[prompts]
planner = "prompts/planner.md"
builder = "prompts/builder.md"
evaluator = "prompts/evaluator.md"

[schemas]
planner_output = "schemas/planner-output.json"
builder_handoff = "schemas/builder-handoff.json"
qa_report = "schemas/qa-report.json"
"#;
        let output = adjust_config_for_workspace(input);
        assert!(output.contains(".loopsmith/prompts/planner.md"));
        assert!(output.contains(".loopsmith/prompts/builder.md"));
        assert!(output.contains(".loopsmith/prompts/evaluator.md"));
        assert!(output.contains(".loopsmith/schemas/planner-output.json"));
        assert!(output.contains(".loopsmith/schemas/builder-handoff.json"));
        assert!(output.contains(".loopsmith/schemas/qa-report.json"));
    }
}
