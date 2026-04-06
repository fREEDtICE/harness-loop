use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::Duration,
};

use loopsmith_core::discovery::{
    WorkspaceDiscoveryPhase, WorkspaceDiscoveryRequest, WorkspaceDiscoveryStatus,
    WorkspaceDiscoveryStore, profile_fingerprint, scan_workspace,
};
use serde_json::{Value, json};
use tempfile::TempDir;

#[test]
fn simulated_cli_happy_path_creates_consistent_artifacts() -> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new(&["pass"], 1)?;
    let output = fixture.run(
        "Build a harness run and persist all stage artifacts for inspection.\n",
        None,
    )?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_root_layout()?;
    run.assert_stage_count(3)?;
    run.assert_cli_field("lifecycle", "passed")?;
    run.assert_cli_field("final_status", "pass")?;
    run.assert_plan_goal("Build a harness run and persist all stage artifacts for inspection.")?;
    run.assert_feature_count(1)?;
    run.assert_feature_state(0, "feature-001", "passed", "complete", 0, "pass")?;
    run.assert_exists("worker/prompts/plan-01.md");
    run.assert_exists("worker/outputs/plan-01-last-message.json");
    run.assert_exists("worker/plan-01-result.json");
    run.assert_exists("features/01-feature-001/feature-contract.json");
    run.assert_exists("features/01-feature-001/builder-handoff.json");
    run.assert_exists("features/01-feature-001/qa-report.json");
    run.assert_exists("features/01-feature-001/worker/prompts/build-01.md");
    run.assert_exists("features/01-feature-001/worker/prompts/evaluate-01.md");
    run.assert_exists("features/01-feature-001/worker/outputs/build-01-last-message.json");
    run.assert_exists("features/01-feature-001/worker/outputs/evaluate-01-last-message.json");
    run.assert_feature_line(
        "feature=index=0 id=feature-001 status=passed phase=complete repairs=0 qa=pass",
    );
    run.assert_stage_line("stage=plan attempt=1 status=prepared session_id=simulated-plan-01")?;
    run.assert_stage_line(
        "stage=feature:feature-001 stage=build attempt=1 status=prepared session_id=simulated-build-01",
    )?;
    run.assert_stage_line(
        "stage=feature:feature-001 stage=evaluate attempt=1 status=prepared session_id=simulated-evaluate-01",
    )?;

    let inspect_output = fixture.inspect(&run.run_root)?;
    fixture.assert_success(&inspect_output)?;
    let inspected = fixture.parse_run(&inspect_output)?;
    inspected.assert_cli_field("lifecycle", "passed")?;

    let resume_output = fixture.resume(&run.run_root)?;
    fixture.assert_success(&resume_output)?;
    let resumed = fixture.parse_run(&resume_output)?;
    resumed.assert_cli_field("lifecycle", "passed")?;
    resumed.assert_stage_count(3)?;

    Ok(())
}

#[test]
fn simulated_cli_resume_completed_failed_run_is_idempotent() -> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new(&["fail"], 0)?;
    let output = fixture.run("Resume should not mutate a completed failed run.\n", None)?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_stage_count(3)?;
    run.assert_cli_field("lifecycle", "failed")?;
    run.assert_cli_field("final_status", "fail")?;

    let resume_output = fixture.resume(&run.run_root)?;
    fixture.assert_success(&resume_output)?;
    let resumed = fixture.parse_run(&resume_output)?;
    resumed.assert_stage_count(3)?;
    resumed.assert_cli_field("lifecycle", "failed")?;
    resumed.assert_cli_field("final_status", "fail")?;

    Ok(())
}

#[test]
fn simulated_cli_continue_after_failure_processes_all_features() -> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new_with_options(SmokeFixtureOptions {
        worker_mode: WorkerMode::Simulated {
            evaluator_statuses: vec!["fail"],
        },
        planner_worker_mode: None,
        max_repair_attempts: 0,
        continue_after_failure: true,
        require_screenshots: false,
        screenshot_commands: Vec::new(),
        verification_commands: vec![vec!["/usr/bin/env", "true"]],
        workspace_isolation: "direct",
        initialize_git_repo: false,
        runtime_supervision: RuntimeSupervisionOptions::default(),
        services: vec![default_service()],
        stacks: Vec::new(),
    })?;
    let output = fixture.run(
        "Both features should fail but the run should process all.\n",
        Some(2),
    )?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_cli_field("lifecycle", "failed")?;
    run.assert_cli_field("final_status", "fail")?;
    run.assert_cli_field("current_feature_index", "2")?;
    run.assert_feature_count(2)?;
    run.assert_feature_state(0, "feature-001", "failed", "complete", 0, "fail")?;
    run.assert_feature_state(1, "feature-002", "failed", "complete", 0, "fail")?;

    Ok(())
}

#[test]
fn simulated_cli_resume_continues_past_failed_features_when_configured()
-> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new_with_options(SmokeFixtureOptions {
        worker_mode: WorkerMode::Simulated {
            evaluator_statuses: vec!["fail"],
        },
        planner_worker_mode: None,
        max_repair_attempts: 0,
        continue_after_failure: false,
        require_screenshots: false,
        screenshot_commands: Vec::new(),
        verification_commands: vec![vec!["/usr/bin/env", "true"]],
        workspace_isolation: "direct",
        initialize_git_repo: false,
        runtime_supervision: RuntimeSupervisionOptions::default(),
        services: vec![default_service()],
        stacks: Vec::new(),
    })?;
    let output = fixture.run("First run stops at first failure.\n", Some(2))?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_cli_field("lifecycle", "failed")?;
    run.assert_cli_field("current_feature_index", "0")?;
    run.assert_feature_state(0, "feature-001", "failed", "complete", 0, "fail")?;
    run.assert_feature_state(1, "feature-002", "pending", "pending_build", 0, "-")?;

    let config_path = &fixture.config_path;
    let config_content = fs::read_to_string(config_path)?;
    let updated_config = config_content.replace(
        "continue_after_failure = false",
        "continue_after_failure = true",
    );
    fs::write(config_path, updated_config)?;

    let resume_output = fixture.resume(&run.run_root)?;
    fixture.assert_success(&resume_output)?;
    let resumed = fixture.parse_run(&resume_output)?;
    resumed.assert_cli_field("lifecycle", "failed")?;
    resumed.assert_cli_field("final_status", "fail")?;
    resumed.assert_cli_field("current_feature_index", "2")?;
    resumed.assert_feature_state(0, "feature-001", "failed", "complete", 0, "fail")?;
    resumed.assert_feature_state(1, "feature-002", "failed", "complete", 0, "fail")?;

    Ok(())
}

#[test]
fn simulated_cli_repair_path_records_resume_artifacts() -> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new(&["fail", "pass"], 1)?;
    let output = fixture.run("Force one repair cycle before accepting the run.\n", None)?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_root_layout()?;
    run.assert_stage_count(5)?;
    run.assert_cli_field("lifecycle", "passed")?;
    run.assert_cli_field("final_status", "pass")?;
    run.assert_feature_state(0, "feature-001", "passed", "complete", 1, "pass")?;
    run.assert_exists("features/01-feature-001/worker/prompts/repair-01.md");
    run.assert_exists("features/01-feature-001/worker/prompts/evaluate-02.md");
    run.assert_exists("features/01-feature-001/worker/outputs/repair-01-last-message.json");
    run.assert_exists("features/01-feature-001/worker/outputs/evaluate-02-last-message.json");
    run.assert_stage_line(
        "stage=feature:feature-001 stage=repair attempt=1 status=prepared session_id=simulated-repair-01",
    )?;
    run.assert_stage_line(
        "stage=feature:feature-001 stage=evaluate attempt=2 status=prepared session_id=simulated-evaluate-02",
    )?;

    let repair_result = run.read_json("features/01-feature-001/worker/repair-01-result.json")?;
    let repair_command = json_string_array(&repair_result["command"]);
    assert_eq!(
        repair_command,
        vec![
            "simulated".to_string(),
            "repair".to_string(),
            "resume".to_string(),
            "simulated-build-01".to_string()
        ]
    );

    let final_handoff = run.read_json("features/01-feature-001/builder-handoff.json")?;
    assert_eq!(
        final_handoff["summary"],
        "Simulated repair handoff. No code changes were made."
    );

    let final_report = run.read_json("features/01-feature-001/qa-report.json")?;
    assert_eq!(final_report["status"], "pass");

    Ok(())
}

#[test]
fn simulated_cli_inconclusive_evaluation_triggers_a_repair_cycle() -> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new(&["inconclusive", "pass"], 1)?;
    let output = fixture.run("Repair after an inconclusive evaluation result.\n", None)?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_stage_count(5)?;
    run.assert_cli_field("lifecycle", "passed")?;
    run.assert_cli_field("final_status", "pass")?;
    run.assert_feature_state(0, "feature-001", "passed", "complete", 1, "pass")?;
    assert_eq!(
        run.read_json("features/01-feature-001/worker/outputs/evaluate-01-last-message.json")?["status"],
        "inconclusive"
    );
    run.assert_stage_line(
        "stage=feature:feature-001 stage=repair attempt=1 status=prepared session_id=simulated-repair-01",
    )?;

    Ok(())
}

#[test]
fn simulated_cli_repair_budget_exhaustion_fails_consistently() -> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new(&["fail", "fail"], 1)?;
    let output = fixture.run("Fail even after one repair attempt.\n", None)?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_root_layout()?;
    run.assert_stage_count(5)?;
    run.assert_cli_field("lifecycle", "failed")?;
    run.assert_cli_field("final_status", "fail")?;
    run.assert_cli_field("current_feature_index", "0")?;
    run.assert_feature_state(0, "feature-001", "failed", "complete", 1, "fail")?;

    let final_report = run.read_json("features/01-feature-001/qa-report.json")?;
    assert_eq!(final_report["status"], "fail");
    assert_eq!(
        final_report["summary"],
        "Simulated evaluator result for attempt 2."
    );

    Ok(())
}

#[test]
fn simulated_cli_multiple_repair_rounds_increment_stage_suffixes() -> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new(&["fail", "fail", "pass"], 2)?;
    let output = fixture.run("Use two repair rounds before a passing evaluation.\n", None)?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_stage_count(7)?;
    run.assert_cli_field("lifecycle", "passed")?;
    run.assert_feature_state(0, "feature-001", "passed", "complete", 2, "pass")?;
    run.assert_exists("features/01-feature-001/worker/prompts/repair-02.md");
    run.assert_exists("features/01-feature-001/worker/prompts/evaluate-03.md");
    run.assert_exists("features/01-feature-001/worker/outputs/repair-02-last-message.json");
    run.assert_exists("features/01-feature-001/worker/outputs/evaluate-03-last-message.json");

    Ok(())
}

#[test]
fn simulated_cli_executes_multiple_features_in_backlog_order() -> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new(&["pass"], 1)?;
    let output = fixture.run("Exercise the seeded backlog with two features.\n", Some(2))?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_root_layout()?;
    run.assert_stage_count(5)?;
    run.assert_cli_field("lifecycle", "passed")?;
    run.assert_cli_field("final_status", "pass")?;
    run.assert_cli_field("current_feature_index", "2")?;
    run.assert_feature_count(2)?;
    run.assert_feature_state(0, "feature-001", "passed", "complete", 0, "pass")?;
    run.assert_feature_state(1, "feature-002", "passed", "complete", 0, "pass")?;
    run.assert_exists("features/01-feature-001/worker/outputs/build-01-last-message.json");
    run.assert_exists("features/02-feature-002/worker/outputs/build-01-last-message.json");
    run.assert_stage_line(
        "stage=feature:feature-002 stage=evaluate attempt=1 status=prepared session_id=simulated-evaluate-01",
    )?;

    Ok(())
}

#[test]
fn simulated_cli_planner_uses_structured_request_lines_for_feature_slices()
-> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new(&["pass"], 1)?;
    let output = fixture.run(
        "Ship account onboarding\n- Add signup form\n- Add verification email\n",
        Some(2),
    )?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_cli_field("lifecycle", "passed")?;
    run.assert_feature_count(2)?;
    assert_eq!(run.plan["goal"], "Ship account onboarding");
    assert_eq!(run.plan["features"][0]["title"], "Add signup form");
    assert_eq!(run.plan["features"][1]["title"], "Add verification email");

    Ok(())
}

#[test]
fn simulated_cli_config_feature_limit_is_advisory_for_structured_requests()
-> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new(&["pass"], 1)?;
    let output = fixture.run(
        "Ship account onboarding\n- Add signup form\n- Add verification email\n",
        None,
    )?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_cli_field("lifecycle", "passed")?;
    run.assert_feature_count(2)?;
    assert_eq!(run.plan["goal"], "Ship account onboarding");
    assert_eq!(run.plan["features"][0]["title"], "Add signup form");
    assert_eq!(run.plan["features"][1]["title"], "Add verification email");

    let prompt = fs::read_to_string(run.run_root.join("worker/prompts/plan-01.md"))?;
    assert!(prompt.contains("\"feature_limit\": 1"));
    assert!(prompt.contains("\"feature_limit_is_hard\": false"));

    Ok(())
}

#[test]
fn simulated_cli_planner_records_prompt_context_and_honest_risks() -> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new(&["pass"], 1)?;
    let output = fixture.run("Stabilize the billing dashboard.\n", Some(2))?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_cli_field("lifecycle", "passed")?;

    let risks = run.plan["risks"]
        .as_array()
        .expect("risks should be an array")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert!(risks.iter().any(|risk| risk.contains("placeholder-only")));
    assert!(risks.iter().any(|risk| risk.contains("runtime readiness")));
    assert!(
        risks
            .iter()
            .any(|risk| risk.contains("planner-derived follow-ups"))
    );

    let checkpoints = run.plan["checkpoints"]
        .as_array()
        .expect("checkpoints should be an array")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert!(
        checkpoints
            .iter()
            .any(|checkpoint| checkpoint.contains("configured verification commands"))
    );
    assert!(
        checkpoints
            .iter()
            .any(|checkpoint| checkpoint.contains("runtime services are ready"))
    );

    let prompt = fs::read_to_string(run.run_root.join("worker/prompts/plan-01.md"))?;
    assert!(prompt.contains("Harness Context:"));
    assert!(prompt.contains("Stabilize the billing dashboard."));
    assert!(prompt.contains("\"feature_limit\": 2"));
    assert!(prompt.contains("Return only JSON matching this schema"));

    Ok(())
}

#[test]
fn simulated_cli_fails_when_verification_commands_fail_even_if_evaluator_passes()
-> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new_with_commands(&["pass"], 0, &[vec!["/usr/bin/env", "false"]])?;
    let output = fixture.run(
        "Deterministic verification should force a failure on this run.\n",
        None,
    )?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_stage_count(3)?;
    run.assert_cli_field("lifecycle", "failed")?;
    run.assert_cli_field("final_status", "fail")?;
    run.assert_feature_state(0, "feature-001", "failed", "complete", 0, "fail")?;
    run.assert_exists("features/01-feature-001/runtime/verification/evaluate-01/report.json");

    let qa_report = run.read_json("features/01-feature-001/qa-report.json")?;
    assert_eq!(qa_report["status"], "fail");
    assert!(
        qa_report["summary"]
            .as_str()
            .expect("summary should be a string")
            .contains("Deterministic verification failed")
    );

    let verification_report =
        run.read_json("features/01-feature-001/runtime/verification/evaluate-01/report.json")?;
    assert_eq!(
        verification_report["results"][0]["status"],
        Value::String("failed".to_string())
    );

    Ok(())
}

#[test]
fn simulated_cli_inspect_and_resume_recover_an_interrupted_run() -> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new(&["pass"], 1)?;
    let output = fixture.run(
        "Persist a recoverable checkpoint and then finish after resume.\n",
        None,
    )?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.rewind_to_pending_evaluate()?;

    let inspect_output = fixture.inspect(&run.run_root)?;
    fixture.assert_success(&inspect_output)?;
    let inspected = fixture.parse_run(&inspect_output)?;
    inspected.assert_cli_field("lifecycle", "running")?;
    inspected.assert_cli_field("final_status", "-")?;
    inspected.assert_cli_field("current_feature_index", "0")?;
    inspected.assert_feature_state(0, "feature-001", "running", "pending_evaluate", 0, "-")?;

    let resume_output = fixture.resume(&run.run_root)?;
    fixture.assert_success(&resume_output)?;
    let resumed = fixture.parse_run(&resume_output)?;
    resumed.assert_stage_count(3)?;
    resumed.assert_cli_field("lifecycle", "passed")?;
    resumed.assert_cli_field("final_status", "pass")?;
    resumed.assert_cli_field("current_feature_index", "1")?;
    resumed.assert_feature_state(0, "feature-001", "passed", "complete", 0, "pass")?;
    resumed.assert_stage_line(
        "stage=feature:feature-001 stage=evaluate attempt=1 status=prepared session_id=simulated-evaluate-01",
    )?;

    Ok(())
}

#[test]
fn simulated_cli_resume_replans_when_plan_stage_is_missing() -> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new(&["pass"], 1)?;
    let output = fixture.run(
        "Persist an interruption before planning completes and recover on resume.\n",
        None,
    )?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.rewind_to_pending_plan()?;

    let inspect_output = fixture.inspect(&run.run_root)?;
    fixture.assert_success(&inspect_output)?;
    let inspected = fixture.parse_run(&inspect_output)?;
    inspected.assert_cli_field("lifecycle", "running")?;
    inspected.assert_cli_field("final_status", "-")?;
    inspected.assert_feature_count(0)?;
    inspected.assert_stage_count(0)?;

    let resume_output = fixture.resume(&run.run_root)?;
    fixture.assert_success(&resume_output)?;
    let resumed = fixture.parse_run(&resume_output)?;
    resumed.assert_stage_count(3)?;
    resumed.assert_cli_field("lifecycle", "passed")?;
    resumed.assert_cli_field("final_status", "pass")?;
    resumed.assert_feature_state(0, "feature-001", "passed", "complete", 0, "pass")?;
    resumed
        .assert_stage_line("stage=plan attempt=1 status=prepared session_id=simulated-plan-01")?;

    Ok(())
}

#[test]
fn simulated_cli_git_worktree_journey_runs_in_the_original_workspace() -> Result<(), Box<dyn Error>>
{
    let fixture = SmokeFixture::new_with_options(SmokeFixtureOptions {
        worker_mode: WorkerMode::Simulated {
            evaluator_statuses: vec!["pass"],
        },
        planner_worker_mode: None,
        max_repair_attempts: 1,
        continue_after_failure: false,
        require_screenshots: false,
        screenshot_commands: Vec::new(),
        verification_commands: vec![vec!["/usr/bin/env", "true"]],
        workspace_isolation: "git_worktree",
        initialize_git_repo: true,
        runtime_supervision: RuntimeSupervisionOptions::default(),
        services: vec![default_service()],
        stacks: Vec::new(),
    })?;
    let output = fixture.run("Run the harness directly in the git workspace.\n", None)?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_root_layout()?;
    run.assert_cli_field("lifecycle", "passed")?;
    run.assert_source_workspace(&fixture.workspace_dir)?;
    run.assert_execution_workspace_matches_source(&fixture.workspace_dir)?;
    run.assert_missing("workspace");

    let runtime_plan = run.read_json("runtime-plan.json")?;
    assert_eq!(runtime_plan["workspace"], run.state["execution_workspace"]);

    Ok(())
}

#[test]
fn simulated_cli_git_worktree_run_fails_for_a_non_git_workspace() -> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new_with_options(SmokeFixtureOptions {
        worker_mode: WorkerMode::Simulated {
            evaluator_statuses: vec!["pass"],
        },
        planner_worker_mode: None,
        max_repair_attempts: 1,
        continue_after_failure: false,
        require_screenshots: false,
        screenshot_commands: Vec::new(),
        verification_commands: vec![vec!["/usr/bin/env", "true"]],
        workspace_isolation: "git_worktree",
        initialize_git_repo: false,
        runtime_supervision: RuntimeSupervisionOptions::default(),
        services: vec![default_service()],
        stacks: Vec::new(),
    })?;
    let output = fixture.run("This should fail before the run starts.\n", None)?;
    fixture.assert_failure_contains(
        &output,
        "failed to locate git repository root for git workspace execution",
    )?;

    Ok(())
}

#[test]
fn simulated_cli_git_worktree_journey_runs_in_the_original_workspace_without_head()
-> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new_with_options(SmokeFixtureOptions {
        worker_mode: WorkerMode::Simulated {
            evaluator_statuses: vec!["pass"],
        },
        planner_worker_mode: None,
        max_repair_attempts: 1,
        continue_after_failure: false,
        require_screenshots: false,
        screenshot_commands: Vec::new(),
        verification_commands: vec![vec!["/usr/bin/env", "true"]],
        workspace_isolation: "git_worktree",
        initialize_git_repo: false,
        runtime_supervision: RuntimeSupervisionOptions::default(),
        services: vec![default_service()],
        stacks: Vec::new(),
    })?;
    fs::write(fixture.workspace_dir.join("README.md"), "workspace\n")?;
    run_ok(
        Command::new("git").arg("init").arg(&fixture.workspace_dir),
        "git init fixture workspace without head",
    )?;

    let output = fixture.run(
        "Run the harness in the original workspace before the first commit.\n",
        None,
    )?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_root_layout()?;
    run.assert_cli_field("lifecycle", "passed")?;
    run.assert_source_workspace(&fixture.workspace_dir)?;
    run.assert_execution_workspace_matches_source(&fixture.workspace_dir)?;
    run.assert_missing("workspace");

    Ok(())
}

#[test]
fn simulated_cli_git_worktree_journey_runs_in_place_when_runs_dir_is_inside_workspace()
-> Result<(), Box<dyn Error>> {
    let mut fixture = SmokeFixture::new_with_options(SmokeFixtureOptions {
        worker_mode: WorkerMode::Simulated {
            evaluator_statuses: vec!["pass"],
        },
        planner_worker_mode: None,
        max_repair_attempts: 1,
        continue_after_failure: false,
        require_screenshots: false,
        screenshot_commands: Vec::new(),
        verification_commands: vec![vec!["/usr/bin/env", "true"]],
        workspace_isolation: "git_worktree",
        initialize_git_repo: false,
        runtime_supervision: RuntimeSupervisionOptions::default(),
        services: vec![default_service()],
        stacks: Vec::new(),
    })?;
    fs::write(
        &fixture.config_path,
        fs::read_to_string(&fixture.config_path)?.replace(
            "runs_dir = \".loopsmith-runs\"",
            "runs_dir = \"workspace/.loopsmith-runs\"",
        ),
    )?;
    fixture.runs_dir = fixture.workspace_dir.join(".loopsmith-runs");
    fs::write(fixture.workspace_dir.join("README.md"), "workspace\n")?;
    run_ok(
        Command::new("git").arg("init").arg(&fixture.workspace_dir),
        "git init fixture workspace without head",
    )?;

    let output = fixture.run(
        "Run the harness in place when run artifacts live under the source workspace.\n",
        None,
    )?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_root_layout()?;
    run.assert_cli_field("lifecycle", "passed")?;
    run.assert_source_workspace(&fixture.workspace_dir)?;
    run.assert_execution_workspace_matches_source(&fixture.workspace_dir)?;
    run.assert_missing("workspace");

    Ok(())
}

#[test]
fn simulated_cli_run_fails_with_a_clear_config_error() -> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new(&["pass"], 1)?;
    fs::write(
        &fixture.config_path,
        r#"
[project]
root_dir = ".."

[storage]
runs_dir = ".loopsmith-runs"

[workspace]
isolation = "direct"

[worker]
kind = "simulated"

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

[evaluator]
dimensions = ["correctness"]
require_screenshots = false
commands = [
  ["/usr/bin/env", "true"]
]
"#,
    )?;

    let output = fixture.run("Surface the config validation error.\n", None)?;
    fixture.assert_failure_contains(&output, "missing field `simulation`")?;

    Ok(())
}

#[test]
fn simulated_cli_run_fails_with_a_missing_codex_worker_config() -> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new(&["pass"], 1)?;
    fs::write(
        &fixture.config_path,
        r#"
[project]
root_dir = ".."

[storage]
runs_dir = ".loopsmith-runs"

[workspace]
isolation = "direct"

[worker]
kind = "codex_cli"

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

[evaluator]
dimensions = ["correctness"]
require_screenshots = false
commands = [
  ["/usr/bin/env", "true"]
]
"#,
    )?;

    let output = fixture.run("Surface the missing codex worker configuration.\n", None)?;
    fixture.assert_failure_contains(&output, "missing field `codex`")?;

    Ok(())
}

#[test]
fn simulated_cli_inspect_fails_when_the_run_root_does_not_exist() -> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new(&["pass"], 1)?;
    let missing_run_root = fixture.runs_dir.join("missing-run");
    let output = fixture.inspect(&missing_run_root)?;
    fixture.assert_failure_contains(&output, "failed to load run state")?;

    Ok(())
}

#[test]
fn simulated_cli_runtime_supervision_happy_path_manages_service_lifecycle()
-> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new_with_options(SmokeFixtureOptions {
        worker_mode: WorkerMode::Simulated {
            evaluator_statuses: vec!["pass"],
        },
        planner_worker_mode: None,
        max_repair_attempts: 1,
        continue_after_failure: false,
        require_screenshots: false,
        screenshot_commands: Vec::new(),
        verification_commands: vec![vec!["/usr/bin/env", "true"]],
        workspace_isolation: "direct",
        initialize_git_repo: false,
        runtime_supervision: RuntimeSupervisionOptions {
            enabled: true,
            startup_timeout_secs: 5,
            readiness_poll_interval_ms: 100,
            shutdown_grace_period_secs: 1,
        },
        services: vec![ServiceFixture {
            name: "web",
            start: vec![
                "/bin/sh",
                "-c",
                "printf 'booted\\n'; touch ready.txt; trap 'exit 0' TERM INT; while true; do sleep 1; done",
            ],
            working_dir: ".",
            ready_url: None,
            ready_command: Some(vec!["/bin/sh", "-c", "test -f ready.txt"]),
        }],
        stacks: Vec::new(),
    })?;
    let output = fixture.run("Exercise runtime supervision on a passing run.\n", None)?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_cli_field("lifecycle", "passed")?;
    let service_record = run.read_json("runtime/services/web/service.json")?;
    assert_eq!(service_record["status"], "stopped");
    assert!(service_record["ready_at"].is_string());
    assert!(service_record["stopped_at"].is_string());
    run.assert_exists("runtime/services/web/stdout.log");
    run.assert_exists("runtime/services/web/stderr.log");

    Ok(())
}

#[test]
fn simulated_cli_runtime_supervision_marks_service_ready_without_probes()
-> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new_with_options(SmokeFixtureOptions {
        worker_mode: WorkerMode::Simulated {
            evaluator_statuses: vec!["pass"],
        },
        planner_worker_mode: None,
        max_repair_attempts: 1,
        continue_after_failure: false,
        require_screenshots: false,
        screenshot_commands: Vec::new(),
        verification_commands: vec![vec!["/usr/bin/env", "true"]],
        workspace_isolation: "direct",
        initialize_git_repo: false,
        runtime_supervision: RuntimeSupervisionOptions {
            enabled: true,
            startup_timeout_secs: 5,
            readiness_poll_interval_ms: 100,
            shutdown_grace_period_secs: 1,
        },
        services: vec![ServiceFixture {
            name: "web",
            start: vec![
                "/bin/sh",
                "-c",
                "printf 'booted\\n'; trap 'exit 0' TERM INT; while true; do sleep 1; done",
            ],
            working_dir: ".",
            ready_url: None,
            ready_command: None,
        }],
        stacks: Vec::new(),
    })?;
    let output = fixture.run(
        "Exercise runtime supervision when no readiness probes are configured.\n",
        None,
    )?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_cli_field("lifecycle", "passed")?;
    let service_record = run.read_json("runtime/services/web/service.json")?;
    assert_eq!(service_record["status"], "stopped");
    assert!(service_record["ready_at"].is_string());
    assert!(service_record["stopped_at"].is_string());

    Ok(())
}

#[cfg(unix)]
#[test]
fn simulated_cli_runtime_supervision_terminates_service_process_group_descendants()
-> Result<(), Box<dyn Error>> {
    struct PidGuard(Option<u32>);

    impl Drop for PidGuard {
        fn drop(&mut self) {
            if let Some(pid) = self.0.take() {
                let _ = Command::new("kill")
                    .arg("-KILL")
                    .arg(pid.to_string())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }
    }

    let fixture = SmokeFixture::new_with_options(SmokeFixtureOptions {
        worker_mode: WorkerMode::Simulated {
            evaluator_statuses: vec!["pass"],
        },
        planner_worker_mode: None,
        max_repair_attempts: 1,
        continue_after_failure: false,
        require_screenshots: false,
        screenshot_commands: Vec::new(),
        verification_commands: vec![vec!["/usr/bin/env", "true"]],
        workspace_isolation: "direct",
        initialize_git_repo: false,
        runtime_supervision: RuntimeSupervisionOptions {
            enabled: true,
            startup_timeout_secs: 5,
            readiness_poll_interval_ms: 100,
            shutdown_grace_period_secs: 1,
        },
        services: vec![ServiceFixture {
            name: "web",
            start: vec![
                "/bin/sh",
                "-c",
                r#"nohup /bin/sh -c 'echo $$ > grandchild.pid; trap "exit 0" TERM INT; while true; do sleep 1; done' >/dev/null 2>&1 & printf 'booted\n'; touch ready.txt; trap 'exit 0' TERM INT; while true; do sleep 1; done"#,
            ],
            working_dir: ".",
            ready_url: None,
            ready_command: Some(vec!["/bin/sh", "-c", "test -f ready.txt"]),
        }],
        stacks: Vec::new(),
    })?;
    let output = fixture.run(
        "Ensure supervised services clean up spawned descendants on shutdown.\n",
        None,
    )?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_cli_field("lifecycle", "passed")?;

    let pid_file = fixture.workspace_dir.join("grandchild.pid");
    let mut grandchild_pid = None;
    for _ in 0..20 {
        if pid_file.exists() {
            grandchild_pid = Some(fs::read_to_string(&pid_file)?.trim().parse::<u32>()?);
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let grandchild_pid = grandchild_pid.expect("expected service descendant pid file");
    let mut guard = PidGuard(Some(grandchild_pid));

    let mut exited = false;
    for _ in 0..20 {
        let status = Command::new("kill")
            .arg("-0")
            .arg(grandchild_pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if !status.success() {
            exited = true;
            guard.0 = None;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    assert!(
        exited,
        "expected supervised service descendant process {grandchild_pid} to exit during shutdown"
    );

    let service_record = run.read_json("runtime/services/web/service.json")?;
    assert_eq!(service_record["status"], "stopped");

    Ok(())
}

#[test]
fn simulated_cli_runtime_supervision_fails_when_service_never_becomes_ready()
-> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new_with_options(SmokeFixtureOptions {
        worker_mode: WorkerMode::Simulated {
            evaluator_statuses: vec!["pass"],
        },
        planner_worker_mode: None,
        max_repair_attempts: 1,
        continue_after_failure: false,
        require_screenshots: false,
        screenshot_commands: Vec::new(),
        verification_commands: vec![vec!["/usr/bin/env", "true"]],
        workspace_isolation: "direct",
        initialize_git_repo: false,
        runtime_supervision: RuntimeSupervisionOptions {
            enabled: true,
            startup_timeout_secs: 1,
            readiness_poll_interval_ms: 100,
            shutdown_grace_period_secs: 1,
        },
        services: vec![ServiceFixture {
            name: "web",
            start: vec![
                "/bin/sh",
                "-c",
                "trap 'exit 0' TERM INT; while true; do sleep 1; done",
            ],
            working_dir: ".",
            ready_url: None,
            ready_command: Some(vec!["/bin/sh", "-c", "test -f ready.txt"]),
        }],
        stacks: Vec::new(),
    })?;
    let output = fixture.run(
        "Fail when the supervised service never becomes ready.\n",
        None,
    )?;
    fixture.assert_failure_contains(&output, "failed readiness checks")?;

    let run_root = fixture.single_run_root()?;
    let service_record = read_json(&run_root.join("runtime/services/web/service.json"))?;
    assert_eq!(service_record["name"], "web");
    assert!(service_record["stopped_at"].is_string());

    Ok(())
}

#[test]
fn simulated_cli_runtime_supervision_fails_when_service_exits_before_becoming_ready()
-> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new_with_options(SmokeFixtureOptions {
        worker_mode: WorkerMode::Simulated {
            evaluator_statuses: vec!["pass"],
        },
        planner_worker_mode: None,
        max_repair_attempts: 1,
        continue_after_failure: false,
        require_screenshots: false,
        screenshot_commands: Vec::new(),
        verification_commands: vec![vec!["/usr/bin/env", "true"]],
        workspace_isolation: "direct",
        initialize_git_repo: false,
        runtime_supervision: RuntimeSupervisionOptions {
            enabled: true,
            startup_timeout_secs: 1,
            readiness_poll_interval_ms: 100,
            shutdown_grace_period_secs: 1,
        },
        services: vec![ServiceFixture {
            name: "web",
            start: vec!["/bin/sh", "-c", "printf 'booted\\n'; exit 7"],
            working_dir: ".",
            ready_url: None,
            ready_command: Some(vec!["/bin/sh", "-c", "test -f ready.txt"]),
        }],
        stacks: Vec::new(),
    })?;
    let output = fixture.run(
        "Fail when the supervised service exits before it reports readiness.\n",
        None,
    )?;
    fixture.assert_failure_contains(&output, "exited before becoming ready")?;

    let run_root = fixture.single_run_root()?;
    let service_record = read_json(&run_root.join("runtime/services/web/service.json"))?;
    assert_eq!(service_record["name"], "web");
    assert_eq!(service_record["exit_code"], 7);

    Ok(())
}

#[test]
fn simulated_cli_runtime_stack_happy_path_manages_stack_lifecycle() -> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new_with_options(SmokeFixtureOptions {
        worker_mode: WorkerMode::Simulated {
            evaluator_statuses: vec!["pass"],
        },
        planner_worker_mode: None,
        max_repair_attempts: 1,
        continue_after_failure: false,
        require_screenshots: false,
        screenshot_commands: Vec::new(),
        verification_commands: vec![vec!["/usr/bin/env", "true"]],
        workspace_isolation: "direct",
        initialize_git_repo: false,
        runtime_supervision: RuntimeSupervisionOptions {
            enabled: true,
            startup_timeout_secs: 5,
            readiness_poll_interval_ms: 100,
            shutdown_grace_period_secs: 1,
        },
        services: Vec::new(),
        stacks: vec![StackFixture {
            name: "compose",
            up: vec![
                "/bin/sh",
                "-c",
                "printf 'up\\n'; touch stack-ready.txt; touch stack-up.txt",
            ],
            down: vec![
                "/bin/sh",
                "-c",
                "printf 'down\\n'; rm -f stack-ready.txt; touch stack-down.txt",
            ],
            working_dir: ".",
            ready_url: None,
            ready_command: Some(vec!["/bin/sh", "-c", "test -f stack-ready.txt"]),
        }],
    })?;
    let output = fixture.run(
        "Exercise runtime stack orchestration on a passing run.\n",
        None,
    )?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_cli_field("lifecycle", "passed")?;
    let stack_record = run.read_json("runtime/stacks/compose/stack.json")?;
    assert_eq!(stack_record["status"], "stopped");
    assert!(stack_record["ready_at"].is_string());
    assert!(stack_record["stopped_at"].is_string());
    run.assert_exists("runtime/stacks/compose/up.stdout.log");
    run.assert_exists("runtime/stacks/compose/down.stdout.log");
    assert!(fixture.workspace_dir.join("stack-up.txt").exists());
    assert!(fixture.workspace_dir.join("stack-down.txt").exists());
    assert!(!fixture.workspace_dir.join("stack-ready.txt").exists());

    Ok(())
}

#[test]
fn simulated_cli_runtime_stack_fails_when_stack_never_becomes_ready() -> Result<(), Box<dyn Error>>
{
    let fixture = SmokeFixture::new_with_options(SmokeFixtureOptions {
        worker_mode: WorkerMode::Simulated {
            evaluator_statuses: vec!["pass"],
        },
        planner_worker_mode: None,
        max_repair_attempts: 1,
        continue_after_failure: false,
        require_screenshots: false,
        screenshot_commands: Vec::new(),
        verification_commands: vec![vec!["/usr/bin/env", "true"]],
        workspace_isolation: "direct",
        initialize_git_repo: false,
        runtime_supervision: RuntimeSupervisionOptions {
            enabled: true,
            startup_timeout_secs: 1,
            readiness_poll_interval_ms: 100,
            shutdown_grace_period_secs: 1,
        },
        services: Vec::new(),
        stacks: vec![StackFixture {
            name: "compose",
            up: vec!["/bin/sh", "-c", "printf 'up\\n'; touch stack-up.txt"],
            down: vec!["/bin/sh", "-c", "printf 'down\\n'; touch stack-down.txt"],
            working_dir: ".",
            ready_url: None,
            ready_command: Some(vec!["/bin/sh", "-c", "test -f stack-ready.txt"]),
        }],
    })?;
    let output = fixture.run(
        "Fail when the orchestrated runtime stack never becomes ready.\n",
        None,
    )?;
    fixture.assert_failure_contains(&output, "stack `compose` failed readiness checks")?;

    let run_root = fixture.single_run_root()?;
    let stack_record = read_json(&run_root.join("runtime/stacks/compose/stack.json"))?;
    assert!(stack_record["stopped_at"].is_string());
    assert!(fixture.workspace_dir.join("stack-down.txt").exists());

    Ok(())
}

#[test]
fn simulated_cli_passes_when_required_screenshots_are_captured() -> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new_with_options(SmokeFixtureOptions {
        worker_mode: WorkerMode::Simulated {
            evaluator_statuses: vec!["pass"],
        },
        planner_worker_mode: None,
        max_repair_attempts: 1,
        continue_after_failure: false,
        require_screenshots: true,
        screenshot_commands: vec![ScreenshotFixture {
            name: "home",
            command: vec![
                "/bin/sh",
                "-c",
                "printf '\\211PNG\\r\\n\\032\\n' > \"$CODEX_HARNESS_SCREENSHOT_OUTPUT\"",
            ],
        }],
        verification_commands: vec![vec!["/usr/bin/env", "true"]],
        workspace_isolation: "direct",
        initialize_git_repo: false,
        runtime_supervision: RuntimeSupervisionOptions::default(),
        services: vec![default_service()],
        stacks: Vec::new(),
    })?;
    let output = fixture.run("Capture required screenshots for evaluation.\n", None)?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_cli_field("lifecycle", "passed")?;
    run.assert_exists("features/01-feature-001/runtime/screenshots/evaluate-01/report.json");
    run.assert_exists("features/01-feature-001/runtime/screenshots/evaluate-01/shot-01-home.png");

    let screenshot_report =
        run.read_json("features/01-feature-001/runtime/screenshots/evaluate-01/report.json")?;
    assert_eq!(screenshot_report["results"][0]["status"], "captured");
    assert_eq!(screenshot_report["results"][0]["bytes"], 8);

    Ok(())
}

#[test]
fn simulated_cli_fails_when_required_screenshots_are_missing() -> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new_with_options(SmokeFixtureOptions {
        worker_mode: WorkerMode::Simulated {
            evaluator_statuses: vec!["pass"],
        },
        planner_worker_mode: None,
        max_repair_attempts: 0,
        continue_after_failure: false,
        require_screenshots: true,
        screenshot_commands: vec![ScreenshotFixture {
            name: "home",
            command: vec!["/usr/bin/env", "true"],
        }],
        verification_commands: vec![vec!["/usr/bin/env", "true"]],
        workspace_isolation: "direct",
        initialize_git_repo: false,
        runtime_supervision: RuntimeSupervisionOptions::default(),
        services: vec![default_service()],
        stacks: Vec::new(),
    })?;
    let output = fixture.run("Fail when required screenshots are not captured.\n", None)?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_cli_field("lifecycle", "failed")?;
    run.assert_cli_field("final_status", "fail")?;
    run.assert_feature_state(0, "feature-001", "failed", "complete", 0, "fail")?;

    let qa_report = run.read_json("features/01-feature-001/qa-report.json")?;
    assert!(
        qa_report["summary"]
            .as_str()
            .expect("summary should be a string")
            .contains("Required screenshot capture failed")
    );
    assert!(
        qa_report["findings"]
            .as_array()
            .expect("findings should be an array")
            .iter()
            .any(|finding| finding
                .as_str()
                .expect("finding should be a string")
                .contains("screenshot capture failed"))
    );

    Ok(())
}

#[test]
fn simulated_cli_fails_when_required_screenshot_commands_exit_non_zero()
-> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new_with_options(SmokeFixtureOptions {
        worker_mode: WorkerMode::Simulated {
            evaluator_statuses: vec!["pass"],
        },
        planner_worker_mode: None,
        max_repair_attempts: 0,
        continue_after_failure: false,
        require_screenshots: true,
        screenshot_commands: vec![ScreenshotFixture {
            name: "home",
            command: vec!["/usr/bin/env", "false"],
        }],
        verification_commands: vec![vec!["/usr/bin/env", "true"]],
        workspace_isolation: "direct",
        initialize_git_repo: false,
        runtime_supervision: RuntimeSupervisionOptions::default(),
        services: vec![default_service()],
        stacks: Vec::new(),
    })?;
    let output = fixture.run("Fail when screenshot capture exits non-zero.\n", None)?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_cli_field("lifecycle", "failed")?;
    run.assert_cli_field("final_status", "fail")?;
    let screenshot_report =
        run.read_json("features/01-feature-001/runtime/screenshots/evaluate-01/report.json")?;
    assert_eq!(screenshot_report["results"][0]["status"], "failed");
    assert_eq!(screenshot_report["results"][0]["exit_code"], 1);

    Ok(())
}

#[test]
fn simulated_cli_resume_pending_repair_completes_repair_flow() -> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new(&["fail", "pass"], 1)?;
    let output = fixture.run(
        "Persist a recoverable repair checkpoint and finish after resume.\n",
        None,
    )?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.rewind_to_pending_repair()?;

    let inspect_output = fixture.inspect(&run.run_root)?;
    fixture.assert_success(&inspect_output)?;
    let inspected = fixture.parse_run(&inspect_output)?;
    inspected.assert_cli_field("lifecycle", "running")?;
    inspected.assert_feature_state(0, "feature-001", "running", "pending_repair", 0, "fail")?;

    let resume_output = fixture.resume(&run.run_root)?;
    fixture.assert_success(&resume_output)?;
    let resumed = fixture.parse_run(&resume_output)?;
    resumed.assert_stage_count(5)?;
    resumed.assert_cli_field("lifecycle", "passed")?;
    resumed.assert_feature_state(0, "feature-001", "passed", "complete", 1, "pass")?;
    resumed.assert_stage_line(
        "stage=feature:feature-001 stage=repair attempt=1 status=prepared session_id=simulated-repair-01",
    )?;

    Ok(())
}

#[test]
fn fake_codex_cli_planner_override_routes_only_plan_stage() -> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new_with_options(SmokeFixtureOptions {
        worker_mode: WorkerMode::Simulated {
            evaluator_statuses: vec!["pass"],
        },
        planner_worker_mode: Some(WorkerMode::CodexCliFake {
            scenario: fake_codex_planner_override_scenario(),
        }),
        max_repair_attempts: 1,
        continue_after_failure: false,
        require_screenshots: false,
        screenshot_commands: Vec::new(),
        verification_commands: vec![vec!["/usr/bin/env", "true"]],
        workspace_isolation: "direct",
        initialize_git_repo: false,
        runtime_supervision: RuntimeSupervisionOptions::default(),
        services: vec![default_service()],
        stacks: Vec::new(),
    })?;
    let output = fixture.run("Exercise planner override routing.\n", None)?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_cli_field("lifecycle", "passed")?;
    run.assert_stage_count(3)?;
    assert_eq!(run.plan["goal"], "fake codex plan");
    run.assert_stage_line("stage=plan attempt=1 status=executed session_id=fake-plan-override-01")?;
    run.assert_stage_line(
        "stage=feature:feature-001 stage=build attempt=1 status=prepared session_id=simulated-build-01",
    )?;
    run.assert_stage_line(
        "stage=feature:feature-001 stage=evaluate attempt=1 status=prepared session_id=simulated-evaluate-01",
    )?;

    Ok(())
}

#[test]
fn fake_codex_cli_repair_path_exercises_real_worker_selection_and_resume_command()
-> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new_with_options(SmokeFixtureOptions {
        worker_mode: WorkerMode::CodexCliFake {
            scenario: fake_codex_repair_pass_scenario(),
        },
        planner_worker_mode: None,
        max_repair_attempts: 1,
        continue_after_failure: false,
        require_screenshots: false,
        screenshot_commands: Vec::new(),
        verification_commands: vec![vec!["/usr/bin/env", "true"]],
        workspace_isolation: "direct",
        initialize_git_repo: false,
        runtime_supervision: RuntimeSupervisionOptions::default(),
        services: vec![default_service()],
        stacks: Vec::new(),
    })?;
    let output = fixture.run("Exercise the real codex worker adapter locally.\n", None)?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_cli_field("lifecycle", "passed")?;
    run.assert_stage_count(5)?;
    let repair_result = run.read_json("features/01-feature-001/worker/repair-01-result.json")?;
    let repair_command = json_string_array(&repair_result["command"]);
    assert!(repair_command.contains(&"resume".to_string()));
    assert!(repair_command.contains(&"fake-build-01".to_string()));
    let cd_index = repair_command
        .iter()
        .position(|arg| arg == "-C")
        .expect("repair command should include -C");
    let resume_index = repair_command
        .iter()
        .position(|arg| arg == "resume")
        .expect("repair command should include resume");
    assert!(
        cd_index < resume_index,
        "expected -C before resume: {repair_command:?}"
    );
    run.assert_stage_line(
        "stage=feature:feature-001 stage=repair attempt=1 status=executed session_id=fake-repair-01",
    )?;

    Ok(())
}

#[test]
fn fake_codex_cli_repair_uses_exec_when_resume_sessions_are_disabled() -> Result<(), Box<dyn Error>>
{
    let fixture = SmokeFixture::new_with_options(SmokeFixtureOptions {
        worker_mode: WorkerMode::CodexCliFake {
            scenario: fake_codex_repair_pass_scenario(),
        },
        planner_worker_mode: None,
        max_repair_attempts: 1,
        continue_after_failure: false,
        require_screenshots: false,
        screenshot_commands: Vec::new(),
        verification_commands: vec![vec!["/usr/bin/env", "true"]],
        workspace_isolation: "direct",
        initialize_git_repo: false,
        runtime_supervision: RuntimeSupervisionOptions::default(),
        services: vec![default_service()],
        stacks: Vec::new(),
    })?;
    fixture.rewrite_config(|config| {
        config.replace("resume_sessions = true", "resume_sessions = false")
    })?;

    let output = fixture.run(
        "Exercise codex repair without session resume support.\n",
        None,
    )?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_cli_field("lifecycle", "passed")?;
    let repair_result = run.read_json("features/01-feature-001/worker/repair-01-result.json")?;
    let repair_command = json_string_array(&repair_result["command"]);
    assert!(!repair_command.contains(&"resume".to_string()));
    assert!(repair_command.contains(&"--output-schema".to_string()));

    Ok(())
}

#[test]
fn fake_codex_cli_repair_flow_succeeds_without_session_ids() -> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new_with_options(SmokeFixtureOptions {
        worker_mode: WorkerMode::CodexCliFake {
            scenario: fake_codex_repair_pass_without_thread_ids_scenario(),
        },
        planner_worker_mode: None,
        max_repair_attempts: 1,
        continue_after_failure: false,
        require_screenshots: false,
        screenshot_commands: Vec::new(),
        verification_commands: vec![vec!["/usr/bin/env", "true"]],
        workspace_isolation: "direct",
        initialize_git_repo: false,
        runtime_supervision: RuntimeSupervisionOptions::default(),
        services: vec![default_service()],
        stacks: Vec::new(),
    })?;
    let output = fixture.run("Exercise codex repair without emitted session ids.\n", None)?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_stage_count(5)?;
    run.assert_cli_field("lifecycle", "passed")?;
    run.assert_stage_line("stage=plan attempt=1 status=executed session_id=-")?;
    run.assert_stage_line(
        "stage=feature:feature-001 stage=repair attempt=1 status=executed session_id=-",
    )?;

    let repair_result = run.read_json("features/01-feature-001/worker/repair-01-result.json")?;
    let repair_command = json_string_array(&repair_result["command"]);
    assert!(!repair_command.contains(&"resume".to_string()));
    assert!(repair_command.contains(&"--output-schema".to_string()));

    Ok(())
}

#[test]
fn fake_codex_cli_multi_feature_failure_stops_after_second_feature_fails()
-> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new_with_options(SmokeFixtureOptions {
        worker_mode: WorkerMode::CodexCliFake {
            scenario: fake_codex_two_feature_second_fails_scenario(),
        },
        planner_worker_mode: None,
        max_repair_attempts: 0,
        continue_after_failure: false,
        require_screenshots: false,
        screenshot_commands: Vec::new(),
        verification_commands: vec![vec!["/usr/bin/env", "true"]],
        workspace_isolation: "direct",
        initialize_git_repo: false,
        runtime_supervision: RuntimeSupervisionOptions::default(),
        services: vec![default_service()],
        stacks: Vec::new(),
    })?;
    let output = fixture.run("Stop after the second feature fails.\n", Some(2))?;
    fixture.assert_success(&output)?;

    let run = fixture.parse_run(&output)?;
    run.assert_stage_count(5)?;
    run.assert_cli_field("lifecycle", "failed")?;
    run.assert_cli_field("final_status", "fail")?;
    run.assert_cli_field("current_feature_index", "1")?;
    run.assert_feature_state(0, "feature-001", "passed", "complete", 0, "pass")?;
    run.assert_feature_state(1, "feature-002", "failed", "complete", 0, "fail")?;

    Ok(())
}

#[test]
fn fake_codex_cli_run_fails_on_invalid_planner_output() -> Result<(), Box<dyn Error>> {
    assert_fake_codex_stage_failure(
        fake_codex_invalid_plan_scenario(),
        "planner output at",
        Some("worker/outputs/plan-01-last-message.json"),
    )
}

#[test]
fn fake_codex_cli_run_fails_on_invalid_builder_output() -> Result<(), Box<dyn Error>> {
    assert_fake_codex_stage_failure(
        fake_codex_invalid_build_scenario(),
        "builder output at",
        Some("features/01-feature-001/worker/outputs/build-01-last-message.json"),
    )
}

#[test]
fn fake_codex_cli_run_fails_on_invalid_evaluator_output() -> Result<(), Box<dyn Error>> {
    assert_fake_codex_stage_failure(
        fake_codex_invalid_evaluate_scenario(),
        "evaluator output at",
        Some("features/01-feature-001/worker/outputs/evaluate-01-last-message.json"),
    )
}

#[test]
fn fake_codex_cli_run_fails_on_invalid_repair_output() -> Result<(), Box<dyn Error>> {
    assert_fake_codex_stage_failure(
        fake_codex_invalid_repair_scenario(),
        "repair output at",
        Some("features/01-feature-001/worker/outputs/repair-01-last-message.json"),
    )
}

#[test]
fn fake_codex_cli_run_fails_when_worker_process_exits_non_zero() -> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new_with_options(SmokeFixtureOptions {
        worker_mode: WorkerMode::CodexCliFake {
            scenario: fake_codex_repair_pass_scenario(),
        },
        planner_worker_mode: None,
        max_repair_attempts: 1,
        continue_after_failure: false,
        require_screenshots: false,
        screenshot_commands: Vec::new(),
        verification_commands: vec![vec!["/usr/bin/env", "true"]],
        workspace_isolation: "direct",
        initialize_git_repo: false,
        runtime_supervision: RuntimeSupervisionOptions::default(),
        services: vec![default_service()],
        stacks: Vec::new(),
    })?;
    fixture.rewrite_config(|config| {
        config
            .lines()
            .map(|line| {
                if line.starts_with("binary = ") {
                    "binary = \"/usr/bin/false\"".to_string()
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    })?;

    let output = fixture.run("Fail when the codex worker process exits non-zero.\n", None)?;
    fixture.assert_failure_contains(&output, "codex stage plan failed with status")?;
    fixture.assert_failure_contains(&output, "stdout_log:")?;
    fixture.assert_failure_contains(&output, "stderr_log:")?;
    fixture.assert_failure_contains(&output, "stderr_excerpt:")?;

    let run_root = fixture.single_run_root()?;
    assert!(run_root.join("worker/prompts/plan-01.md").exists());

    Ok(())
}

#[derive(Clone, Debug)]
enum WorkerMode<'a> {
    Simulated { evaluator_statuses: Vec<&'a str> },
    CodexCliFake { scenario: FakeCodexScenario },
}

#[derive(Clone, Debug)]
struct RuntimeSupervisionOptions {
    enabled: bool,
    startup_timeout_secs: u64,
    readiness_poll_interval_ms: u64,
    shutdown_grace_period_secs: u64,
}

impl Default for RuntimeSupervisionOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            startup_timeout_secs: 30,
            readiness_poll_interval_ms: 250,
            shutdown_grace_period_secs: 5,
        }
    }
}

#[derive(Clone, Debug)]
struct ServiceFixture<'a> {
    name: &'a str,
    start: Vec<&'a str>,
    working_dir: &'a str,
    ready_url: Option<&'a str>,
    ready_command: Option<Vec<&'a str>>,
}

#[derive(Clone, Debug)]
struct StackFixture<'a> {
    name: &'a str,
    up: Vec<&'a str>,
    down: Vec<&'a str>,
    working_dir: &'a str,
    ready_url: Option<&'a str>,
    ready_command: Option<Vec<&'a str>>,
}

#[derive(Clone, Debug)]
struct ScreenshotFixture<'a> {
    name: &'a str,
    command: Vec<&'a str>,
}

#[derive(Clone, Debug)]
struct SmokeFixtureOptions<'a> {
    worker_mode: WorkerMode<'a>,
    planner_worker_mode: Option<WorkerMode<'a>>,
    max_repair_attempts: usize,
    continue_after_failure: bool,
    require_screenshots: bool,
    screenshot_commands: Vec<ScreenshotFixture<'a>>,
    verification_commands: Vec<Vec<&'a str>>,
    workspace_isolation: &'a str,
    initialize_git_repo: bool,
    runtime_supervision: RuntimeSupervisionOptions,
    services: Vec<ServiceFixture<'a>>,
    stacks: Vec<StackFixture<'a>>,
}

struct SmokeFixture {
    _temp: TempDir,
    project_root: PathBuf,
    config_path: PathBuf,
    request_file: PathBuf,
    workspace_dir: PathBuf,
    runs_dir: PathBuf,
    loopsmith_home: PathBuf,
}

impl SmokeFixture {
    fn new(
        evaluator_statuses: &[&str],
        max_repair_attempts: usize,
    ) -> Result<Self, Box<dyn Error>> {
        Self::new_with_options(SmokeFixtureOptions {
            worker_mode: WorkerMode::Simulated {
                evaluator_statuses: evaluator_statuses.to_vec(),
            },
            planner_worker_mode: None,
            max_repair_attempts,
            continue_after_failure: false,
            require_screenshots: false,
            screenshot_commands: Vec::new(),
            verification_commands: vec![vec!["/usr/bin/env", "true"]],
            workspace_isolation: "direct",
            initialize_git_repo: false,
            runtime_supervision: RuntimeSupervisionOptions::default(),
            services: vec![default_service()],
            stacks: Vec::new(),
        })
    }

    fn new_with_commands(
        evaluator_statuses: &[&str],
        max_repair_attempts: usize,
        verification_commands: &[Vec<&str>],
    ) -> Result<Self, Box<dyn Error>> {
        Self::new_with_options(SmokeFixtureOptions {
            worker_mode: WorkerMode::Simulated {
                evaluator_statuses: evaluator_statuses.to_vec(),
            },
            planner_worker_mode: None,
            max_repair_attempts,
            continue_after_failure: false,
            require_screenshots: false,
            screenshot_commands: Vec::new(),
            verification_commands: verification_commands.to_vec(),
            workspace_isolation: "direct",
            initialize_git_repo: false,
            runtime_supervision: RuntimeSupervisionOptions::default(),
            services: vec![default_service()],
            stacks: Vec::new(),
        })
    }

    fn new_with_options(options: SmokeFixtureOptions<'_>) -> Result<Self, Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let project_root = temp.path().to_path_buf();
        let config_dir = project_root.join("config");
        let bin_dir = project_root.join("bin");
        let prompts_dir = project_root.join("prompts");
        let schemas_dir = project_root.join("schemas");
        let workspace_dir = project_root.join("workspace");
        let runs_dir = project_root.join(".loopsmith-runs");
        let loopsmith_home = project_root.join(".loopsmith-home");

        for dir in [
            &config_dir,
            &bin_dir,
            &prompts_dir,
            &schemas_dir,
            &workspace_dir,
            &runs_dir,
            &loopsmith_home,
        ] {
            fs::create_dir_all(dir)?;
        }

        let worker_toml =
            render_worker_mode_toml(&bin_dir, "worker", &options.worker_mode, "default")?;
        let planner_worker_toml = options
            .planner_worker_mode
            .as_ref()
            .map(|worker_mode| {
                render_worker_mode_toml(&bin_dir, "worker.planner", worker_mode, "planner")
            })
            .transpose()?
            .unwrap_or_default();
        let command_list = options
            .verification_commands
            .iter()
            .map(|command| {
                let rendered = command
                    .iter()
                    .map(|arg| format!("\"{arg}\""))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("  [{rendered}]")
            })
            .collect::<Vec<_>>()
            .join(",\n");
        let runtime_supervision_toml = format!(
            r#"[runtime.supervision]
enabled = {}
startup_timeout_secs = {}
readiness_poll_interval_ms = {}
shutdown_grace_period_secs = {}
"#,
            options.runtime_supervision.enabled,
            options.runtime_supervision.startup_timeout_secs,
            options.runtime_supervision.readiness_poll_interval_ms,
            options.runtime_supervision.shutdown_grace_period_secs,
        );
        let services_toml = options
            .services
            .iter()
            .map(render_service_toml)
            .collect::<Vec<_>>()
            .join("\n\n");
        let stacks_toml = options
            .stacks
            .iter()
            .map(render_stack_toml)
            .collect::<Vec<_>>()
            .join("\n\n");
        let screenshot_toml = options
            .screenshot_commands
            .iter()
            .map(render_screenshot_toml)
            .collect::<Vec<_>>()
            .join("\n\n");

        fs::write(
            config_dir.join("harness.toml"),
            format!(
                r#"
[project]
root_dir = ".."

[storage]
runs_dir = ".loopsmith-runs"

[workspace]
isolation = "{}"

{worker_toml}

{planner_worker_toml}

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
max_repair_attempts = {}
continue_after_failure = {continue_after_failure}

{runtime_supervision_toml}

{services_toml}

{stacks_toml}

[evaluator]
dimensions = ["correctness", "ux"]
require_screenshots = {}
commands = [
{command_list}
]

{screenshot_toml}
"#,
                options.workspace_isolation,
                options.max_repair_attempts,
                options.require_screenshots,
                continue_after_failure = options.continue_after_failure,
                worker_toml = worker_toml.trim_end(),
                planner_worker_toml = planner_worker_toml.trim_end(),
                runtime_supervision_toml = runtime_supervision_toml.trim_end(),
                services_toml = services_toml.trim_end(),
                stacks_toml = stacks_toml.trim_end(),
                screenshot_toml = screenshot_toml.trim_end(),
            ),
        )?;

        fs::write(prompts_dir.join("planner.md"), "planner")?;
        fs::write(prompts_dir.join("builder.md"), "builder")?;
        fs::write(prompts_dir.join("evaluator.md"), "evaluator")?;

        fs::write(
            schemas_dir.join("planner-output.json"),
            r#"{"type":"object","required":["goal","features","risks","checkpoints"]}"#,
        )?;
        fs::write(
            schemas_dir.join("builder-handoff.json"),
            r#"{"type":"object","required":["summary","changed_files","verification","open_questions"]}"#,
        )?;
        fs::write(
            schemas_dir.join("qa-report.json"),
            r#"{"type":"object","required":["status","summary","findings","next_actions","checks"]}"#,
        )?;

        if options.initialize_git_repo {
            fs::write(workspace_dir.join("README.md"), "workspace\n")?;
            run_ok(
                Command::new("git").arg("init").arg(&workspace_dir),
                "git init fixture workspace",
            )?;
            run_ok(
                Command::new("git")
                    .arg("-C")
                    .arg(&workspace_dir)
                    .arg("config")
                    .arg("user.email")
                    .arg("test@example.com"),
                "git config user.email",
            )?;
            run_ok(
                Command::new("git")
                    .arg("-C")
                    .arg(&workspace_dir)
                    .arg("config")
                    .arg("user.name")
                    .arg("Test User"),
                "git config user.name",
            )?;
            run_ok(
                Command::new("git")
                    .arg("-C")
                    .arg(&workspace_dir)
                    .arg("add")
                    .arg("."),
                "git add",
            )?;
            run_ok(
                Command::new("git")
                    .arg("-C")
                    .arg(&workspace_dir)
                    .arg("commit")
                    .arg("-m")
                    .arg("init"),
                "git commit",
            )?;
        }

        if uses_fake_codex_worker(&options.worker_mode)
            || options
                .planner_worker_mode
                .as_ref()
                .is_some_and(uses_fake_codex_worker)
        {
            seed_cached_workspace_profile(&workspace_dir)?;
        }

        let request_file = project_root.join("request.md");

        Ok(Self {
            _temp: temp,
            project_root,
            config_path: config_dir.join("harness.toml"),
            request_file,
            workspace_dir,
            runs_dir,
            loopsmith_home,
        })
    }

    fn run(&self, request: &str, feature_limit: Option<usize>) -> Result<Output, Box<dyn Error>> {
        fs::write(&self.request_file, request)?;

        let mut command = Command::new(env!("CARGO_BIN_EXE_loopsmith"));
        command
            .current_dir(&self.project_root)
            .env("LOOPSMITH_HOME", &self.loopsmith_home)
            .arg("run")
            .arg("--config")
            .arg(&self.config_path)
            .arg("--workspace")
            .arg(&self.workspace_dir)
            .arg("--request-file")
            .arg(&self.request_file);

        if let Some(limit) = feature_limit {
            command.arg("--feature-limit").arg(limit.to_string());
        }

        Ok(command.output()?)
    }

    fn resume(&self, run_root: &Path) -> Result<Output, Box<dyn Error>> {
        Ok(Command::new(env!("CARGO_BIN_EXE_loopsmith"))
            .current_dir(&self.project_root)
            .env("LOOPSMITH_HOME", &self.loopsmith_home)
            .arg("resume")
            .arg("--config")
            .arg(&self.config_path)
            .arg("--run-root")
            .arg(run_root)
            .output()?)
    }

    fn inspect(&self, run_root: &Path) -> Result<Output, Box<dyn Error>> {
        Ok(Command::new(env!("CARGO_BIN_EXE_loopsmith"))
            .current_dir(&self.project_root)
            .env("LOOPSMITH_HOME", &self.loopsmith_home)
            .arg("inspect")
            .arg("--config")
            .arg(&self.config_path)
            .arg("--run-root")
            .arg(run_root)
            .output()?)
    }

    fn single_run_root(&self) -> Result<PathBuf, Box<dyn Error>> {
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
        assert_eq!(
            run_roots.len(),
            1,
            "expected exactly one run root under {}",
            self.runs_dir.display()
        );
        Ok(run_roots.remove(0))
    }

    fn rewrite_config<F>(&self, rewrite: F) -> Result<(), Box<dyn Error>>
    where
        F: FnOnce(String) -> String,
    {
        let config = fs::read_to_string(&self.config_path)?;
        fs::write(&self.config_path, rewrite(config))?;
        Ok(())
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

    fn assert_failure_contains(&self, output: &Output, needle: &str) -> Result<(), Box<dyn Error>> {
        assert!(
            !output.status.success(),
            "expected command failure but it succeeded\nstdout:\n{}\n\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(needle),
            "expected stderr to contain `{needle}`\nstdout:\n{}\n\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            stderr
        );
        Ok(())
    }

    fn parse_run(&self, output: &Output) -> Result<RunInspection, Box<dyn Error>> {
        let stdout = String::from_utf8(output.stdout.clone())?;
        let run_root = PathBuf::from(
            line_value(&stdout, "run_root: ").expect("missing run_root in CLI output"),
        );
        let state = read_json(&run_root.join("run-state.json"))?;
        let manifest = read_json(&run_root.join("manifest.json"))?;
        let plan = read_json(&run_root.join("plan.json"))?;

        Ok(RunInspection {
            stdout,
            run_root,
            state,
            manifest,
            plan,
            runs_dir: self.runs_dir.clone(),
            feature_lines: line_values_prefix(
                &String::from_utf8(output.stdout.clone())?,
                "feature=",
            ),
            stage_lines: line_values_prefix(&String::from_utf8(output.stdout.clone())?, "stage="),
        })
    }
}

struct RunInspection {
    stdout: String,
    run_root: PathBuf,
    state: Value,
    manifest: Value,
    plan: Value,
    runs_dir: PathBuf,
    feature_lines: Vec<String>,
    stage_lines: Vec<String>,
}

impl RunInspection {
    fn assert_root_layout(&self) -> Result<(), Box<dyn Error>> {
        assert_eq!(self.run_root.parent(), Some(self.runs_dir.as_path()));
        assert!(self.run_root.starts_with(&self.runs_dir));
        self.assert_exists("request.md");
        self.assert_exists("plan.json");
        self.assert_exists("runtime-plan.json");
        self.assert_exists("run-state.json");
        self.assert_exists("manifest.json");
        self.assert_exists("worker/prompts/plan-01.md");
        assert_eq!(self.state, self.manifest);
        assert_eq!(self.state["run_root"], json_path(&self.run_root));
        assert_eq!(
            self.state["request_file"],
            json_path(&self.run_root.join("request.md"))
        );
        assert_eq!(
            self.state["plan_file"],
            json_path(&self.run_root.join("plan.json"))
        );
        assert_eq!(
            self.state["runtime_plan_file"],
            json_path(&self.run_root.join("runtime-plan.json"))
        );
        Ok(())
    }

    fn assert_stage_count(&self, expected: usize) -> Result<(), Box<dyn Error>> {
        let feature_stage_count: usize = self.state["features"]
            .as_array()
            .expect("features should be an array")
            .iter()
            .map(|feature| {
                feature["stages"]
                    .as_array()
                    .expect("stages should be an array")
                    .len()
            })
            .sum();
        let plan_stage_count = usize::from(!self.state["plan_stage"].is_null());
        assert_eq!(self.stage_lines.len(), expected);
        assert_eq!(feature_stage_count + plan_stage_count, expected);
        Ok(())
    }

    fn assert_cli_field(&self, label: &str, expected: &str) -> Result<(), Box<dyn Error>> {
        let actual = line_value(&self.stdout, &format!("{label}: "))
            .unwrap_or_else(|| panic!("missing {label} in CLI output"));
        assert_eq!(actual, expected);
        Ok(())
    }

    fn assert_plan_goal(&self, expected: &str) -> Result<(), Box<dyn Error>> {
        assert_eq!(self.plan["goal"], expected);
        Ok(())
    }

    fn assert_feature_count(&self, expected: usize) -> Result<(), Box<dyn Error>> {
        assert_eq!(self.feature_lines.len(), expected);
        assert_eq!(
            self.state["features"]
                .as_array()
                .expect("features should be an array")
                .len(),
            expected
        );
        Ok(())
    }

    fn assert_feature_state(
        &self,
        index: usize,
        feature_id: &str,
        status: &str,
        phase: &str,
        repairs: usize,
        qa: &str,
    ) -> Result<(), Box<dyn Error>> {
        let feature = self.state["features"]
            .as_array()
            .expect("features should be an array")
            .get(index)
            .expect("feature index should exist");

        assert_eq!(feature["feature_id"], feature_id);
        assert_eq!(feature["status"], status);
        assert_eq!(feature["phase"], phase);
        assert_eq!(feature["repair_attempts_used"], repairs);
        if qa == "-" {
            assert!(feature["last_qa_status"].is_null());
        } else {
            assert_eq!(feature["last_qa_status"], qa);
        }
        Ok(())
    }

    fn assert_source_workspace(&self, expected: &Path) -> Result<(), Box<dyn Error>> {
        assert_eq!(self.state["source_workspace"], json_path(expected));
        Ok(())
    }

    fn assert_execution_workspace_matches_source(
        &self,
        source_workspace: &Path,
    ) -> Result<(), Box<dyn Error>> {
        let execution_workspace = self.state["execution_workspace"]
            .as_str()
            .expect("execution_workspace should be a string");
        assert_eq!(execution_workspace, source_workspace.display().to_string());
        Ok(())
    }

    fn rewind_to_pending_plan(&self) -> Result<(), Box<dyn Error>> {
        let mut state = self.state.clone();
        state["lifecycle"] = json!("running");
        state["final_status"] = Value::Null;
        state["current_feature_index"] = json!(0);
        state["plan_stage"] = Value::Null;
        state["features"] = Value::Array(Vec::new());

        let state_bytes = serde_json::to_vec_pretty(&state)?;
        fs::write(self.run_root.join("run-state.json"), &state_bytes)?;
        fs::write(self.run_root.join("manifest.json"), &state_bytes)?;

        Ok(())
    }

    fn rewind_to_pending_evaluate(&self) -> Result<(), Box<dyn Error>> {
        let mut state = self.state.clone();
        state["lifecycle"] = json!("running");
        state["final_status"] = Value::Null;
        state["current_feature_index"] = json!(0);

        let feature = state["features"]
            .as_array_mut()
            .expect("features should be an array")
            .get_mut(0)
            .expect("feature 0 should exist");
        feature["status"] = json!("running");
        feature["phase"] = json!("pending_evaluate");
        feature["repair_attempts_used"] = json!(0);
        feature["next_evaluate_attempt"] = json!(1);
        feature["last_qa_status"] = Value::Null;
        feature["stages"] = Value::Array(vec![
            feature["stages"]
                .as_array()
                .expect("stages should be an array")
                .iter()
                .find(|stage| stage["stage"] == "build")
                .cloned()
                .expect("build stage should exist"),
        ]);

        let state_bytes = serde_json::to_vec_pretty(&state)?;
        fs::write(self.run_root.join("run-state.json"), &state_bytes)?;
        fs::write(self.run_root.join("manifest.json"), &state_bytes)?;

        for stale in [
            "features/01-feature-001/qa-report.json",
            "features/01-feature-001/worker/evaluate-01-result.json",
            "features/01-feature-001/worker/outputs/evaluate-01-last-message.json",
        ] {
            let path = self.run_root.join(stale);
            if path.exists() {
                fs::remove_file(path)?;
            }
        }

        Ok(())
    }

    fn rewind_to_pending_repair(&self) -> Result<(), Box<dyn Error>> {
        let mut state = self.state.clone();
        state["lifecycle"] = json!("running");
        state["final_status"] = Value::Null;
        state["current_feature_index"] = json!(0);

        let feature = state["features"]
            .as_array_mut()
            .expect("features should be an array")
            .get_mut(0)
            .expect("feature 0 should exist");
        feature["status"] = json!("running");
        feature["phase"] = json!("pending_repair");
        feature["repair_attempts_used"] = json!(0);
        feature["next_evaluate_attempt"] = json!(2);
        feature["last_qa_status"] = json!("fail");
        feature["last_session_id"] = json!("simulated-build-01");
        feature["stages"] = Value::Array(
            feature["stages"]
                .as_array()
                .expect("stages should be an array")
                .iter()
                .filter(|stage| {
                    matches!(stage["stage"].as_str(), Some("build") | Some("evaluate"))
                        && stage["attempt"] == 1
                })
                .cloned()
                .collect(),
        );

        let state_bytes = serde_json::to_vec_pretty(&state)?;
        fs::write(self.run_root.join("run-state.json"), &state_bytes)?;
        fs::write(self.run_root.join("manifest.json"), &state_bytes)?;
        fs::copy(
            self.run_root
                .join("features/01-feature-001/worker/outputs/build-01-last-message.json"),
            self.run_root
                .join("features/01-feature-001/builder-handoff.json"),
        )?;
        fs::copy(
            self.run_root
                .join("features/01-feature-001/worker/outputs/evaluate-01-last-message.json"),
            self.run_root.join("features/01-feature-001/qa-report.json"),
        )?;

        for stale in [
            "features/01-feature-001/worker/repair-01-result.json",
            "features/01-feature-001/worker/evaluate-02-result.json",
            "features/01-feature-001/worker/outputs/repair-01-last-message.json",
            "features/01-feature-001/worker/outputs/evaluate-02-last-message.json",
        ] {
            let path = self.run_root.join(stale);
            if path.exists() {
                fs::remove_file(path)?;
            }
        }

        Ok(())
    }

    fn read_json(&self, relative: &str) -> Result<Value, Box<dyn Error>> {
        read_json(&self.run_root.join(relative))
    }

    fn assert_exists(&self, relative: &str) {
        let path = self.run_root.join(relative);
        assert!(path.exists(), "expected {} to exist", path.display());
    }

    fn assert_missing(&self, relative: &str) {
        let path = self.run_root.join(relative);
        assert!(!path.exists(), "expected {} to be absent", path.display());
    }

    fn assert_feature_line(&self, expected_prefix: &str) {
        assert!(
            self.feature_lines
                .iter()
                .any(|line| line.starts_with(expected_prefix)),
            "missing feature line prefix `{expected_prefix}` in:\n{}",
            self.feature_lines.join("\n")
        );
    }

    fn assert_stage_line(&self, expected_prefix: &str) -> Result<(), Box<dyn Error>> {
        assert!(
            self.stage_lines
                .iter()
                .any(|line| line.starts_with(expected_prefix)),
            "missing stage line prefix `{expected_prefix}` in:\n{}",
            self.stage_lines.join("\n")
        );
        Ok(())
    }
}

fn read_json(path: &Path) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn line_value<'a>(stdout: &'a str, prefix: &str) -> Option<&'a str> {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix(prefix).map(str::trim))
}

fn line_values_prefix(stdout: &str, prefix: &str) -> Vec<String> {
    stdout
        .lines()
        .filter(|line| line.starts_with(prefix))
        .map(str::to_string)
        .collect()
}

fn json_path(path: &Path) -> Value {
    Value::String(path.display().to_string())
}

fn json_string_array(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("json value should be an array")
        .iter()
        .map(|item| {
            item.as_str()
                .expect("array item should be a string")
                .to_string()
        })
        .collect()
}

fn run_ok(command: &mut Command, label: &str) -> Result<(), Box<dyn Error>> {
    let output = command.output()?;
    assert!(
        output.status.success(),
        "{label} failed\nstdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

fn uses_fake_codex_worker(worker_mode: &WorkerMode<'_>) -> bool {
    matches!(worker_mode, WorkerMode::CodexCliFake { .. })
}

fn seed_cached_workspace_profile(workspace_dir: &Path) -> Result<(), Box<dyn Error>> {
    let store = WorkspaceDiscoveryStore::new(workspace_dir);
    let scan = scan_workspace(workspace_dir)?;
    let profile = WorkspaceDiscoveryRequest {
        scan: scan.clone(),
        previous_profile: None,
    }
    .synthesize_profile();
    let profile_fingerprint = profile_fingerprint(&profile)?;

    store.save_scan(&scan)?;
    store.save_profile(&profile)?;
    store.save_status(&WorkspaceDiscoveryStatus {
        workspace_path: workspace_dir.to_path_buf(),
        scan_path: store.scan_path(),
        profile_path: store.profile_path(),
        workspace_fingerprint: scan.workspace_fingerprint,
        profile_fingerprint: Some(profile_fingerprint),
        last_scanned_at: scan.scanned_at,
        last_refreshed_at: Some(profile.generated_at),
        last_refresh_error: None,
        used_fallback_profile: false,
        current_phase: WorkspaceDiscoveryPhase::Ready,
    })?;

    Ok(())
}

#[derive(Clone, Debug)]
struct FakeCodexScenario {
    routes: Vec<FakeCodexRoute>,
}

#[derive(Clone, Debug)]
struct FakeCodexRoute {
    output_pattern: String,
    output_body: String,
    thread_id: Option<String>,
}

fn default_service<'a>() -> ServiceFixture<'a> {
    ServiceFixture {
        name: "web",
        start: vec!["pnpm", "dev"],
        working_dir: ".",
        ready_url: Some("http://127.0.0.1:3000/"),
        ready_command: None,
    }
}

fn render_service_toml(service: &ServiceFixture<'_>) -> String {
    let mut lines = vec![
        "[[runtime.services]]".to_string(),
        format!("name = {:?}", service.name),
        format!("start = {}", toml_string_array(&service.start)),
        format!("working_dir = {:?}", service.working_dir),
    ];

    if let Some(ready_url) = service.ready_url {
        lines.push(format!("ready_url = {:?}", ready_url));
    }

    if let Some(ready_command) = &service.ready_command {
        lines.push(format!(
            "ready_command = {}",
            toml_string_array(ready_command)
        ));
    }

    lines.join("\n")
}

fn render_stack_toml(stack: &StackFixture<'_>) -> String {
    let mut lines = vec![
        "[[runtime.stacks]]".to_string(),
        format!("name = {:?}", stack.name),
        format!("up = {}", toml_string_array(&stack.up)),
        format!("down = {}", toml_string_array(&stack.down)),
        format!("working_dir = {:?}", stack.working_dir),
    ];

    if let Some(ready_url) = stack.ready_url {
        lines.push(format!("ready_url = {:?}", ready_url));
    }

    if let Some(ready_command) = &stack.ready_command {
        lines.push(format!(
            "ready_command = {}",
            toml_string_array(ready_command)
        ));
    }

    lines.join("\n")
}

fn render_screenshot_toml(screenshot: &ScreenshotFixture<'_>) -> String {
    [
        "[[evaluator.screenshots]]".to_string(),
        format!("name = {:?}", screenshot.name),
        format!("command = {}", toml_string_array(&screenshot.command)),
    ]
    .join("\n")
}

fn toml_string_array<T: AsRef<str>>(items: &[T]) -> String {
    let rendered = items
        .iter()
        .map(|item| format!("{:?}", item.as_ref()))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{rendered}]")
}

fn write_fake_codex_script(
    bin_dir: &Path,
    script_name: &str,
    scenario: &FakeCodexScenario,
) -> Result<PathBuf, Box<dyn Error>> {
    let script_path = bin_dir.join(format!("fake-codex-{script_name}.sh"));
    let mut script = String::from(
        "#!/bin/sh\nset -eu\nOUTPUT=\"\"\nPREV=\"\"\nfor ARG in \"$@\"; do\n  if [ \"$PREV\" = \"o\" ]; then\n    OUTPUT=\"$ARG\"\n    PREV=\"\"\n    continue\n  fi\n  if [ \"$ARG\" = \"-o\" ]; then\n    PREV=\"o\"\n  fi\ndone\ncat >/dev/null\ncase \"$OUTPUT\" in\n",
    );

    script.push_str(
        "  */.loopsmith/discovery/worker/workspace-profile.json)\n    cat >\"$OUTPUT\" <<'__CODEX_JSON__'\n",
    );
    script.push_str(&workspace_profile_json());
    script.push_str("\n__CODEX_JSON__\n    ;;\n");

    for route in &scenario.routes {
        script.push_str(&format!("  {}\n", route.output_pattern));
        script.push_str(&format!(
            "    cat >\"$OUTPUT\" <<'__CODEX_JSON__'\n{}\n__CODEX_JSON__\n",
            route.output_body
        ));
        if let Some(thread_id) = &route.thread_id {
            script.push_str(&format!(
                "    printf '%s\\n' '{}'\n",
                json!({"type": "thread.started", "thread_id": thread_id})
            ));
        }
        script.push_str("    ;;\n");
    }

    script.push_str(
        "  *)\n    echo \"unexpected fake codex output target: $OUTPUT\" >&2\n    exit 1\n    ;;\nesac\n",
    );

    fs::write(&script_path, script)?;
    run_ok(
        Command::new("chmod").arg("+x").arg(&script_path),
        "chmod fake codex script",
    )?;
    Ok(script_path)
}

fn render_worker_mode_toml(
    bin_dir: &Path,
    table_path: &str,
    worker_mode: &WorkerMode<'_>,
    script_name: &str,
) -> Result<String, Box<dyn Error>> {
    Ok(match worker_mode {
        WorkerMode::Simulated { evaluator_statuses } => {
            let status_list = evaluator_statuses
                .iter()
                .map(|status| format!("\"{status}\""))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                r#"[{table_path}]
kind = "simulated"

[{table_path}.simulation]
evaluator_statuses = [{status_list}]
session_prefix = "simulated"
"#
            )
        }
        WorkerMode::CodexCliFake { scenario } => {
            let binary = write_fake_codex_script(bin_dir, script_name, scenario)?;
            format!(
                r#"[{table_path}]
kind = "codex_cli"

[{table_path}.codex]
binary = "{}"
model = "gpt-5.4"
sandbox = "workspace-write"
full_auto = true
skip_git_repo_check = true
resume_sessions = true
"#,
                binary.display()
            )
        }
    })
}

fn fake_codex_repair_pass_scenario() -> FakeCodexScenario {
    FakeCodexScenario {
        routes: vec![
            FakeCodexRoute {
                output_pattern: "*/worker/outputs/plan-01-last-message.json)".to_string(),
                output_body: plan_output_json(&["feature-001"]).to_string(),
                thread_id: Some("fake-plan-01".to_string()),
            },
            FakeCodexRoute {
                output_pattern:
                    "*/features/01-feature-001/worker/outputs/build-01-last-message.json)"
                        .to_string(),
                output_body: builder_handoff_json("fake build handoff"),
                thread_id: Some("fake-build-01".to_string()),
            },
            FakeCodexRoute {
                output_pattern:
                    "*/features/01-feature-001/worker/outputs/evaluate-01-last-message.json)"
                        .to_string(),
                output_body: qa_report_json("fail", "fake qa fail"),
                thread_id: Some("fake-evaluate-01".to_string()),
            },
            FakeCodexRoute {
                output_pattern:
                    "*/features/01-feature-001/worker/outputs/repair-01-last-message.json)"
                        .to_string(),
                output_body: builder_handoff_json("fake repair handoff"),
                thread_id: Some("fake-repair-01".to_string()),
            },
            FakeCodexRoute {
                output_pattern:
                    "*/features/01-feature-001/worker/outputs/evaluate-02-last-message.json)"
                        .to_string(),
                output_body: qa_report_json("pass", "fake qa pass"),
                thread_id: Some("fake-evaluate-02".to_string()),
            },
        ],
    }
}

fn fake_codex_repair_pass_without_thread_ids_scenario() -> FakeCodexScenario {
    let mut scenario = fake_codex_repair_pass_scenario();
    for route in &mut scenario.routes {
        route.thread_id = None;
    }
    scenario
}

fn fake_codex_planner_override_scenario() -> FakeCodexScenario {
    FakeCodexScenario {
        routes: vec![FakeCodexRoute {
            output_pattern: "*/worker/outputs/plan-01-last-message.json)".to_string(),
            output_body: plan_output_json(&["feature-001"]).to_string(),
            thread_id: Some("fake-plan-override-01".to_string()),
        }],
    }
}

fn fake_codex_two_feature_second_fails_scenario() -> FakeCodexScenario {
    FakeCodexScenario {
        routes: vec![
            FakeCodexRoute {
                output_pattern: "*/worker/outputs/plan-01-last-message.json)".to_string(),
                output_body: plan_output_json(&["feature-001", "feature-002"]).to_string(),
                thread_id: Some("fake-plan-01".to_string()),
            },
            FakeCodexRoute {
                output_pattern:
                    "*/features/01-feature-001/worker/outputs/build-01-last-message.json)"
                        .to_string(),
                output_body: builder_handoff_json("feature one build"),
                thread_id: Some("fake-build-01".to_string()),
            },
            FakeCodexRoute {
                output_pattern:
                    "*/features/01-feature-001/worker/outputs/evaluate-01-last-message.json)"
                        .to_string(),
                output_body: qa_report_json("pass", "feature one qa"),
                thread_id: Some("fake-evaluate-01".to_string()),
            },
            FakeCodexRoute {
                output_pattern:
                    "*/features/02-feature-002/worker/outputs/build-01-last-message.json)"
                        .to_string(),
                output_body: builder_handoff_json("feature two build"),
                thread_id: Some("fake-build-02".to_string()),
            },
            FakeCodexRoute {
                output_pattern:
                    "*/features/02-feature-002/worker/outputs/evaluate-01-last-message.json)"
                        .to_string(),
                output_body: qa_report_json("fail", "feature two qa failed"),
                thread_id: Some("fake-evaluate-02".to_string()),
            },
        ],
    }
}

fn fake_codex_invalid_plan_scenario() -> FakeCodexScenario {
    FakeCodexScenario {
        routes: vec![FakeCodexRoute {
            output_pattern: "*/worker/outputs/plan-01-last-message.json)".to_string(),
            output_body: json!({"unexpected": true}).to_string(),
            thread_id: Some("fake-plan-01".to_string()),
        }],
    }
}

fn fake_codex_invalid_build_scenario() -> FakeCodexScenario {
    FakeCodexScenario {
        routes: vec![
            FakeCodexRoute {
                output_pattern: "*/worker/outputs/plan-01-last-message.json)".to_string(),
                output_body: plan_output_json(&["feature-001"]).to_string(),
                thread_id: Some("fake-plan-01".to_string()),
            },
            FakeCodexRoute {
                output_pattern:
                    "*/features/01-feature-001/worker/outputs/build-01-last-message.json)"
                        .to_string(),
                output_body: json!({"unexpected": true}).to_string(),
                thread_id: Some("fake-build-01".to_string()),
            },
        ],
    }
}

fn fake_codex_invalid_evaluate_scenario() -> FakeCodexScenario {
    FakeCodexScenario {
        routes: vec![
            FakeCodexRoute {
                output_pattern: "*/worker/outputs/plan-01-last-message.json)".to_string(),
                output_body: plan_output_json(&["feature-001"]).to_string(),
                thread_id: Some("fake-plan-01".to_string()),
            },
            FakeCodexRoute {
                output_pattern:
                    "*/features/01-feature-001/worker/outputs/build-01-last-message.json)"
                        .to_string(),
                output_body: builder_handoff_json("feature build"),
                thread_id: Some("fake-build-01".to_string()),
            },
            FakeCodexRoute {
                output_pattern:
                    "*/features/01-feature-001/worker/outputs/evaluate-01-last-message.json)"
                        .to_string(),
                output_body: json!({"unexpected": true}).to_string(),
                thread_id: Some("fake-evaluate-01".to_string()),
            },
        ],
    }
}

fn fake_codex_invalid_repair_scenario() -> FakeCodexScenario {
    FakeCodexScenario {
        routes: vec![
            FakeCodexRoute {
                output_pattern: "*/worker/outputs/plan-01-last-message.json)".to_string(),
                output_body: plan_output_json(&["feature-001"]).to_string(),
                thread_id: Some("fake-plan-01".to_string()),
            },
            FakeCodexRoute {
                output_pattern:
                    "*/features/01-feature-001/worker/outputs/build-01-last-message.json)"
                        .to_string(),
                output_body: builder_handoff_json("feature build"),
                thread_id: Some("fake-build-01".to_string()),
            },
            FakeCodexRoute {
                output_pattern:
                    "*/features/01-feature-001/worker/outputs/evaluate-01-last-message.json)"
                        .to_string(),
                output_body: qa_report_json("fail", "feature qa failed"),
                thread_id: Some("fake-evaluate-01".to_string()),
            },
            FakeCodexRoute {
                output_pattern:
                    "*/features/01-feature-001/worker/outputs/repair-01-last-message.json)"
                        .to_string(),
                output_body: json!({"unexpected": true}).to_string(),
                thread_id: Some("fake-repair-01".to_string()),
            },
        ],
    }
}

fn plan_output_json(feature_ids: &[&str]) -> serde_json::Value {
    json!({
        "goal": "fake codex plan",
        "features": feature_ids
            .iter()
            .enumerate()
            .map(|(index, feature_id)| json!({
                "id": feature_id,
                "title": format!("Feature {}", index + 1),
                "summary": format!("Implement {feature_id}"),
                "acceptance_criteria": ["ship it"],
            }))
            .collect::<Vec<_>>(),
        "risks": [],
        "checkpoints": [],
    })
}

fn builder_handoff_json(summary: &str) -> String {
    json!({
        "summary": summary,
        "changed_files": [],
        "verification": [],
        "open_questions": [],
    })
    .to_string()
}

fn qa_report_json(status: &str, summary: &str) -> String {
    json!({
        "status": status,
        "summary": summary,
        "findings": [],
        "next_actions": [],
        "checks": [],
    })
    .to_string()
}

fn workspace_profile_json() -> String {
    json!({
        "workspace_path": ".",
        "generated_at": "2026-04-04T00:00:00Z",
        "summary": "Fake codex discovery profile",
        "key_concepts": ["Pre-run discovery context"],
        "tech_stack": [],
        "repositories": [],
        "dependency_relationships": [],
        "api_contracts": [],
        "layering": {
            "summary": "No strong layer names were detected from file layout alone.",
            "layers": [],
            "allowed_dependency_directions": [],
            "unresolved_ambiguities": [],
        },
        "user_journeys": [],
        "e2e_test_cases": [],
        "auth": [],
        "coding_conventions": [],
        "commands": {
            "build": [],
            "test": [],
            "dev": [],
        },
        "risks": [],
    })
    .to_string()
}

fn assert_fake_codex_stage_failure(
    scenario: FakeCodexScenario,
    error_substring: &str,
    artifact_to_check: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    let fixture = SmokeFixture::new_with_options(SmokeFixtureOptions {
        worker_mode: WorkerMode::CodexCliFake { scenario },
        planner_worker_mode: None,
        max_repair_attempts: 1,
        continue_after_failure: false,
        require_screenshots: false,
        screenshot_commands: Vec::new(),
        verification_commands: vec![vec!["/usr/bin/env", "true"]],
        workspace_isolation: "direct",
        initialize_git_repo: false,
        runtime_supervision: RuntimeSupervisionOptions::default(),
        services: vec![default_service()],
        stacks: Vec::new(),
    })?;
    let output = fixture.run("Fail on malformed worker output.\n", None)?;
    fixture.assert_failure_contains(&output, error_substring)?;

    let run_root = fixture.single_run_root()?;
    if let Some(artifact) = artifact_to_check {
        assert!(
            run_root.join(artifact).exists(),
            "expected {} to exist",
            run_root.join(artifact).display()
        );
    }

    Ok(())
}
