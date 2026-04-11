use std::{fs, path::Path};

use anyhow::{Context, Result};
use chrono::Utc;
use tracing::info;
use uuid::Uuid;

use crate::{
    artifacts::{FeatureLayout, FileArtifactStore, RunLayout},
    config::ResolvedConfig,
    discovery::{RunWorkspaceProfileSnapshot, WorkspaceProfile},
    domain::{
        ActiveRunStage, FeatureContract, FeatureLifecycleStatus, FeaturePhase, FeatureRunState,
        PlanDocument, PlanningRequest, PromptSnapshot, QaStatus, RunLaunchSnapshot,
        RunLifecycleStatus, RunRequest, RunStageRecord, RunState, WorkerStage,
    },
    evaluator::build_evaluation_request,
    runtime::{RuntimePlan, RuntimeSupervisor, run_screenshot_commands, run_verification_commands},
    worker::{WorkerAdapter, WorkerContext},
    workspace::WorkspaceManager,
};

pub struct HarnessController<W> {
    config: ResolvedConfig,
    artifacts: FileArtifactStore,
    worker: W,
}

impl<W> HarnessController<W>
where
    W: WorkerAdapter,
{
    pub fn new(config: ResolvedConfig, artifacts: FileArtifactStore, worker: W) -> Self {
        Self {
            config,
            artifacts,
            worker,
        }
    }

    pub async fn start_run(&self, request: RunRequest) -> Result<RunState> {
        let run_id = Uuid::new_v4();
        let layout = self.artifacts.initialize(run_id)?;
        info!(
            %run_id,
            run_root = %layout.root.display(),
            source_workspace = %request.source_workspace.display(),
            "starting harness run"
        );
        let prepared_workspace = WorkspaceManager::prepare(
            self.config.workspace.isolation,
            &request.source_workspace,
            &layout.root,
        )?;

        self.artifacts
            .write_text(&layout.request_file, &request.user_request)?;

        let runtime_plan = RuntimePlan::from_config(
            &prepared_workspace.execution_workspace,
            &self.config.runtime,
        );
        self.artifacts
            .write_json(&layout.runtime_plan_file, &runtime_plan)?;
        let requested_feature_limit = request.feature_limit.map(|limit| limit.max(1));
        let effective_feature_limit = requested_feature_limit
            .unwrap_or(self.config.runtime.feature_limit)
            .max(1);
        let feature_limit_is_hard = requested_feature_limit.is_some();

        let now = Utc::now();
        let launch_snapshot = self.build_launch_snapshot(
            &request,
            &layout.workspace_profile_file,
            now,
            effective_feature_limit,
            feature_limit_is_hard,
        )?;
        self.artifacts
            .write_json(&layout.launch_file, &launch_snapshot)?;
        self.artifacts.write_text(
            &layout.planner_prompt_file,
            &launch_snapshot.prompts.planner,
        )?;
        self.artifacts.write_text(
            &layout.builder_prompt_file,
            &launch_snapshot.prompts.builder,
        )?;
        self.artifacts.write_text(
            &layout.evaluator_prompt_file,
            &launch_snapshot.prompts.evaluator,
        )?;
        if let Some(profile) = request.workspace_profile.as_ref() {
            self.artifacts
                .write_json(&layout.workspace_profile_file, &profile.profile)?;
        }
        let mut state = RunState {
            run_id,
            run_title: truncate_title(&request.user_request, 50),
            created_at: now,
            updated_at: now,
            run_root: layout.root.clone(),
            state_file: layout.state_file.clone(),
            manifest_file: layout.manifest_file.clone(),
            launch_file: Some(layout.launch_file.clone()),
            request_file: layout.request_file.clone(),
            plan_file: layout.plan_file.clone(),
            runtime_plan_file: layout.runtime_plan_file.clone(),
            source_workspace: prepared_workspace.source_workspace,
            execution_workspace: prepared_workspace.execution_workspace,
            lifecycle: RunLifecycleStatus::Planning,
            final_status: None,
            current_feature_index: 0,
            awaiting_feature_confirmation: false,
            active_stage: None,
            plan_stage: None,
            features: Vec::new(),
        };
        self.checkpoint(&mut state)?;

        self.ensure_plan(
            &mut state,
            &layout,
            &runtime_plan,
            &request.user_request,
            effective_feature_limit,
            feature_limit_is_hard,
        )
        .await?;

        if request.confirm_before_build {
            state.awaiting_feature_confirmation = true;
            state.lifecycle = RunLifecycleStatus::Running;
            self.checkpoint(&mut state)?;
            return Ok(state);
        }

        self.drive_run_with_runtime(&mut state, &layout, &runtime_plan)
            .await?;
        Ok(state)
    }

    pub async fn resume_run(&self, run_root: impl AsRef<Path>) -> Result<RunState> {
        let run_root = run_root.as_ref();
        let layout = self.layout_from_run_root(run_root);
        let mut state: RunState =
            self.artifacts
                .read_json(&layout.state_file)
                .with_context(|| {
                    format!(
                        "failed to load run state from {}",
                        layout.state_file.display()
                    )
                })?;

        state.backfill_log_paths();

        if state.lifecycle == RunLifecycleStatus::Passed {
            return Ok(state);
        }

        if state.awaiting_feature_confirmation {
            info!(
                run_id = %state.run_id,
                "run confirmed after plan review; continuing into build"
            );
            state.awaiting_feature_confirmation = false;
            state.lifecycle = RunLifecycleStatus::Running;
            state.active_stage = None;
            self.checkpoint(&mut state)?;
        }

        if state.lifecycle == RunLifecycleStatus::Failed {
            if !self.config.runtime.continue_after_failure {
                return Ok(state);
            }

            let next_index = state
                .features
                .iter()
                .position(|f| {
                    f.status != FeatureLifecycleStatus::Failed
                        && f.status != FeatureLifecycleStatus::Passed
                })
                .unwrap_or(state.features.len());

            if next_index >= state.features.len() {
                return Ok(state);
            }

            info!(
                run_id = %state.run_id,
                next_feature_index = next_index,
                "resuming failed run with continue_after_failure"
            );
            state.current_feature_index = next_index;
            state.lifecycle = RunLifecycleStatus::Running;
            state.active_stage = None;
            self.checkpoint(&mut state)?;
        }

        let runtime_plan: RuntimePlan = self
            .artifacts
            .read_json(&layout.runtime_plan_file)
            .with_context(|| {
                format!(
                    "failed to load runtime plan from {}",
                    layout.runtime_plan_file.display()
                )
            })?;
        let request = fs::read_to_string(&layout.request_file)
            .with_context(|| format!("failed to read {}", layout.request_file.display()))?;

        if state.plan_stage.is_none() || state.features.is_empty() {
            self.ensure_plan(
                &mut state,
                &layout,
                &runtime_plan,
                &request,
                self.config.runtime.feature_limit.max(1),
                false,
            )
            .await?;
        }

        self.drive_run_with_runtime(&mut state, &layout, &runtime_plan)
            .await?;
        Ok(state)
    }

    pub fn inspect_run(&self, run_root: impl AsRef<Path>) -> Result<RunState> {
        let run_root = run_root.as_ref();
        let layout = self.layout_from_run_root(run_root);
        let mut state: RunState =
            self.artifacts
                .read_json(&layout.state_file)
                .with_context(|| {
                    format!(
                        "failed to load run state from {}",
                        layout.state_file.display()
                    )
                })?;
        state.backfill_log_paths();
        if state.run_title.is_empty() {
            if let Ok(plan) = self.artifacts.read_json::<PlanDocument>(&state.plan_file) {
                if !plan.goal.is_empty() {
                    state.run_title = truncate_title(&plan.goal, 50);
                }
            }
            if state.run_title.is_empty() {
                if let Ok(req) = std::fs::read_to_string(&state.request_file) {
                    let trimmed = req.trim();
                    if !trimmed.is_empty() {
                        state.run_title = truncate_title(trimmed, 50);
                    }
                }
            }
        }
        Ok(state)
    }

    async fn ensure_plan(
        &self,
        state: &mut RunState,
        layout: &RunLayout,
        runtime_plan: &RuntimePlan,
        user_request: &str,
        feature_limit: usize,
        feature_limit_is_hard: bool,
    ) -> Result<()> {
        if state.plan_stage.is_some() && !state.features.is_empty() {
            return Ok(());
        }

        let worker_context = self.worker_context(
            state.run_id,
            state.execution_workspace.clone(),
            layout.clone(),
            load_workspace_profile_snapshot(&self.artifacts, layout).as_ref(),
        );
        let workspace_profile = load_workspace_profile_snapshot(&self.artifacts, layout);
        let planning_request = PlanningRequest {
            user_request: user_request.to_string(),
            feature_limit,
            feature_limit_is_hard,
            service_names: runtime_plan
                .services
                .iter()
                .map(|service| service.name.clone())
                .collect(),
            verification_commands: self.config.evaluator.commands.clone(),
        };

        let plan_artifacts = layout.stage_artifacts(WorkerStage::Plan, 1);
        self.begin_stage(state, WorkerStage::Plan, plan_artifacts.attempt, None, None)?;
        info!(
            run_id = %state.run_id,
            attempt = plan_artifacts.attempt,
            workspace = %state.execution_workspace.display(),
            feature_limit,
            feature_limit_is_hard,
            "starting plan stage"
        );
        let plan_result = self
            .worker
            .plan(&worker_context, &plan_artifacts, &planning_request)
            .await?;
        self.artifacts
            .write_json(&plan_artifacts.result_file, &plan_result)?;
        state.active_stage = None;
        state.plan_stage = Some(RunStageRecord {
            stage: plan_result.stage,
            attempt: plan_artifacts.attempt,
            status: plan_result.status,
            artifact: plan_artifacts.result_file.clone(),
            stdout_log: plan_artifacts.stdout_log.clone(),
            stderr_log: plan_artifacts.stderr_log.clone(),
            session_id: plan_result.session_id.clone(),
        });

        let plan: crate::domain::PlanDocument = self
            .artifacts
            .read_json(&plan_artifacts.output_file)
            .with_context(|| {
                format!(
                    "planner output at {} did not match the plan schema",
                    plan_artifacts.output_file.display()
                )
            })?;
        self.artifacts.write_json(&layout.plan_file, &plan)?;

        state.run_title = truncate_title(&plan.goal, 50);

        state.features.clear();
        for (index, feature) in plan.features.iter().enumerate() {
            let feature_layout = layout.feature_layout(index, &feature.id)?;
            let contract = FeatureContract::from_feature(
                feature,
                &self.config.evaluator.commands,
                workspace_profile.as_ref(),
            );
            self.artifacts
                .write_json(&feature_layout.contract_file, &contract)?;

            state.features.push(FeatureRunState {
                index,
                feature_id: feature.id.clone(),
                title: feature.title.clone(),
                feature_root: feature_layout.root.clone(),
                contract_file: feature_layout.contract_file.clone(),
                builder_handoff_file: feature_layout.builder_handoff_file.clone(),
                qa_report_file: feature_layout.qa_report_file.clone(),
                status: FeatureLifecycleStatus::Pending,
                phase: FeaturePhase::PendingBuild,
                repair_attempts_used: 0,
                next_evaluate_attempt: 1,
                last_session_id: None,
                last_qa_status: None,
                stages: Vec::new(),
            });
        }

        state.lifecycle = RunLifecycleStatus::Running;
        info!(
            run_id = %state.run_id,
            feature_count = state.features.len(),
            "plan stage completed"
        );
        self.checkpoint(state)
    }

    async fn drive_run(
        &self,
        state: &mut RunState,
        layout: &RunLayout,
        runtime_plan: &RuntimePlan,
    ) -> Result<()> {
        let worker_context = self.worker_context(
            state.run_id,
            state.execution_workspace.clone(),
            layout.clone(),
            load_workspace_profile_snapshot(&self.artifacts, layout).as_ref(),
        );

        while state.current_feature_index < state.features.len() {
            let index = state.current_feature_index;
            let feature_layout = self.feature_layout_from_state(&state.features[index]);
            let contract = self
                .artifacts
                .read_json(&state.features[index].contract_file)
                .with_context(|| {
                    format!(
                        "failed to load feature contract from {}",
                        state.features[index].contract_file.display()
                    )
                })?;

            match state.features[index].phase {
                FeaturePhase::PendingBuild => {
                    let build_artifacts = feature_layout.stage_artifacts(WorkerStage::Build, 1);
                    self.begin_stage(
                        state,
                        WorkerStage::Build,
                        build_artifacts.attempt,
                        Some(index),
                        Some(state.features[index].feature_id.clone()),
                    )?;
                    info!(
                        run_id = %state.run_id,
                        feature_id = %state.features[index].feature_id,
                        attempt = build_artifacts.attempt,
                        feature_root = %feature_layout.root.display(),
                        "starting build stage"
                    );
                    let build_result = self
                        .worker
                        .build(
                            &worker_context,
                            &feature_layout,
                            &build_artifacts,
                            &contract,
                        )
                        .await?;
                    self.artifacts
                        .write_json(&build_artifacts.result_file, &build_result)?;
                    state.active_stage = None;
                    state.features[index].stages.push(RunStageRecord {
                        stage: build_result.stage,
                        attempt: build_artifacts.attempt,
                        status: build_result.status,
                        artifact: build_artifacts.result_file.clone(),
                        stdout_log: build_artifacts.stdout_log.clone(),
                        stderr_log: build_artifacts.stderr_log.clone(),
                        session_id: build_result.session_id.clone(),
                    });

                    let handoff: crate::domain::BuilderHandoff = self
                        .artifacts
                        .read_json(&build_artifacts.output_file)
                        .with_context(|| {
                            format!(
                                "builder output at {} did not match the builder handoff schema",
                                build_artifacts.output_file.display()
                            )
                        })?;
                    self.artifacts
                        .write_json(&state.features[index].builder_handoff_file, &handoff)?;

                    state.features[index].status = FeatureLifecycleStatus::Running;
                    state.features[index].phase = FeaturePhase::PendingEvaluate;
                    state.features[index].last_session_id = build_result.session_id;
                    self.checkpoint(state)?;
                }
                FeaturePhase::PendingEvaluate => {
                    let handoff = self
                        .artifacts
                        .read_json(&state.features[index].builder_handoff_file)
                        .with_context(|| {
                            format!(
                                "failed to load builder handoff from {}",
                                state.features[index].builder_handoff_file.display()
                            )
                        })?;

                    let attempt = state.features[index].next_evaluate_attempt;
                    let verification_evidence = self.capture_verification_evidence(
                        &feature_layout,
                        attempt,
                        &state.execution_workspace,
                    )?;
                    let screenshot_evidence = self.capture_screenshot_evidence(
                        &feature_layout,
                        attempt,
                        &state.execution_workspace,
                    )?;
                    let evaluation_request = build_evaluation_request(
                        &contract,
                        &handoff,
                        runtime_plan,
                        &self.config.evaluator,
                        verification_evidence,
                        screenshot_evidence,
                    );
                    let evaluate_artifacts =
                        feature_layout.stage_artifacts(WorkerStage::Evaluate, attempt);
                    self.begin_stage(
                        state,
                        WorkerStage::Evaluate,
                        evaluate_artifacts.attempt,
                        Some(index),
                        Some(state.features[index].feature_id.clone()),
                    )?;
                    info!(
                        run_id = %state.run_id,
                        feature_id = %state.features[index].feature_id,
                        attempt = evaluate_artifacts.attempt,
                        verification_count = evaluation_request.verification_commands.len(),
                        screenshot_required = evaluation_request.require_screenshots,
                        "starting evaluate stage"
                    );
                    let evaluate_result = self
                        .worker
                        .evaluate(
                            &worker_context,
                            &feature_layout,
                            &evaluate_artifacts,
                            &evaluation_request,
                        )
                        .await?;
                    self.artifacts
                        .write_json(&evaluate_artifacts.result_file, &evaluate_result)?;
                    state.active_stage = None;
                    state.features[index].stages.push(RunStageRecord {
                        stage: evaluate_result.stage,
                        attempt: evaluate_artifacts.attempt,
                        status: evaluate_result.status,
                        artifact: evaluate_artifacts.result_file.clone(),
                        stdout_log: evaluate_artifacts.stdout_log.clone(),
                        stderr_log: evaluate_artifacts.stderr_log.clone(),
                        session_id: evaluate_result.session_id.clone(),
                    });

                    let mut qa_report: crate::domain::QaReport = self
                        .artifacts
                        .read_json(&evaluate_artifacts.output_file)
                        .with_context(|| {
                            format!(
                                "evaluator output at {} did not match the QA schema",
                                evaluate_artifacts.output_file.display()
                            )
                        })?;
                    self.apply_verification_gate(
                        &mut qa_report,
                        &evaluation_request.verification_evidence,
                    );
                    self.apply_screenshot_gate(
                        &mut qa_report,
                        evaluation_request.screenshot_evidence.as_ref(),
                        evaluation_request.require_screenshots,
                    );
                    self.artifacts
                        .write_json(&state.features[index].qa_report_file, &qa_report)?;

                    state.features[index].last_qa_status = Some(qa_report.status);
                    if qa_report.status == QaStatus::Pass {
                        info!(
                            run_id = %state.run_id,
                            feature_id = %state.features[index].feature_id,
                            attempt,
                            "evaluate stage passed"
                        );
                        state.features[index].status = FeatureLifecycleStatus::Passed;
                        state.features[index].phase = FeaturePhase::Complete;
                        state.current_feature_index += 1;
                        state.final_status = Some(QaStatus::Pass);

                        if state.current_feature_index == state.features.len() {
                            state.lifecycle = RunLifecycleStatus::Passed;
                        }

                        self.checkpoint(state)?;
                    } else if state.features[index].repair_attempts_used
                        < self.config.runtime.max_repair_attempts
                    {
                        info!(
                            run_id = %state.run_id,
                            feature_id = %state.features[index].feature_id,
                            attempt,
                            qa_status = qa_report.status.as_str(),
                            "evaluate stage requested repair"
                        );
                        state.features[index].phase = FeaturePhase::PendingRepair;
                        state.features[index].next_evaluate_attempt += 1;
                        self.checkpoint(state)?;
                    } else {
                        info!(
                            run_id = %state.run_id,
                            feature_id = %state.features[index].feature_id,
                            attempt,
                            qa_status = qa_report.status.as_str(),
                            "evaluate stage exhausted repair attempts"
                        );
                        state.features[index].status = FeatureLifecycleStatus::Failed;
                        state.features[index].phase = FeaturePhase::Complete;
                        state.final_status = Some(qa_report.status);

                        if self.config.runtime.continue_after_failure {
                            state.current_feature_index += 1;
                            self.checkpoint(state)?;
                        } else {
                            state.lifecycle = RunLifecycleStatus::Failed;
                            self.checkpoint(state)?;
                            break;
                        }
                    }
                }
                FeaturePhase::PendingRepair => {
                    let handoff = self
                        .artifacts
                        .read_json(&state.features[index].builder_handoff_file)
                        .with_context(|| {
                            format!(
                                "failed to load builder handoff from {}",
                                state.features[index].builder_handoff_file.display()
                            )
                        })?;
                    let qa_report = self
                        .artifacts
                        .read_json(&state.features[index].qa_report_file)
                        .with_context(|| {
                            format!(
                                "failed to load qa report from {}",
                                state.features[index].qa_report_file.display()
                            )
                        })?;

                    let attempt = state.features[index].repair_attempts_used + 1;
                    let repair_artifacts =
                        feature_layout.stage_artifacts(WorkerStage::Repair, attempt);
                    self.begin_stage(
                        state,
                        WorkerStage::Repair,
                        repair_artifacts.attempt,
                        Some(index),
                        Some(state.features[index].feature_id.clone()),
                    )?;
                    info!(
                        run_id = %state.run_id,
                        feature_id = %state.features[index].feature_id,
                        attempt = repair_artifacts.attempt,
                        resume_session_id = state.features[index]
                            .last_session_id
                            .as_deref()
                            .unwrap_or("-"),
                        "starting repair stage"
                    );
                    let repair_result = self
                        .worker
                        .repair(
                            &worker_context,
                            &feature_layout,
                            &repair_artifacts,
                            &contract,
                            &handoff,
                            &qa_report,
                            state.features[index].last_session_id.as_deref(),
                        )
                        .await?;
                    self.artifacts
                        .write_json(&repair_artifacts.result_file, &repair_result)?;
                    state.active_stage = None;
                    state.features[index].stages.push(RunStageRecord {
                        stage: repair_result.stage,
                        attempt: repair_artifacts.attempt,
                        status: repair_result.status,
                        artifact: repair_artifacts.result_file.clone(),
                        stdout_log: repair_artifacts.stdout_log.clone(),
                        stderr_log: repair_artifacts.stderr_log.clone(),
                        session_id: repair_result.session_id.clone(),
                    });

                    let repaired_handoff: crate::domain::BuilderHandoff = self
                        .artifacts
                        .read_json(&repair_artifacts.output_file)
                        .with_context(|| {
                            format!(
                                "repair output at {} did not match the builder handoff schema",
                                repair_artifacts.output_file.display()
                            )
                        })?;
                    self.artifacts.write_json(
                        &state.features[index].builder_handoff_file,
                        &repaired_handoff,
                    )?;

                    state.features[index].repair_attempts_used = attempt;
                    state.features[index].phase = FeaturePhase::PendingEvaluate;
                    if repair_result.session_id.is_some() {
                        state.features[index].last_session_id = repair_result.session_id;
                    }
                    self.checkpoint(state)?;
                }
                FeaturePhase::Complete => {
                    if state.features[index].status == FeatureLifecycleStatus::Passed {
                        state.current_feature_index += 1;
                        self.checkpoint(state)?;
                    } else if self.config.runtime.continue_after_failure {
                        state.current_feature_index += 1;
                        self.checkpoint(state)?;
                    } else {
                        state.lifecycle = RunLifecycleStatus::Failed;
                        self.checkpoint(state)?;
                        break;
                    }
                }
            }
        }

        if state.current_feature_index == state.features.len()
            && state.lifecycle != RunLifecycleStatus::Failed
        {
            let has_failed_feature = state
                .features
                .iter()
                .any(|f| f.status == FeatureLifecycleStatus::Failed);

            if has_failed_feature {
                state.lifecycle = RunLifecycleStatus::Failed;
                if state.final_status.is_none() {
                    state.final_status = Some(QaStatus::Fail);
                }
            } else {
                state.lifecycle = RunLifecycleStatus::Passed;
                state.final_status = Some(QaStatus::Pass);
            }
            self.checkpoint(state)?;
        }

        Ok(())
    }

    async fn drive_run_with_runtime(
        &self,
        state: &mut RunState,
        layout: &RunLayout,
        runtime_plan: &RuntimePlan,
    ) -> Result<()> {
        let mut runtime_supervisor =
            RuntimeSupervisor::start_if_enabled(&self.config.runtime, runtime_plan, &layout.root)
                .await?;

        let run_result = self.drive_run(state, layout, runtime_plan).await;
        let shutdown_result = runtime_supervisor.shutdown().await;

        match (run_result, shutdown_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(run_err), Ok(())) => Err(run_err),
            (Ok(()), Err(shutdown_err)) => Err(shutdown_err),
            (Err(run_err), Err(shutdown_err)) => {
                Err(run_err.context(format!("runtime shutdown also failed: {shutdown_err:#}")))
            }
        }
    }

    fn checkpoint(&self, state: &mut RunState) -> Result<()> {
        state.updated_at = Utc::now();
        self.artifacts.write_json(&state.state_file, state)?;
        self.artifacts.write_json(&state.manifest_file, state)
    }

    fn begin_stage(
        &self,
        state: &mut RunState,
        stage: WorkerStage,
        attempt: usize,
        feature_index: Option<usize>,
        feature_id: Option<String>,
    ) -> Result<()> {
        state.active_stage = Some(ActiveRunStage {
            stage,
            attempt,
            feature_index,
            feature_id,
            started_at: Utc::now(),
        });
        self.checkpoint(state)
    }

    fn worker_context(
        &self,
        run_id: Uuid,
        workspace: std::path::PathBuf,
        layout: RunLayout,
        workspace_profile: Option<&WorkspaceProfile>,
    ) -> WorkerContext {
        let workspace_profile_artifact =
            workspace_profile.map(|_| layout.workspace_profile_file.clone());
        let planner_prompt = layout.planner_prompt_file.clone();
        let builder_prompt = layout.builder_prompt_file.clone();
        let evaluator_prompt = layout.evaluator_prompt_file.clone();
        WorkerContext {
            run_id,
            workspace,
            layout,
            planner_prompt,
            builder_prompt,
            evaluator_prompt,
            planner_schema: self.config.schemas.planner_output.clone(),
            builder_schema: self.config.schemas.builder_handoff.clone(),
            qa_schema: self.config.schemas.qa_report.clone(),
            workspace_profile_artifact,
            workspace_profile_context: workspace_profile.map(WorkspaceProfile::prompt_context),
        }
    }

    fn layout_from_run_root(&self, run_root: &Path) -> RunLayout {
        let inputs_dir = run_root.join("inputs");
        let prompt_inputs_dir = inputs_dir.join("prompts");
        RunLayout {
            root: run_root.to_path_buf(),
            inputs_dir: inputs_dir.clone(),
            prompt_inputs_dir: prompt_inputs_dir.clone(),
            workspace_profile_file: inputs_dir.join("workspace-profile.json"),
            planner_prompt_file: prompt_inputs_dir.join("planner.md"),
            builder_prompt_file: prompt_inputs_dir.join("builder.md"),
            evaluator_prompt_file: prompt_inputs_dir.join("evaluator.md"),
            launch_file: run_root.join("launch.json"),
            request_file: run_root.join("request.md"),
            plan_file: run_root.join("plan.json"),
            runtime_plan_file: run_root.join("runtime-plan.json"),
            state_file: run_root.join("run-state.json"),
            manifest_file: run_root.join("manifest.json"),
            features_dir: run_root.join("features"),
            worker_dir: run_root.join("worker"),
        }
    }

    fn build_launch_snapshot(
        &self,
        request: &RunRequest,
        workspace_profile_snapshot_path: &Path,
        launched_at: chrono::DateTime<Utc>,
        effective_feature_limit: usize,
        feature_limit_is_hard: bool,
    ) -> Result<RunLaunchSnapshot> {
        let config_contents = request
            .selected_config
            .as_ref()
            .map(|path| {
                fs::read_to_string(path)
                    .with_context(|| format!("failed to read config file {}", path.display()))
            })
            .transpose()?;
        let prompts = PromptSnapshot {
            planner: request.prompt_overrides.planner.clone().unwrap_or(
                fs::read_to_string(&self.config.prompts.planner).with_context(|| {
                    format!(
                        "failed to read planner prompt {}",
                        self.config.prompts.planner.display()
                    )
                })?,
            ),
            builder: request.prompt_overrides.builder.clone().unwrap_or(
                fs::read_to_string(&self.config.prompts.builder).with_context(|| {
                    format!(
                        "failed to read builder prompt {}",
                        self.config.prompts.builder.display()
                    )
                })?,
            ),
            evaluator: request.prompt_overrides.evaluator.clone().unwrap_or(
                fs::read_to_string(&self.config.prompts.evaluator).with_context(|| {
                    format!(
                        "failed to read evaluator prompt {}",
                        self.config.prompts.evaluator.display()
                    )
                })?,
            ),
        };

        Ok(RunLaunchSnapshot {
            source_workspace: request.source_workspace.clone(),
            selected_config: request.selected_config.clone(),
            config_contents,
            requested_feature_limit: request.feature_limit,
            effective_feature_limit,
            feature_limit_is_hard,
            confirm_before_build: request.confirm_before_build,
            user_request: request.user_request.clone(),
            prompts,
            workspace_profile: request.workspace_profile.as_ref().map(|selection| {
                RunWorkspaceProfileSnapshot {
                    snapshot_path: workspace_profile_snapshot_path.to_path_buf(),
                    canonical_profile_path: selection.canonical_profile_path.clone(),
                    workspace_fingerprint: selection.workspace_fingerprint.clone(),
                    profile_fingerprint: selection.profile_fingerprint.clone(),
                    last_scanned_at: selection.last_scanned_at,
                    last_refreshed_at: selection.last_refreshed_at,
                    refresh_error: selection.refresh_error.clone(),
                    used_fallback_profile: selection.used_fallback_profile,
                }
            }),
            launched_at,
        })
    }

    fn feature_layout_from_state(&self, feature: &FeatureRunState) -> FeatureLayout {
        FeatureLayout {
            root: feature.feature_root.clone(),
            contract_file: feature.contract_file.clone(),
            builder_handoff_file: feature.builder_handoff_file.clone(),
            qa_report_file: feature.qa_report_file.clone(),
            runtime_dir: feature.feature_root.join("runtime"),
            worker_dir: feature.feature_root.join("worker"),
        }
    }

    fn capture_verification_evidence(
        &self,
        feature_layout: &FeatureLayout,
        attempt: usize,
        workspace: &Path,
    ) -> Result<crate::domain::VerificationEvidence> {
        let artifacts = feature_layout.verification_artifacts(attempt)?;
        let evidence = run_verification_commands(
            workspace,
            &artifacts.root,
            attempt,
            &self.config.evaluator.commands,
        )?;
        self.artifacts
            .write_json(&artifacts.report_file, &evidence)
            .with_context(|| {
                format!(
                    "failed to write verification report {}",
                    artifacts.report_file.display()
                )
            })?;
        Ok(evidence)
    }

    fn capture_screenshot_evidence(
        &self,
        feature_layout: &FeatureLayout,
        attempt: usize,
        workspace: &Path,
    ) -> Result<Option<crate::domain::ScreenshotEvidence>> {
        if self.config.evaluator.screenshots.is_empty() {
            return Ok(None);
        }

        let artifacts = feature_layout.screenshot_artifacts(attempt)?;
        let evidence = run_screenshot_commands(
            workspace,
            &artifacts.root,
            attempt,
            &self.config.evaluator.screenshots,
        )?;
        self.artifacts
            .write_json(&artifacts.report_file, &evidence)
            .with_context(|| {
                format!(
                    "failed to write screenshot report {}",
                    artifacts.report_file.display()
                )
            })?;
        Ok(Some(evidence))
    }

    fn apply_verification_gate(
        &self,
        qa_report: &mut crate::domain::QaReport,
        verification_evidence: &crate::domain::VerificationEvidence,
    ) {
        if verification_evidence.all_passed() {
            return;
        }

        qa_report.status = QaStatus::Fail;
        qa_report.findings.extend(
            verification_evidence
                .results
                .iter()
                .filter(|result| {
                    !matches!(result.status, crate::domain::VerificationStatus::Passed)
                })
                .map(|result| {
                    format!(
                        "{} failed with exit code {:?}. stdout={} stderr={}",
                        result.name,
                        result.exit_code,
                        result.stdout_log.display(),
                        result.stderr_log.display()
                    )
                }),
        );
        qa_report.next_actions.push(
            "Repair the failing verification commands before accepting this feature.".to_string(),
        );
        qa_report.summary = format!(
            "{} Deterministic verification failed for evaluate attempt {}.",
            qa_report.summary, verification_evidence.attempt
        );
    }

    fn apply_screenshot_gate(
        &self,
        qa_report: &mut crate::domain::QaReport,
        screenshot_evidence: Option<&crate::domain::ScreenshotEvidence>,
        require_screenshots: bool,
    ) {
        if !require_screenshots {
            return;
        }

        let Some(screenshot_evidence) = screenshot_evidence else {
            qa_report.status = QaStatus::Fail;
            qa_report.findings.push(
                "Required screenshot capture was enabled, but no screenshot evidence was produced."
                    .to_string(),
            );
            qa_report
                .next_actions
                .push("Configure required screenshot capture and rerun evaluation.".to_string());
            qa_report.summary = format!(
                "{} Required screenshot capture failed for evaluate attempt {}.",
                qa_report.summary, 0
            );
            return;
        };

        if screenshot_evidence.all_captured() {
            return;
        }

        qa_report.status = QaStatus::Fail;
        qa_report.findings.extend(
            screenshot_evidence
                .results
                .iter()
                .filter(|result| {
                    !matches!(result.status, crate::domain::ScreenshotStatus::Captured)
                })
                .map(|result| {
                    format!(
                        "{} screenshot capture failed with exit code {:?}. output={} stdout={} stderr={}",
                        result.name,
                        result.exit_code,
                        result.output_file.display(),
                        result.stdout_log.display(),
                        result.stderr_log.display()
                    )
                }),
        );
        qa_report.next_actions.push(
            "Repair the failing screenshot capture commands before accepting this feature."
                .to_string(),
        );
        qa_report.summary = format!(
            "{} Required screenshot capture failed for evaluate attempt {}.",
            qa_report.summary, screenshot_evidence.attempt
        );
    }
}

fn load_workspace_profile_snapshot(
    artifacts: &FileArtifactStore,
    layout: &RunLayout,
) -> Option<WorkspaceProfile> {
    if !layout.workspace_profile_file.exists() {
        return None;
    }

    artifacts.read_json(&layout.workspace_profile_file).ok()
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
    };

    use anyhow::{Context, Result};
    use async_trait::async_trait;
    use chrono::Utc;
    use tempfile::tempdir;
    use uuid::Uuid;

    use crate::{
        artifacts::{FeatureLayout, FileArtifactStore, StageArtifactSet},
        config::{
            EvaluatorConfig, ResolvedConfig, ResolvedPromptConfig, ResolvedSchemaConfig,
            ResolvedStorageConfig, RuntimeConfig, RuntimeSupervisionConfig, ServiceConfig,
            SimulationWorkerConfig, WorkerConfig, WorkerSelection, WorkspaceConfig,
        },
        discovery::{
            CommandCatalog, DiscoveryArtifactSet, DiscoveryFact, LayeringProfile,
            WorkspaceDiscoveryRequest, WorkspaceProfile, WorkspaceProfileSelection,
        },
        domain::{
            ActiveRunStage, BuilderHandoff, EvaluationRequest, Feature, FeatureContract,
            FeatureLifecycleStatus, PlanDocument, PlannerConversationRequest, PromptOverrides,
            QaCheck, QaReport, QaStatus, RunLaunchSnapshot, RunLifecycleStatus, RunRequest,
            RunStageRecord, RunState, WorkerResult, WorkerStage, WorkerStatus,
        },
        worker::{
            DiscoveryContext, DiscoveryWorkerResult, PlannerConversationArtifactSet,
            PlannerConversationContext, PlannerConversationWorkerResult, WorkerAdapter,
            WorkerContext,
        },
        workspace::WorkspaceIsolation,
    };

    use super::HarnessController;

    #[derive(Default)]
    struct FakeState {
        evaluate_calls: usize,
        observed_build_active_stage: Option<ActiveRunStage>,
    }

    struct FakeWorker {
        state: Arc<Mutex<FakeState>>,
    }

    #[async_trait]
    impl WorkerAdapter for FakeWorker {
        async fn discover(
            &self,
            _context: &DiscoveryContext,
            artifacts: &DiscoveryArtifactSet,
            request: &WorkspaceDiscoveryRequest,
        ) -> Result<DiscoveryWorkerResult> {
            let profile = request.synthesize_profile();
            fs::write(&artifacts.output_file, serde_json::to_vec_pretty(&profile)?)?;
            Ok(DiscoveryWorkerResult {
                status: WorkerStatus::Prepared,
                command: vec!["fake".to_string(), "discover".to_string()],
                prompt_file: artifacts.prompt_file.clone(),
                output_file: artifacts.output_file.clone(),
                stdout_log: artifacts.stdout_log.clone(),
                stderr_log: artifacts.stderr_log.clone(),
                notes: Vec::new(),
                session_id: None,
            })
        }

        async fn plan(
            &self,
            _context: &WorkerContext,
            artifacts: &StageArtifactSet,
            _request: &crate::domain::PlanningRequest,
        ) -> Result<WorkerResult> {
            let plan = PlanDocument {
                goal: "goal".to_string(),
                features: vec![
                    Feature {
                        id: "feature-001".to_string(),
                        title: "one".to_string(),
                        summary: "first".to_string(),
                        acceptance_criteria: vec!["a".to_string()],
                    },
                    Feature {
                        id: "feature-002".to_string(),
                        title: "two".to_string(),
                        summary: "second".to_string(),
                        acceptance_criteria: vec!["b".to_string()],
                    },
                ],
                risks: Vec::new(),
                checkpoints: Vec::new(),
            };
            fs::write(&artifacts.output_file, serde_json::to_vec_pretty(&plan)?)?;

            Ok(worker_result(artifacts, WorkerStage::Plan, None))
        }

        async fn consult_planner(
            &self,
            _context: &PlannerConversationContext,
            artifacts: &PlannerConversationArtifactSet,
            request: &PlannerConversationRequest,
        ) -> Result<PlannerConversationWorkerResult> {
            let response = request.synthesize_response();
            fs::write(
                &artifacts.output_file,
                serde_json::to_vec_pretty(&response)?,
            )?;
            Ok(PlannerConversationWorkerResult {
                status: WorkerStatus::Prepared,
                command: vec!["fake".to_string(), "planner-consult".to_string()],
                prompt_file: artifacts.prompt_file.clone(),
                output_file: artifacts.output_file.clone(),
                stdout_log: artifacts.stdout_log.clone(),
                stderr_log: artifacts.stderr_log.clone(),
                notes: Vec::new(),
                session_id: None,
            })
        }

        async fn build(
            &self,
            context: &WorkerContext,
            _feature: &FeatureLayout,
            artifacts: &StageArtifactSet,
            _contract: &FeatureContract,
        ) -> Result<WorkerResult> {
            let state: RunState = serde_json::from_slice(&fs::read(&context.layout.state_file)?)?;
            let mut fake_state = self.state.lock().expect("lock");
            if fake_state.observed_build_active_stage.is_none() {
                fake_state.observed_build_active_stage = state.active_stage;
            }
            let handoff = BuilderHandoff {
                summary: "build".to_string(),
                changed_files: Vec::new(),
                verification: Vec::new(),
                open_questions: Vec::new(),
            };
            fs::write(&artifacts.output_file, serde_json::to_vec_pretty(&handoff)?)?;
            Ok(worker_result(
                artifacts,
                WorkerStage::Build,
                Some("thread-build".to_string()),
            ))
        }

        async fn evaluate(
            &self,
            _context: &WorkerContext,
            _feature: &FeatureLayout,
            artifacts: &StageArtifactSet,
            _request: &EvaluationRequest,
        ) -> Result<WorkerResult> {
            let mut state = self.state.lock().expect("lock");
            state.evaluate_calls += 1;
            let status = if state.evaluate_calls == 1 {
                QaStatus::Fail
            } else {
                QaStatus::Pass
            };
            let report = QaReport {
                status,
                summary: "qa".to_string(),
                findings: Vec::new(),
                next_actions: Vec::new(),
                checks: vec![QaCheck {
                    name: "check".to_string(),
                    command: vec!["cargo".to_string(), "test".to_string()],
                    rationale: "why".to_string(),
                }],
            };
            fs::write(&artifacts.output_file, serde_json::to_vec_pretty(&report)?)?;
            Ok(worker_result(artifacts, WorkerStage::Evaluate, None))
        }

        async fn repair(
            &self,
            _context: &WorkerContext,
            _feature: &FeatureLayout,
            artifacts: &StageArtifactSet,
            _contract: &FeatureContract,
            _builder_handoff: &BuilderHandoff,
            _qa_report: &QaReport,
            previous_session_id: Option<&str>,
        ) -> Result<WorkerResult> {
            let handoff = BuilderHandoff {
                summary: format!("repair {previous_session_id:?}"),
                changed_files: Vec::new(),
                verification: Vec::new(),
                open_questions: Vec::new(),
            };
            fs::write(&artifacts.output_file, serde_json::to_vec_pretty(&handoff)?)?;
            Ok(worker_result(
                artifacts,
                WorkerStage::Repair,
                Some("thread-repair".to_string()),
            ))
        }
    }

    #[tokio::test]
    async fn controller_runs_multiple_features_and_records_state() -> Result<()> {
        let temp = tempdir()?;
        let source_workspace = temp.path().join("workspace");
        fs::create_dir_all(&source_workspace)?;

        let config = resolved_config(temp.path(), temp.path().join(".loopsmith-runs"));
        let artifacts = FileArtifactStore::new(config.storage.runs_dir.clone());
        let shared_state = Arc::new(Mutex::new(FakeState::default()));
        let worker = FakeWorker {
            state: shared_state.clone(),
        };
        let controller = HarnessController::new(config, artifacts, worker);

        let state = controller
            .start_run(RunRequest {
                user_request: "Build a harness".to_string(),
                source_workspace,
                feature_limit: Some(2),
                confirm_before_build: false,
                selected_config: None,
                prompt_overrides: Default::default(),
                workspace_profile: None,
            })
            .await?;
        let observed = shared_state
            .lock()
            .expect("lock")
            .observed_build_active_stage
            .clone()
            .expect("observed active stage");

        assert_eq!(state.lifecycle, RunLifecycleStatus::Passed);
        assert_eq!(state.features.len(), 2);
        assert_eq!(observed.stage, WorkerStage::Build);
        assert_eq!(observed.attempt, 1);
        assert_eq!(observed.feature_index, Some(0));
        assert_eq!(observed.feature_id.as_deref(), Some("feature-001"));
        assert!(
            state
                .features
                .iter()
                .all(|feature| feature.status == FeatureLifecycleStatus::Passed)
        );
        assert_eq!(
            fs::read_to_string(state.run_root.join("inputs/prompts/builder.md"))?,
            "builder override\n"
        );
        assert_eq!(
            fs::read_to_string(state.run_root.join("inputs/prompts/evaluator.md"))?,
            "evaluator prompt\n"
        );

        Ok(())
    }

    #[tokio::test]
    async fn controller_records_config_feature_limit_as_advisory() -> Result<()> {
        let temp = tempdir()?;
        let source_workspace = temp.path().join("workspace");
        fs::create_dir_all(&source_workspace)?;

        let config = resolved_config(temp.path(), temp.path().join("runs"));
        let artifacts = FileArtifactStore::new(config.storage.runs_dir.clone());
        let worker = FakeWorker {
            state: Arc::new(Mutex::new(FakeState::default())),
        };
        let controller = HarnessController::new(config, artifacts, worker);

        let state = controller
            .start_run(RunRequest {
                user_request: "Build a harness".to_string(),
                source_workspace,
                feature_limit: None,
                selected_config: None,
                prompt_overrides: Default::default(),
            })
            .await?;

        let launch_file = state.launch_file.clone().expect("launch file");
        let launch: RunLaunchSnapshot =
            serde_json::from_slice(&fs::read(&launch_file).context("read launch")?)?;
        assert_eq!(launch.requested_feature_limit, None);
        assert_eq!(launch.effective_feature_limit, 2);
        assert!(!launch.feature_limit_is_hard);

        Ok(())
    }

    #[tokio::test]
    async fn controller_snapshots_launch_inputs_and_prompt_overrides() -> Result<()> {
        let temp = tempdir()?;
        let source_workspace = temp.path().join("workspace");
        fs::create_dir_all(&source_workspace)?;
        let config_file = temp.path().join("config.toml");
        fs::write(&config_file, "feature_limit = 2\n")?;

        let config = resolved_config(temp.path(), temp.path().join(".loopsmith-runs"));
        let artifacts = FileArtifactStore::new(config.storage.runs_dir.clone());
        let worker = FakeWorker {
            state: Arc::new(Mutex::new(FakeState::default())),
        };
        let controller = HarnessController::new(config, artifacts, worker);

        let state = controller
            .start_run(RunRequest {
                user_request: "Build a harness".to_string(),
                source_workspace,
                feature_limit: Some(2),
                confirm_before_build: false,
                selected_config: Some(config_file.clone()),
                prompt_overrides: PromptOverrides {
                    planner: Some("planner override\n".to_string()),
                    builder: Some("builder override\n".to_string()),
                    evaluator: None,
                },
                workspace_profile: None,
            })
            .await?;

        let launch_file = state.launch_file.clone().expect("launch file");
        let launch: RunLaunchSnapshot =
            serde_json::from_slice(&fs::read(&launch_file).context("read launch")?)?;
        assert_eq!(launch.selected_config, Some(config_file));
        assert_eq!(launch.requested_feature_limit, Some(2));
        assert_eq!(launch.effective_feature_limit, 2);
        assert!(launch.feature_limit_is_hard);
        assert_eq!(launch.user_request, "Build a harness");
        assert_eq!(launch.prompts.planner, "planner override\n");
        assert_eq!(launch.prompts.builder, "builder override\n");
        assert_eq!(launch.prompts.evaluator, "evaluator prompt\n");
        assert_eq!(
            fs::read_to_string(state.run_root.join("inputs/prompts/planner.md"))?,
            "planner override\n"
        );
        assert_eq!(
            fs::read_to_string(state.run_root.join("inputs/prompts/builder.md"))?,
            "builder override\n"
        );
        assert_eq!(
            fs::read_to_string(state.run_root.join("inputs/prompts/evaluator.md"))?,
            "evaluator prompt\n"
        );

        Ok(())
    }

    #[tokio::test]
    async fn controller_records_config_feature_limit_as_advisory() -> Result<()> {
        let temp = tempdir()?;
        let source_workspace = temp.path().join("workspace");
        fs::create_dir_all(&source_workspace)?;

        let config = resolved_config(temp.path(), temp.path().join(".loopsmith-runs"));
        let artifacts = FileArtifactStore::new(config.storage.runs_dir.clone());
        let worker = FakeWorker {
            state: Arc::new(Mutex::new(FakeState::default())),
        };
        let controller = HarnessController::new(config, artifacts, worker);

        let state = controller
            .start_run(RunRequest {
                user_request: "Build a harness".to_string(),
                source_workspace,
                feature_limit: None,
                confirm_before_build: false,
                selected_config: None,
                prompt_overrides: Default::default(),
                workspace_profile: None,
            })
            .await?;

        let launch_file = state.launch_file.clone().expect("launch file");
        let launch: RunLaunchSnapshot =
            serde_json::from_slice(&fs::read(&launch_file).context("read launch")?)?;
        assert_eq!(launch.requested_feature_limit, None);
        assert_eq!(launch.effective_feature_limit, 2);
        assert!(!launch.feature_limit_is_hard);

        Ok(())
    }

    #[tokio::test]
    async fn controller_snapshots_workspace_profile_and_applies_contract_notes() -> Result<()> {
        let temp = tempdir()?;
        let source_workspace = temp.path().join("workspace");
        fs::create_dir_all(&source_workspace)?;

        let config = resolved_config(temp.path(), temp.path().join(".loopsmith-runs"));
        let artifacts = FileArtifactStore::new(config.storage.runs_dir.clone());
        let worker = FakeWorker {
            state: Arc::new(Mutex::new(FakeState::default())),
        };
        let controller = HarnessController::new(config, artifacts, worker);
        let profile = WorkspaceProfile {
            workspace_path: source_workspace.clone(),
            generated_at: Utc::now(),
            summary: "Workspace profile summary".to_string(),
            key_concepts: vec!["Core loop with strict layering".to_string()],
            tech_stack: Vec::new(),
            repositories: Vec::new(),
            dependency_relationships: Vec::new(),
            api_contracts: vec![DiscoveryFact {
                id: "api_contract.001".to_string(),
                title: "HTTP route evidence in src/api.rs".to_string(),
                summary: "Route signatures: router.get(\"/health\").".to_string(),
                evidence: vec![PathBuf::from("src/api.rs")],
                tier: crate::discovery::NegentropyTier::Implementation,
            }],
            layering: LayeringProfile {
                summary: "Detected layers: ui, service, core.".to_string(),
                layers: Vec::new(),
                allowed_dependency_directions: vec![
                    "UI and interface layers may depend inward on service and core layers, not the reverse."
                        .to_string(),
                ],
                unresolved_ambiguities: Vec::new(),
            },
            user_journeys: Vec::new(),
            e2e_test_cases: Vec::new(),
            auth: Vec::new(),
            coding_conventions: vec![DiscoveryFact {
                id: "coding_convention.001".to_string(),
                title: ".editorconfig".to_string(),
                summary: "root = true".to_string(),
                evidence: vec![PathBuf::from(".editorconfig")],
                tier: crate::discovery::NegentropyTier::Specification,
            }],
            commands: CommandCatalog::default(),
            risks: Vec::new(),
            project_intent: Vec::new(),
            environment_requirements: Vec::new(),
            change_boundaries: crate::discovery::ChangeBoundaryProfile::default(),
        };
        let profile_fingerprint = crate::discovery::profile_fingerprint(&profile)?;

        let state = controller
            .start_run(RunRequest {
                user_request: "Build a harness".to_string(),
                source_workspace,
                feature_limit: Some(1),
                confirm_before_build: false,
                selected_config: None,
                prompt_overrides: Default::default(),
                workspace_profile: Some(WorkspaceProfileSelection {
                    profile: profile.clone(),
                    canonical_profile_path: temp
                        .path()
                        .join("workspace/.loopsmith/discovery/profile.json"),
                    scan_path: temp.path().join("workspace/.loopsmith/discovery/scan.json"),
                    evidence_path: temp
                        .path()
                        .join("workspace/.loopsmith/discovery/evidence.json"),
                    inference_path: temp
                        .path()
                        .join("workspace/.loopsmith/discovery/inference.json"),
                    status_path: temp
                        .path()
                        .join("workspace/.loopsmith/discovery/status.json"),
                    workspace_fingerprint: "workspace-fingerprint".to_string(),
                    profile_fingerprint: profile_fingerprint.clone(),
                    last_scanned_at: profile.generated_at,
                    last_refreshed_at: profile.generated_at,
                    refresh_error: None,
                    used_fallback_profile: false,
                }),
            })
            .await?;

        let launch_file = state.launch_file.clone().expect("launch file");
        let launch: RunLaunchSnapshot =
            serde_json::from_slice(&fs::read(&launch_file).context("read launch")?)?;
        let snapshot_path = state.run_root.join("inputs/workspace-profile.json");
        let snapped_profile: WorkspaceProfile = serde_json::from_slice(&fs::read(&snapshot_path)?)?;
        let contract: FeatureContract = serde_json::from_slice(&fs::read(
            state
                .run_root
                .join("features/01-feature-001/feature-contract.json"),
        )?)?;

        assert_eq!(snapped_profile.summary, profile.summary);
        assert_eq!(
            launch
                .workspace_profile
                .as_ref()
                .expect("workspace profile snapshot")
                .snapshot_path,
            snapshot_path
        );
        assert_eq!(
            launch
                .workspace_profile
                .as_ref()
                .expect("workspace profile snapshot")
                .profile_fingerprint,
            profile_fingerprint
        );
        assert!(
            contract
                .scope_notes
                .iter()
                .any(|note| note.contains("Respect workspace layering"))
        );
        assert!(
            contract
                .scope_notes
                .iter()
                .any(|note| note.contains("Preserve detected API contracts"))
        );
        assert!(
            contract
                .scope_notes
                .iter()
                .any(|note| note.contains("Follow detected coding conventions"))
        );

        Ok(())
    }

    #[tokio::test]
    async fn controller_waits_for_feature_confirmation_before_build() -> Result<()> {
        let temp = tempdir()?;
        let source_workspace = temp.path().join("workspace");
        fs::create_dir_all(&source_workspace)?;

        let config = resolved_config(temp.path(), temp.path().join(".loopsmith-runs"));
        let artifacts = FileArtifactStore::new(config.storage.runs_dir.clone());
        let shared_state = Arc::new(Mutex::new(FakeState::default()));
        let worker = FakeWorker {
            state: shared_state.clone(),
        };
        let controller = HarnessController::new(config, artifacts, worker);

        let planned = controller
            .start_run(RunRequest {
                user_request: "Build a harness".to_string(),
                source_workspace,
                feature_limit: Some(1),
                confirm_before_build: true,
                selected_config: None,
                prompt_overrides: Default::default(),
                workspace_profile: None,
            })
            .await?;

        assert_eq!(planned.lifecycle, RunLifecycleStatus::Running);
        assert!(planned.awaiting_feature_confirmation);
        assert!(
            shared_state
                .lock()
                .expect("lock")
                .observed_build_active_stage
                .is_none()
        );

        let resumed = controller.resume_run(&planned.run_root).await?;

        assert!(!resumed.awaiting_feature_confirmation);
        assert_eq!(resumed.lifecycle, RunLifecycleStatus::Passed);
        assert!(
            shared_state
                .lock()
                .expect("lock")
                .observed_build_active_stage
                .is_some()
        );

        Ok(())
    }

    #[test]
    fn inspect_reads_persisted_state() -> Result<()> {
        let temp = tempdir()?;
        let run_root = temp.path().join("run");
        fs::create_dir_all(&run_root)?;
        let config = resolved_config(temp.path(), temp.path().join(".loopsmith-runs"));
        let artifacts = FileArtifactStore::new(config.storage.runs_dir.clone());
        let controller = HarnessController::new(
            config,
            artifacts,
            FakeWorker {
                state: Arc::new(Mutex::new(FakeState::default())),
            },
        );

        let state = RunState {
            run_id: Uuid::nil(),
            run_title: String::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            run_root: run_root.clone(),
            state_file: run_root.join("run-state.json"),
            manifest_file: run_root.join("manifest.json"),
            launch_file: Some(run_root.join("launch.json")),
            request_file: run_root.join("request.md"),
            plan_file: run_root.join("plan.json"),
            runtime_plan_file: run_root.join("runtime-plan.json"),
            source_workspace: run_root.join("src"),
            execution_workspace: run_root.join("src"),
            lifecycle: RunLifecycleStatus::Running,
            final_status: None,
            current_feature_index: 0,
            awaiting_feature_confirmation: false,
            active_stage: None,
            plan_stage: Some(RunStageRecord {
                stage: WorkerStage::Plan,
                attempt: 1,
                status: WorkerStatus::Prepared,
                artifact: run_root.join("worker/plan-01-result.json"),
                stdout_log: run_root.join("worker/logs/plan-01-stdout.log"),
                stderr_log: run_root.join("worker/logs/plan-01-stderr.log"),
                session_id: None,
            }),
            features: Vec::new(),
        };
        fs::write(&state.state_file, serde_json::to_vec_pretty(&state)?)?;

        let loaded = controller.inspect_run(&run_root)?;
        assert_eq!(loaded.run_id, Uuid::nil());
        assert_eq!(loaded.lifecycle, RunLifecycleStatus::Running);

        Ok(())
    }

    fn worker_result(
        artifacts: &StageArtifactSet,
        stage: WorkerStage,
        session_id: Option<String>,
    ) -> WorkerResult {
        WorkerResult {
            stage,
            status: WorkerStatus::Prepared,
            command: vec!["fake".to_string()],
            prompt_file: artifacts.prompt_file.clone(),
            output_file: artifacts.output_file.clone(),
            stdout_log: artifacts.stdout_log.clone(),
            stderr_log: artifacts.stderr_log.clone(),
            notes: Vec::new(),
            session_id,
        }
    }

    fn resolved_config(project_root: &Path, runs_dir: PathBuf) -> ResolvedConfig {
        let prompts = project_root.join("prompts");
        let schemas = project_root.join("schemas");
        let _ = fs::create_dir_all(&prompts);
        let _ = fs::create_dir_all(&schemas);
        let _ = fs::write(prompts.join("discovery.md"), "discovery prompt\n");
        let _ = fs::write(prompts.join("planner.md"), "planner prompt\n");
        let _ = fs::write(prompts.join("builder.md"), "builder prompt\n");
        let _ = fs::write(prompts.join("evaluator.md"), "evaluator prompt\n");
        let _ = fs::write(schemas.join("workspace-profile.json"), "{}\n");
        let _ = fs::write(schemas.join("planner-output.json"), "{}\n");
        let _ = fs::write(schemas.join("builder-handoff.json"), "{}\n");
        let _ = fs::write(schemas.join("qa-report.json"), "{}\n");

        ResolvedConfig {
            project_root: project_root.to_path_buf(),
            storage: ResolvedStorageConfig { runs_dir },
            workspace: WorkspaceConfig {
                isolation: WorkspaceIsolation::Direct,
            },
            worker: WorkerConfig {
                selection: WorkerSelection::Simulated {
                    simulation: SimulationWorkerConfig {
                        evaluator_statuses: vec![QaStatus::Pass],
                        session_prefix: "sim".to_string(),
                    },
                },
                planner: None,
            },
            prompts: ResolvedPromptConfig {
                discovery: prompts.join("discovery.md"),
                planner: prompts.join("planner.md"),
                builder: prompts.join("builder.md"),
                evaluator: prompts.join("evaluator.md"),
            },
            schemas: ResolvedSchemaConfig {
                workspace_profile: schemas.join("workspace-profile.json"),
                workspace_inference: schemas.join("workspace-inference.json"),
                planner_output: schemas.join("planner-output.json"),
                builder_handoff: schemas.join("builder-handoff.json"),
                qa_report: schemas.join("qa-report.json"),
            },
            runtime: RuntimeConfig {
                feature_limit: 2,
                max_repair_attempts: 1,
                continue_after_failure: false,
                confirm_before_build: false,
                supervision: RuntimeSupervisionConfig::default(),
                services: vec![ServiceConfig {
                    name: "web".to_string(),
                    start: vec!["pnpm".to_string(), "dev".to_string()],
                    working_dir: None,
                    ready_url: None,
                    ready_command: None,
                }],
                stacks: Vec::new(),
            },
            evaluator: EvaluatorConfig {
                dimensions: vec!["correctness".to_string()],
                require_screenshots: false,
                commands: vec![vec!["/usr/bin/env".to_string(), "true".to_string()]],
                screenshots: Vec::new(),
            },
        }
    }
}

fn truncate_title(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    let first_line = trimmed.lines().next().unwrap_or(trimmed);
    if first_line.chars().count() <= max_chars {
        first_line.to_string()
    } else {
        let truncated: String = first_line.chars().take(max_chars).collect();
        format!("{truncated}…")
    }
}
