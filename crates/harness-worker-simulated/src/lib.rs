use std::fs;

use anyhow::{Context, Result};
use async_trait::async_trait;
use loopsmith_core::{
    artifacts::{FeatureLayout, StageArtifactSet},
    config::SimulationWorkerConfig,
    domain::{
        BuilderHandoff, EvaluationRequest, PlanningRequest, QaCheck, QaReport, QaStatus,
        WorkerResult, WorkerStatus,
    },
    worker::{WorkerAdapter, WorkerContext, render_worker_prompt},
};
use serde::Serialize;

pub struct SimulatedWorker {
    config: SimulationWorkerConfig,
}

impl SimulatedWorker {
    pub fn new(config: SimulationWorkerConfig) -> Self {
        Self { config }
    }

    fn write_stage_result(
        &self,
        artifacts: &StageArtifactSet,
        command: Vec<String>,
        prompt: String,
        output_json: String,
    ) -> Result<WorkerResult> {
        fs::write(&artifacts.prompt_file, prompt)
            .with_context(|| format!("failed to write {}", artifacts.prompt_file.display()))?;
        fs::write(&artifacts.stdout_log, "simulated\n")
            .with_context(|| format!("failed to write {}", artifacts.stdout_log.display()))?;
        fs::write(&artifacts.stderr_log, "")
            .with_context(|| format!("failed to write {}", artifacts.stderr_log.display()))?;
        fs::write(&artifacts.output_file, output_json)
            .with_context(|| format!("failed to write {}", artifacts.output_file.display()))?;

        Ok(WorkerResult {
            stage: artifacts.stage,
            status: WorkerStatus::Prepared,
            command,
            prompt_file: artifacts.prompt_file.clone(),
            output_file: artifacts.output_file.clone(),
            stdout_log: artifacts.stdout_log.clone(),
            stderr_log: artifacts.stderr_log.clone(),
            notes: vec!["Simulated worker emitted deterministic artifacts.".to_string()],
            session_id: Some(format!(
                "{}-{}-{:02}",
                self.config.session_prefix,
                artifacts.stage.as_str(),
                artifacts.attempt
            )),
        })
    }

    fn evaluation_status(&self, attempt: usize) -> QaStatus {
        self.config
            .evaluator_statuses
            .get(attempt.saturating_sub(1))
            .copied()
            .or_else(|| self.config.evaluator_statuses.last().copied())
            .unwrap_or(QaStatus::Pass)
    }
}

#[async_trait]
impl WorkerAdapter for SimulatedWorker {
    async fn plan(
        &self,
        context: &WorkerContext,
        artifacts: &StageArtifactSet,
        request: &PlanningRequest,
    ) -> Result<WorkerResult> {
        let prompt = render_worker_prompt(
            context,
            None,
            artifacts.stage,
            &context.planner_prompt,
            &context.planner_schema,
            request,
        )?;
        let output = serde_json::to_string_pretty(&request.synthesize_plan())
            .context("failed to serialize simulated plan")?;
        self.write_stage_result(
            artifacts,
            vec!["simulated".to_string(), "plan".to_string()],
            prompt,
            output,
        )
    }

    async fn build(
        &self,
        context: &WorkerContext,
        feature: &FeatureLayout,
        artifacts: &StageArtifactSet,
        contract: &loopsmith_core::domain::FeatureContract,
    ) -> Result<WorkerResult> {
        let prompt = render_worker_prompt(
            context,
            Some(feature),
            artifacts.stage,
            &context.builder_prompt,
            &context.builder_schema,
            contract,
        )?;
        let output = serde_json::to_string_pretty(&BuilderHandoff {
            summary: "Simulated builder handoff. No code changes were made.".to_string(),
            changed_files: Vec::new(),
            verification: vec!["Simulation skipped workspace mutation.".to_string()],
            open_questions: vec![
                "Replace the simulated worker with codex_cli for real runs.".to_string(),
            ],
        })
        .context("failed to serialize simulated build handoff")?;

        self.write_stage_result(
            artifacts,
            vec!["simulated".to_string(), "build".to_string()],
            prompt,
            output,
        )
    }

    async fn evaluate(
        &self,
        context: &WorkerContext,
        feature: &FeatureLayout,
        artifacts: &StageArtifactSet,
        request: &EvaluationRequest,
    ) -> Result<WorkerResult> {
        let prompt = render_worker_prompt(
            context,
            Some(feature),
            artifacts.stage,
            &context.evaluator_prompt,
            &context.qa_schema,
            request,
        )?;
        let status = self.evaluation_status(artifacts.attempt);
        let output = serde_json::to_string_pretty(&QaReport {
            status,
            summary: format!(
                "Simulated evaluator result for attempt {}.",
                artifacts.attempt
            ),
            findings: vec!["Simulation skipped external verification.".to_string()],
            next_actions: if status == QaStatus::Pass {
                vec!["Current feature accepted by the simulated worker.".to_string()]
            } else {
                vec!["Trigger a repair attempt and re-evaluate.".to_string()]
            },
            checks: request
                .verification_commands
                .iter()
                .enumerate()
                .map(|(index, command)| QaCheck {
                    name: format!("sim-check-{:02}", index + 1),
                    command: command.clone(),
                    rationale: "Deterministic placeholder check from the simulated worker."
                        .to_string(),
                })
                .collect(),
        })
        .context("failed to serialize simulated qa report")?;

        self.write_stage_result(
            artifacts,
            vec!["simulated".to_string(), "evaluate".to_string()],
            prompt,
            output,
        )
    }

    async fn repair(
        &self,
        context: &WorkerContext,
        feature: &FeatureLayout,
        artifacts: &StageArtifactSet,
        contract: &loopsmith_core::domain::FeatureContract,
        builder_handoff: &BuilderHandoff,
        qa_report: &QaReport,
        previous_session_id: Option<&str>,
    ) -> Result<WorkerResult> {
        #[derive(Serialize)]
        struct RepairPayload<'a> {
            contract: &'a loopsmith_core::domain::FeatureContract,
            builder_handoff: &'a BuilderHandoff,
            qa_report: &'a QaReport,
        }

        let payload = RepairPayload {
            contract,
            builder_handoff,
            qa_report,
        };
        let prompt = render_worker_prompt(
            context,
            Some(feature),
            artifacts.stage,
            &context.builder_prompt,
            &context.builder_schema,
            &payload,
        )?;
        let output = serde_json::to_string_pretty(&BuilderHandoff {
            summary: "Simulated repair handoff. No code changes were made.".to_string(),
            changed_files: Vec::new(),
            verification: vec!["Simulation skipped workspace mutation.".to_string()],
            open_questions: vec![
                "Replace the simulated worker with codex_cli for real runs.".to_string(),
            ],
        })
        .context("failed to serialize simulated repair handoff")?;

        let mut command = vec!["simulated".to_string(), "repair".to_string()];
        if let Some(session_id) = previous_session_id {
            command.push("resume".to_string());
            command.push(session_id.to_string());
        }

        self.write_stage_result(artifacts, command, prompt, output)
    }
}
