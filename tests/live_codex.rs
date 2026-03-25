use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde_json::Value;
use tempfile::tempdir;

#[test]
fn live_codex_harness_round_trip_passes_when_enabled() -> Result<(), Box<dyn Error>> {
    if std::env::var("CODEX_LIVE_E2E").ok().as_deref() != Some("1") {
        eprintln!("skipping live Codex E2E; set CODEX_LIVE_E2E=1 to enable");
        return Ok(());
    }

    let temp = tempdir()?;
    let workspace = temp.path().join("workspace");
    let runs_dir = temp.path().join("runs");
    let request_file = temp.path().join("request.md");
    let config_path = temp.path().join("live.toml");

    fs::create_dir_all(&workspace)?;
    fs::create_dir_all(&runs_dir)?;
    fs::write(
        &request_file,
        "Create a README.md file in the workspace containing exactly this line:\nlive codex round-trip\n",
    )?;
    fs::write(
        &config_path,
        format!(
            r#"
[project]
root_dir = "{}"

[storage]
runs_dir = "{}"

[workspace]
isolation = "direct"

[worker]
kind = "codex_cli"

[worker.codex]
binary = "codex"
model = "gpt-5.4"
sandbox = "workspace-write"
full_auto = true
skip_git_repo_check = true
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
services = []

[runtime.supervision]
enabled = false
startup_timeout_secs = 30
readiness_poll_interval_ms = 250
shutdown_grace_period_secs = 5

[evaluator]
dimensions = ["correctness"]
require_screenshots = false
commands = [
  ["/bin/sh", "-c", "test -f README.md && grep -Fx 'live codex round-trip' README.md"]
]
"#,
            escape_toml_string(&manifest_dir()),
            escape_toml_string(&runs_dir),
        ),
    )?;

    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness-rs"))
        .current_dir(manifest_dir())
        .arg("run")
        .arg("--config")
        .arg(&config_path)
        .arg("--workspace")
        .arg(&workspace)
        .arg("--request-file")
        .arg(&request_file)
        .output()?;

    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout)?;
    let run_root =
        PathBuf::from(line_value(&stdout, "run_root: ").expect("missing run_root in CLI output"));
    let state = read_json(&run_root.join("run-state.json"))?;

    assert_eq!(line_value(&stdout, "lifecycle: "), Some("passed"));
    assert_eq!(line_value(&stdout, "final_status: "), Some("pass"));
    assert_eq!(state["lifecycle"], "passed");
    assert_eq!(state["final_status"], "pass");
    assert_eq!(
        fs::read_to_string(workspace.join("README.md"))?,
        "live codex round-trip\n"
    );

    Ok(())
}

#[test]
fn live_codex_planner_override_passes_when_enabled() -> Result<(), Box<dyn Error>> {
    if std::env::var("CODEX_LIVE_E2E").ok().as_deref() != Some("1") {
        eprintln!("skipping live Codex planner E2E; set CODEX_LIVE_E2E=1 to enable");
        return Ok(());
    }

    let temp = tempdir()?;
    let workspace = temp.path().join("workspace");
    let runs_dir = temp.path().join("runs");
    let request_file = temp.path().join("request.md");
    let config_path = temp.path().join("live-planner.toml");

    fs::create_dir_all(&workspace)?;
    fs::create_dir_all(&runs_dir)?;
    fs::write(
        &request_file,
        "Plan one bounded implementation slice for improving a Rust CLI harness.\n",
    )?;
    fs::write(
        &config_path,
        format!(
            r#"
[project]
root_dir = "{}"

[storage]
runs_dir = "{}"

[workspace]
isolation = "direct"

[worker]
kind = "simulated"

[worker.simulation]
evaluator_statuses = ["pass"]
session_prefix = "simulated"

[worker.planner]
kind = "codex_cli"

[worker.planner.codex]
binary = "codex"
model = "gpt-5.4"
sandbox = "workspace-write"
full_auto = true
skip_git_repo_check = true
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
max_repair_attempts = 0
services = []

[runtime.supervision]
enabled = false
startup_timeout_secs = 30
readiness_poll_interval_ms = 250
shutdown_grace_period_secs = 5

[evaluator]
dimensions = ["correctness"]
require_screenshots = false
commands = [
  ["/usr/bin/env", "true"]
]
"#,
            escape_toml_string(&manifest_dir()),
            escape_toml_string(&runs_dir),
        ),
    )?;

    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness-rs"))
        .current_dir(manifest_dir())
        .arg("run")
        .arg("--config")
        .arg(&config_path)
        .arg("--workspace")
        .arg(&workspace)
        .arg("--request-file")
        .arg(&request_file)
        .output()?;

    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout)?;
    let run_root =
        PathBuf::from(line_value(&stdout, "run_root: ").expect("missing run_root in CLI output"));
    let state = read_json(&run_root.join("run-state.json"))?;
    let plan = read_json(&run_root.join("plan.json"))?;

    assert_eq!(line_value(&stdout, "lifecycle: "), Some("passed"));
    assert_eq!(state["lifecycle"], "passed");
    assert_eq!(state["plan_stage"]["status"], "executed");
    assert_eq!(
        plan["features"]
            .as_array()
            .expect("features should be an array")
            .len(),
        1
    );
    assert!(
        plan["goal"]
            .as_str()
            .is_some_and(|goal| !goal.trim().is_empty())
    );

    Ok(())
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_json(path: &Path) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn line_value<'a>(stdout: &'a str, prefix: &str) -> Option<&'a str> {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix(prefix).map(str::trim))
}

fn escape_toml_string(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}
