use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde_json::Value;
use tempfile::TempDir;

#[test]
fn simulated_cli_discovery_writes_workspace_profile_before_run_loop() -> Result<(), Box<dyn Error>>
{
    let fixture = DiscoveryFixture::new()?;
    let output = fixture.run("Exercise discovery before the harness loop.\n")?;
    fixture.assert_success(&output)?;

    let run_root = fixture.single_run_root()?;
    let discovery_root = fixture.workspace_dir.join(".loopsmith/discovery");
    assert!(discovery_root.join("scan.json").exists());
    assert!(discovery_root.join("profile.json").exists());
    assert!(discovery_root.join("status.json").exists());
    assert!(run_root.join("inputs/workspace-profile.json").exists());

    let state = read_json(&run_root.join("run-state.json"))?;
    let launch = read_json(&run_root.join("launch.json"))?;
    let status = read_json(&discovery_root.join("status.json"))?;
    let profile = read_json(&discovery_root.join("profile.json"))?;
    let snapshot = read_json(&run_root.join("inputs/workspace-profile.json"))?;

    assert_eq!(state["lifecycle"], "passed");
    assert_eq!(status["last_refresh_error"], Value::Null);
    assert_eq!(status["used_fallback_profile"], Value::Bool(false));
    assert_eq!(launch["workspace_profile"]["used_fallback_profile"], false);
    assert_eq!(
        launch["workspace_profile"]["snapshot_path"],
        snapshot_path_value(&run_root.join("inputs/workspace-profile.json"))
    );
    assert_eq!(snapshot["summary"], profile["summary"]);
    assert!(
        profile["summary"]
            .as_str()
            .unwrap_or_default()
            .contains("Workspace profile derived from")
    );

    Ok(())
}

#[test]
fn cli_discover_uses_fallback_profile_when_live_refresh_produces_no_output()
-> Result<(), Box<dyn Error>> {
    let fixture = DiscoveryFixture::new()?;
    let initial_output = fixture.discover()?;
    fixture.assert_success(&initial_output)?;

    let initial_profile = read_json(
        &fixture
            .workspace_dir
            .join(".loopsmith/discovery/profile.json"),
    )?;
    fs::write(
        fixture.workspace_dir.join("package.json"),
        r#"{"name":"discovery-fixture","version":"0.2.0","scripts":{"test":"vitest","dev":"vite"},"dependencies":{"react":"18.3.0","zod":"3.23.8"}}"#,
    )?;

    let fake_codex =
        fixture.write_fake_codex_binary("fake-codex-no-output", "#!/bin/sh\nexit 0\n")?;
    let codex_config = fixture.write_codex_config("codex-missing-output.toml", &fake_codex)?;
    let output = fixture.discover_with_config(&codex_config)?;
    fixture.assert_success(&output)?;

    let discovery_root = fixture.workspace_dir.join(".loopsmith/discovery");
    let status = read_json(&discovery_root.join("status.json"))?;
    let profile = read_json(&discovery_root.join("profile.json"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(status["current_phase"], "using_fallback_profile");
    assert_eq!(status["used_fallback_profile"], Value::Bool(true));
    assert!(
        status["last_refresh_error"]
            .as_str()
            .unwrap_or_default()
            .contains("failed to read")
    );
    assert_eq!(profile["summary"], initial_profile["summary"]);
    assert!(stdout.contains("phase: using_fallback_profile"));
    assert!(stdout.contains("used_fallback_profile: true"));
    assert!(!discovery_root.join("worker/result.json").exists());

    Ok(())
}

#[test]
fn simulated_cli_discover_command_refreshes_workspace_profile_without_creating_a_run()
-> Result<(), Box<dyn Error>> {
    let fixture = DiscoveryFixture::new()?;
    let output = fixture.discover()?;
    fixture.assert_success(&output)?;

    let discovery_root = fixture.workspace_dir.join(".loopsmith/discovery");
    assert!(discovery_root.join("scan.json").exists());
    assert!(discovery_root.join("profile.json").exists());
    assert!(discovery_root.join("status.json").exists());
    assert!(fixture.run_roots()?.is_empty());

    let status = read_json(&discovery_root.join("status.json"))?;
    let profile = read_json(&discovery_root.join("profile.json"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(status["current_phase"], "ready");
    assert_eq!(status["last_refresh_error"], Value::Null);
    assert_eq!(status["used_fallback_profile"], Value::Bool(false));
    assert!(
        profile["summary"]
            .as_str()
            .unwrap_or_default()
            .contains("Workspace profile derived from")
    );
    assert!(stdout.contains("phase: ready"));
    assert!(stdout.contains("profile:"));
    assert!(stdout.contains("status:"));
    assert!(stdout.contains("summary:"));
    assert!(stdout.contains("source_files:"));
    assert!(!stdout.contains("run_root:"));

    Ok(())
}

#[test]
fn simulated_cli_discover_persists_evidence_and_inference_artifacts() -> Result<(), Box<dyn Error>>
{
    let fixture = DiscoveryFixture::new()?;
    let output = fixture.discover()?;
    fixture.assert_success(&output)?;

    let discovery_root = fixture.workspace_dir.join(".loopsmith/discovery");
    assert!(discovery_root.join("evidence.json").exists());
    assert!(discovery_root.join("inference.json").exists());

    let evidence = read_json(&discovery_root.join("evidence.json"))?;
    let inference = read_json(&discovery_root.join("inference.json"))?;
    let inference_items = inference["inferences"].as_array().expect("inference items");

    assert!(evidence["source_files"].as_array().is_some());
    assert!(!inference_items.is_empty());
    for item in inference_items {
        let confidence = item["confidence"].as_u64().expect("confidence");
        assert!((1..=10).contains(&confidence));
        if confidence == 10 {
            let chains = item["evidence_chains"].as_array().expect("chains");
            assert_eq!(chains.len(), 1);
            assert_eq!(chains[0]["strength"], "strong");
        }
    }

    Ok(())
}

struct DiscoveryFixture {
    _temp: TempDir,
    project_root: PathBuf,
    config_path: PathBuf,
    request_file: PathBuf,
    workspace_dir: PathBuf,
    runs_dir: PathBuf,
    loopsmith_home: PathBuf,
}

impl DiscoveryFixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let project_root = temp.path().to_path_buf();
        let config_dir = project_root.join("config");
        let prompts_dir = project_root.join("prompts");
        let schemas_dir = project_root.join("schemas");
        let workspace_dir = project_root.join("workspace");
        let runs_dir = project_root.join(".loopsmith-runs");
        let loopsmith_home = project_root.join(".loopsmith-home");

        for dir in [
            &config_dir,
            &prompts_dir,
            &schemas_dir,
            &workspace_dir,
            &runs_dir,
            &loopsmith_home,
            &workspace_dir.join("src"),
        ] {
            fs::create_dir_all(dir)?;
        }

        fs::write(prompts_dir.join("discovery.md"), "discovery\n")?;
        fs::write(prompts_dir.join("planner.md"), "planner\n")?;
        fs::write(prompts_dir.join("builder.md"), "builder\n")?;
        fs::write(prompts_dir.join("evaluator.md"), "evaluator\n")?;
        fs::write(
            schemas_dir.join("workspace-profile.json"),
            "{\"type\":\"object\"}\n",
        )?;
        fs::write(
            schemas_dir.join("planner-output.json"),
            "{\"type\":\"object\"}\n",
        )?;
        fs::write(
            schemas_dir.join("builder-handoff.json"),
            "{\"type\":\"object\"}\n",
        )?;
        fs::write(
            schemas_dir.join("qa-report.json"),
            "{\"type\":\"object\"}\n",
        )?;

        fs::write(
            workspace_dir.join("Cargo.toml"),
            "[package]\nname = \"discovery-fixture\"\nversion = \"0.1.0\"\n[dependencies]\nserde = \"1\"\n",
        )?;
        fs::write(workspace_dir.join("src/lib.rs"), "pub fn fixture() {}\n")?;
        fs::write(
            workspace_dir.join("package.json"),
            r#"{"name":"discovery-fixture","scripts":{"test":"vitest","dev":"vite"},"dependencies":{"react":"18.3.0"}}"#,
        )?;
        fs::write(
            workspace_dir.join("Makefile"),
            "build:\n\tcargo build\n\ntest:\n\tcargo test\n",
        )?;
        fs::write(
            workspace_dir.join(".editorconfig"),
            "root = true\n[*]\nindent_style = space\n",
        )?;

        let config_path = config_dir.join("harness.toml");
        fs::write(
            &config_path,
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
session_prefix = "simulated"

[prompts]
discovery = "prompts/discovery.md"
planner = "prompts/planner.md"
builder = "prompts/builder.md"
evaluator = "prompts/evaluator.md"

[schemas]
workspace_profile = "schemas/workspace-profile.json"
planner_output = "schemas/planner-output.json"
builder_handoff = "schemas/builder-handoff.json"
qa_report = "schemas/qa-report.json"

[runtime]
feature_limit = 1
max_repair_attempts = 1
continue_after_failure = false
services = []
stacks = []

[evaluator]
dimensions = ["correctness"]
require_screenshots = false
commands = [
  ["/usr/bin/env", "true"]
]
"#,
        )?;

        Ok(Self {
            _temp: temp,
            project_root: project_root.clone(),
            config_path,
            request_file: project_root.join("request.md"),
            workspace_dir,
            runs_dir,
            loopsmith_home,
        })
    }

    fn run(&self, request: &str) -> Result<Output, Box<dyn Error>> {
        fs::write(&self.request_file, request)?;

        Ok(Command::new(env!("CARGO_BIN_EXE_loopsmith"))
            .current_dir(&self.project_root)
            .env("LOOPSMITH_HOME", &self.loopsmith_home)
            .arg("run")
            .arg("--config")
            .arg(&self.config_path)
            .arg("--workspace")
            .arg(&self.workspace_dir)
            .arg("--request-file")
            .arg(&self.request_file)
            .output()?)
    }

    fn discover(&self) -> Result<Output, Box<dyn Error>> {
        self.discover_with_config(&self.config_path)
    }

    fn discover_with_config(&self, config_path: &Path) -> Result<Output, Box<dyn Error>> {
        Ok(Command::new(env!("CARGO_BIN_EXE_loopsmith"))
            .current_dir(&self.project_root)
            .env("LOOPSMITH_HOME", &self.loopsmith_home)
            .arg("discover")
            .arg("--config")
            .arg(config_path)
            .arg("--workspace")
            .arg(&self.workspace_dir)
            .output()?)
    }

    fn write_codex_config(
        &self,
        file_name: &str,
        binary_path: &Path,
    ) -> Result<PathBuf, Box<dyn Error>> {
        let config_path = self.project_root.join("config").join(file_name);
        fs::write(
            &config_path,
            format!(
                r#"
[project]
root_dir = ".."

[storage]
runs_dir = ".loopsmith-runs"

[workspace]
isolation = "direct"

[worker]
kind = "codex_cli"

[worker.codex]
binary = "{binary}"
model = "gpt-5.4"
sandbox = "workspace-write"
full_auto = false
resume_sessions = false
skip_git_repo_check = true

[prompts]
discovery = "prompts/discovery.md"
planner = "prompts/planner.md"
builder = "prompts/builder.md"
evaluator = "prompts/evaluator.md"

[schemas]
workspace_profile = "schemas/workspace-profile.json"
planner_output = "schemas/planner-output.json"
builder_handoff = "schemas/builder-handoff.json"
qa_report = "schemas/qa-report.json"

[runtime]
feature_limit = 1
max_repair_attempts = 1
continue_after_failure = false
services = []
stacks = []

[evaluator]
dimensions = ["correctness"]
require_screenshots = false
commands = [
  ["/usr/bin/env", "true"]
]
"#,
                binary = binary_path.display()
            ),
        )?;
        Ok(config_path)
    }

    fn write_fake_codex_binary(
        &self,
        file_name: &str,
        contents: &str,
    ) -> Result<PathBuf, Box<dyn Error>> {
        let binary_path = self.project_root.join(file_name);
        fs::write(&binary_path, contents)?;
        make_executable(&binary_path)?;
        Ok(binary_path)
    }

    fn single_run_root(&self) -> Result<PathBuf, Box<dyn Error>> {
        let mut run_roots = self.run_roots()?;
        assert_eq!(
            run_roots.len(),
            1,
            "expected exactly one run root under {}",
            self.runs_dir.display()
        );
        Ok(run_roots.remove(0))
    }

    fn run_roots(&self) -> Result<Vec<PathBuf>, Box<dyn Error>> {
        let mut run_roots = fs::read_dir(&self.runs_dir)?
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                entry
                    .file_type()
                    .ok()
                    .filter(|file_type| file_type.is_dir())
                    .map(|_| entry.path())
            })
            .collect::<Vec<_>>();
        run_roots.sort();
        Ok(run_roots)
    }

    fn assert_success(&self, output: &Output) -> Result<(), Box<dyn Error>> {
        assert!(
            output.status.success(),
            "stdout:\n{}\n\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    }
}

fn read_json(path: &Path) -> Result<Value, Box<dyn Error>> {
    let bytes = fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn snapshot_path_value(path: &Path) -> Value {
    Value::String(path.to_string_lossy().into_owned())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), Box<dyn Error>> {
    Ok(())
}
