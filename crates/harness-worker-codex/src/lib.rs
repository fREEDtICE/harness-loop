use std::{fs, path::Path, process::Stdio};

use anyhow::{Context, Result};
use async_trait::async_trait;
use harness_core::{
    artifacts::{FeatureLayout, StageArtifactSet},
    config::CodexWorkerConfig,
    domain::{
        BuilderHandoff, EvaluationRequest, PlanningRequest, QaReport, WorkerResult, WorkerStage,
        WorkerStatus,
    },
    worker::{WorkerAdapter, WorkerContext, render_worker_prompt},
};
use serde::Serialize;
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

pub struct CodexCliWorker {
    config: CodexWorkerConfig,
}

impl CodexCliWorker {
    pub fn new(config: CodexWorkerConfig) -> Self {
        Self { config }
    }

    async fn run_exec_stage<T: Serialize>(
        &self,
        context: &WorkerContext,
        feature: Option<&FeatureLayout>,
        artifacts: &StageArtifactSet,
        template_path: &Path,
        schema_path: &Path,
        payload: &T,
    ) -> Result<WorkerResult> {
        let prompt = render_worker_prompt(
            context,
            feature,
            artifacts.stage,
            template_path,
            schema_path,
            payload,
        )?;
        fs::write(&artifacts.prompt_file, &prompt)
            .with_context(|| format!("failed to write {}", artifacts.prompt_file.display()))?;

        let command = self.exec_command_for_stage(context, schema_path, &artifacts.output_file);
        self.execute_command(artifacts, command, &prompt, artifacts.stage)
            .await
    }

    async fn run_repair_stage<T: Serialize>(
        &self,
        context: &WorkerContext,
        feature: &FeatureLayout,
        artifacts: &StageArtifactSet,
        template_path: &Path,
        schema_path: &Path,
        payload: &T,
        previous_session_id: Option<&str>,
    ) -> Result<WorkerResult> {
        let prompt = render_worker_prompt(
            context,
            Some(feature),
            artifacts.stage,
            template_path,
            schema_path,
            payload,
        )?;
        fs::write(&artifacts.prompt_file, &prompt)
            .with_context(|| format!("failed to write {}", artifacts.prompt_file.display()))?;

        if let Some(session_id) = previous_session_id.filter(|_| self.config.resume_sessions) {
            let command =
                self.resume_command_for_stage(context, session_id, &artifacts.output_file);
            let mut result = self
                .execute_command(artifacts, command, &prompt, artifacts.stage)
                .await?;

            if result.session_id.is_none() {
                result.session_id = Some(session_id.to_string());
            }

            return Ok(result);
        }

        let command = self.exec_command_for_stage(context, schema_path, &artifacts.output_file);
        self.execute_command(artifacts, command, &prompt, artifacts.stage)
            .await
    }

    async fn execute_command(
        &self,
        artifacts: &StageArtifactSet,
        command: Vec<String>,
        prompt: &str,
        stage: WorkerStage,
    ) -> Result<WorkerResult> {
        let mut process = Command::new(&self.config.binary);
        process
            .args(command.iter().skip(1))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = process
            .spawn()
            .with_context(|| format!("failed to execute {}", self.config.binary))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(prompt.as_bytes())
                .await
                .context("failed to write prompt to codex stdin")?;
        }

        let output = child.wait_with_output().await.with_context(|| {
            format!(
                "failed while waiting for {} stage {}",
                self.config.binary,
                stage.as_str()
            )
        })?;

        fs::write(&artifacts.stdout_log, &output.stdout)
            .with_context(|| format!("failed to write {}", artifacts.stdout_log.display()))?;
        fs::write(&artifacts.stderr_log, &output.stderr)
            .with_context(|| format!("failed to write {}", artifacts.stderr_log.display()))?;

        if !output.status.success() {
            anyhow::bail!(
                "codex stage {} failed with status {}",
                stage.as_str(),
                output.status
            );
        }

        Ok(WorkerResult {
            stage,
            status: WorkerStatus::Executed,
            command,
            prompt_file: artifacts.prompt_file.clone(),
            output_file: artifacts.output_file.clone(),
            stdout_log: artifacts.stdout_log.clone(),
            stderr_log: artifacts.stderr_log.clone(),
            notes: vec!["Execution completed.".to_string()],
            session_id: extract_session_id(&output.stdout),
        })
    }

    fn exec_command_for_stage(
        &self,
        context: &WorkerContext,
        schema_path: &Path,
        output_file: &Path,
    ) -> Vec<String> {
        let mut command = vec![
            self.config.binary.clone(),
            "-a".to_string(),
            "never".to_string(),
            "exec".to_string(),
            "--json".to_string(),
            "-C".to_string(),
            context.workspace.display().to_string(),
            "-m".to_string(),
            self.config.model.clone(),
            "-s".to_string(),
            self.config.sandbox.clone(),
        ];

        if self.config.full_auto {
            command.push("--full-auto".to_string());
        }

        if self.config.skip_git_repo_check {
            command.push("--skip-git-repo-check".to_string());
        }

        command.extend([
            "--output-schema".to_string(),
            schema_path.display().to_string(),
            "-o".to_string(),
            output_file.display().to_string(),
            "-".to_string(),
        ]);

        command
    }

    fn resume_command_for_stage(
        &self,
        context: &WorkerContext,
        session_id: &str,
        output_file: &Path,
    ) -> Vec<String> {
        let mut command = vec![
            self.config.binary.clone(),
            "-a".to_string(),
            "never".to_string(),
            "exec".to_string(),
            "resume".to_string(),
            "--json".to_string(),
            "-C".to_string(),
            context.workspace.display().to_string(),
            "-m".to_string(),
            self.config.model.clone(),
        ];

        if self.config.full_auto {
            command.push("--full-auto".to_string());
        }

        if self.config.skip_git_repo_check {
            command.push("--skip-git-repo-check".to_string());
        }

        command.extend([
            "-o".to_string(),
            output_file.display().to_string(),
            session_id.to_string(),
            "-".to_string(),
        ]);

        command
    }
}

#[async_trait]
impl WorkerAdapter for CodexCliWorker {
    async fn plan(
        &self,
        context: &WorkerContext,
        artifacts: &StageArtifactSet,
        request: &PlanningRequest,
    ) -> Result<WorkerResult> {
        self.run_exec_stage(
            context,
            None,
            artifacts,
            &context.planner_prompt,
            &context.planner_schema,
            request,
        )
        .await
    }

    async fn build(
        &self,
        context: &WorkerContext,
        feature: &FeatureLayout,
        artifacts: &StageArtifactSet,
        contract: &harness_core::domain::FeatureContract,
    ) -> Result<WorkerResult> {
        self.run_exec_stage(
            context,
            Some(feature),
            artifacts,
            &context.builder_prompt,
            &context.builder_schema,
            contract,
        )
        .await
    }

    async fn evaluate(
        &self,
        context: &WorkerContext,
        feature: &FeatureLayout,
        artifacts: &StageArtifactSet,
        request: &EvaluationRequest,
    ) -> Result<WorkerResult> {
        self.run_exec_stage(
            context,
            Some(feature),
            artifacts,
            &context.evaluator_prompt,
            &context.qa_schema,
            request,
        )
        .await
    }

    async fn repair(
        &self,
        context: &WorkerContext,
        feature: &FeatureLayout,
        artifacts: &StageArtifactSet,
        contract: &harness_core::domain::FeatureContract,
        builder_handoff: &BuilderHandoff,
        qa_report: &QaReport,
        previous_session_id: Option<&str>,
    ) -> Result<WorkerResult> {
        #[derive(Serialize)]
        struct RepairPayload<'a> {
            contract: &'a harness_core::domain::FeatureContract,
            builder_handoff: &'a BuilderHandoff,
            qa_report: &'a QaReport,
        }

        let payload = RepairPayload {
            contract,
            builder_handoff,
            qa_report,
        };

        self.run_repair_stage(
            context,
            feature,
            artifacts,
            &context.builder_prompt,
            &context.builder_schema,
            &payload,
            previous_session_id,
        )
        .await
    }
}

fn extract_session_id(stdout: &[u8]) -> Option<String> {
    std::str::from_utf8(stdout).ok().and_then(|text| {
        text.lines().find_map(|line| {
            let value: Value = serde_json::from_str(line).ok()?;
            let kind = value.get("type")?.as_str()?;

            if kind == "thread.started" {
                value
                    .get("thread_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            } else {
                None
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::extract_session_id;

    #[test]
    fn extract_session_id_from_jsonl_stdout() {
        let stdout =
            br#"{"type":"thread.started","thread_id":"019d232e-675d-7131-9783-da5c3cee4e1f"}
{"type":"turn.started"}
"#;

        assert_eq!(
            extract_session_id(stdout).as_deref(),
            Some("019d232e-675d-7131-9783-da5c3cee4e1f")
        );
    }
}
