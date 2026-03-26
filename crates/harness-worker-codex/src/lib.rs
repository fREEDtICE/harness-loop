use std::{fmt::Write as _, fs, path::Path, process::Stdio};

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
use tracing::info;

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
            info!(
                stage = artifacts.stage.as_str(),
                attempt = artifacts.attempt,
                session_id,
                workspace = %context.workspace.display(),
                "resuming codex worker stage from prior session"
            );
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
        let rendered_command = render_command(&command);
        info!(
            stage = stage.as_str(),
            attempt = artifacts.attempt,
            command = %rendered_command,
            prompt_file = %artifacts.prompt_file.display(),
            output_file = %artifacts.output_file.display(),
            stdout_log = %artifacts.stdout_log.display(),
            stderr_log = %artifacts.stderr_log.display(),
            "starting codex worker stage"
        );

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

        let session_id = extract_session_id(&output.stdout);
        info!(
            stage = stage.as_str(),
            attempt = artifacts.attempt,
            status = %output.status,
            session_id = session_id.as_deref().unwrap_or("-"),
            "completed codex worker stage"
        );

        if !output.status.success() {
            anyhow::bail!(
                "{}",
                format_stage_failure(
                    stage,
                    &output.status,
                    &command,
                    artifacts,
                    &output.stdout,
                    &output.stderr,
                    session_id.as_deref(),
                )
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
            session_id,
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
            "-C".to_string(),
            context.workspace.display().to_string(),
            "resume".to_string(),
            "--json".to_string(),
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

fn format_stage_failure(
    stage: WorkerStage,
    status: &std::process::ExitStatus,
    command: &[String],
    artifacts: &StageArtifactSet,
    stdout: &[u8],
    stderr: &[u8],
    session_id: Option<&str>,
) -> String {
    let mut message = format!(
        "codex stage {} failed with status {}",
        stage.as_str(),
        status
    );
    let _ = write!(
        message,
        "\ncommand: {}\
\nprompt_file: {}\
\noutput_file: {}\
\nstdout_log: {}\
\nstderr_log: {}",
        render_command(command),
        artifacts.prompt_file.display(),
        artifacts.output_file.display(),
        artifacts.stdout_log.display(),
        artifacts.stderr_log.display(),
    );

    if let Some(session_id) = session_id {
        let _ = write!(message, "\nsession_id: {session_id}");
    }

    let _ = write!(
        message,
        "\nstdout_excerpt:\n{}\nstderr_excerpt:\n{}",
        render_output_excerpt(stdout),
        render_output_excerpt(stderr),
    );
    message
}

fn render_command(command: &[String]) -> String {
    command
        .iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }

    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':' | '='))
    {
        return value.to_string();
    }

    format!("'{}'", value.replace('\'', r"'\''"))
}

fn render_output_excerpt(output: &[u8]) -> String {
    let text = String::from_utf8_lossy(output);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return "-".to_string();
    }

    let lines = trimmed.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(20);
    let excerpt = lines[start..].join("\n");
    truncate_start(&excerpt, 4_000)
}

fn truncate_start(value: &str, max_chars: usize) -> String {
    let total = value.chars().count();
    if total <= max_chars {
        return value.to_string();
    }

    let start = total.saturating_sub(max_chars);
    let truncated = value.chars().skip(start).collect::<String>();
    format!("...\n{truncated}")
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use harness_core::{artifacts::RunLayout, config::CodexWorkerConfig, worker::WorkerContext};
    use uuid::Uuid;

    use super::{CodexCliWorker, extract_session_id, render_output_excerpt};

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

    #[test]
    fn resume_command_places_exec_cd_before_resume_subcommand() {
        let worker = CodexCliWorker::new(CodexWorkerConfig {
            binary: "codex".to_string(),
            model: "gpt-5.4".to_string(),
            sandbox: "workspace-write".to_string(),
            full_auto: true,
            skip_git_repo_check: true,
            resume_sessions: true,
        });
        let context = WorkerContext {
            run_id: Uuid::nil(),
            workspace: PathBuf::from("/tmp/workspace"),
            layout: RunLayout {
                root: PathBuf::from("/tmp/run"),
                inputs_dir: PathBuf::from("/tmp/run/inputs"),
                prompt_inputs_dir: PathBuf::from("/tmp/run/inputs/prompts"),
                planner_prompt_file: PathBuf::from("/tmp/run/inputs/prompts/planner.md"),
                builder_prompt_file: PathBuf::from("/tmp/run/inputs/prompts/builder.md"),
                evaluator_prompt_file: PathBuf::from("/tmp/run/inputs/prompts/evaluator.md"),
                launch_file: PathBuf::from("/tmp/run/launch.json"),
                request_file: PathBuf::from("/tmp/run/request.md"),
                plan_file: PathBuf::from("/tmp/run/plan.json"),
                runtime_plan_file: PathBuf::from("/tmp/run/runtime-plan.json"),
                state_file: PathBuf::from("/tmp/run/run-state.json"),
                manifest_file: PathBuf::from("/tmp/run/manifest.json"),
                features_dir: PathBuf::from("/tmp/run/features"),
                worker_dir: PathBuf::from("/tmp/run/worker"),
            },
            planner_prompt: PathBuf::from("/tmp/prompts/planner.md"),
            builder_prompt: PathBuf::from("/tmp/prompts/builder.md"),
            evaluator_prompt: PathBuf::from("/tmp/prompts/evaluator.md"),
            planner_schema: PathBuf::from("/tmp/schemas/plan.json"),
            builder_schema: PathBuf::from("/tmp/schemas/build.json"),
            qa_schema: PathBuf::from("/tmp/schemas/qa.json"),
        };

        let command = worker.resume_command_for_stage(
            &context,
            "session-123",
            Path::new("/tmp/run/worker/outputs/repair-01-last-message.json"),
        );
        let cd_index = command.iter().position(|arg| arg == "-C").unwrap();
        let resume_index = command.iter().position(|arg| arg == "resume").unwrap();

        assert!(cd_index < resume_index);
        assert_eq!(command[cd_index + 1], "/tmp/workspace");
    }

    #[test]
    fn render_output_excerpt_keeps_tail_for_debugging() {
        let output = (0..30)
            .map(|index| format!("line-{index}"))
            .collect::<Vec<_>>()
            .join("\n");

        let excerpt = render_output_excerpt(output.as_bytes());

        assert!(!excerpt.contains("line-0"));
        assert!(excerpt.contains("line-29"));
        assert!(excerpt.contains("line-10"));
    }
}
