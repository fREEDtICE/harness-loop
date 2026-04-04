use std::{fmt::Write as _, fs, io, path::Path, process::Stdio};

use anyhow::{Context, Result};
use async_trait::async_trait;
use loopsmith_core::{
    artifacts::{FeatureLayout, StageArtifactSet},
    config::ClaudeWorkerConfig,
    domain::{
        BuilderHandoff, EvaluationRequest, PlanningRequest, QaReport, WorkerResult, WorkerStage,
        WorkerStatus,
    },
    shell_env::wrap_command_for_user_shell,
    worker::{WorkerAdapter, WorkerContext, render_worker_prompt},
};
use serde::Serialize;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::time::{Duration, sleep};
use tracing::{info, warn};

pub struct ClaudeCliWorker {
    config: ClaudeWorkerConfig,
}

impl ClaudeCliWorker {
    pub fn new(config: ClaudeWorkerConfig) -> Self {
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

        let schema_content = fs::read_to_string(schema_path)
            .with_context(|| format!("failed to read schema {}", schema_path.display()))?;
        let command = self.exec_command_for_stage(context, &schema_content);
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
                "resuming claude worker stage from prior session"
            );
            let command = self.resume_command_for_stage(session_id);
            let mut result = self
                .execute_command(artifacts, command, &prompt, artifacts.stage)
                .await?;

            if result.session_id.is_none() {
                result.session_id = Some(session_id.to_string());
            }

            return Ok(result);
        }

        let schema_content = fs::read_to_string(schema_path)
            .with_context(|| format!("failed to read schema {}", schema_path.display()))?;
        let command = self.exec_command_for_stage(context, &schema_content);
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
            "starting claude worker stage"
        );

        let (shell_program, shell_args) = wrap_command_for_user_shell(&command);
        let mut process = Command::new(&shell_program);
        process
            .args(&shell_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(unix)]
        {
            process.process_group(0);
        }

        let mut child = process.spawn().with_context(|| {
            format!(
                "failed to execute `{}`. Is the Claude CLI installed and available in PATH?",
                self.config.binary
            )
        })?;
        let child_pid = child.id();

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(prompt.as_bytes())
                .await
                .context("failed to write prompt to claude stdin")?;
        }

        let child_stdout = child
            .stdout
            .take()
            .context("failed to capture claude stdout")?;
        let child_stderr = child
            .stderr
            .take()
            .context("failed to capture claude stderr")?;

        let stdout_log_path = artifacts.stdout_log.clone();
        let stderr_log_path = artifacts.stderr_log.clone();

        let stdout_handle: tokio::task::JoinHandle<Result<Vec<u8>>> =
            tokio::spawn(async move { tee_stream_to_file(child_stdout, &stdout_log_path).await });
        let stderr_handle: tokio::task::JoinHandle<Result<Vec<u8>>> =
            tokio::spawn(async move { tee_stream_to_file(child_stderr, &stderr_log_path).await });

        let wait_result = child.wait().await;

        let stdout_bytes = stdout_handle
            .await
            .context("stdout tee task panicked")?
            .context("failed to stream claude stdout")?;
        let stderr_bytes = stderr_handle
            .await
            .context("stderr tee task panicked")?
            .context("failed to stream claude stderr")?;

        #[cfg(unix)]
        let cleanup_result = cleanup_process_group(child_pid).await;

        let status = match wait_result {
            Ok(status) => status,
            Err(error) => {
                #[cfg(unix)]
                log_cleanup_result(stage, artifacts.attempt, child_pid, &cleanup_result);

                return Err(error).with_context(|| {
                    format!(
                        "failed while waiting for {} stage {}",
                        self.config.binary,
                        stage.as_str()
                    )
                });
            }
        };

        #[cfg(unix)]
        log_cleanup_result(stage, artifacts.attempt, child_pid, &cleanup_result);

        let (session_id, json_output) = extract_session_and_output(&stdout_bytes);
        info!(
            stage = stage.as_str(),
            attempt = artifacts.attempt,
            status = %status,
            session_id = session_id.as_deref().unwrap_or("-"),
            "completed claude worker stage"
        );

        if !status.success() {
            anyhow::bail!(
                "{}",
                format_stage_failure(
                    stage,
                    &status,
                    &command,
                    artifacts,
                    &stdout_bytes,
                    &stderr_bytes,
                    session_id.as_deref(),
                )
            );
        }

        if let Some(json_bytes) = json_output {
            fs::write(&artifacts.output_file, &json_bytes)
                .with_context(|| format!("failed to write {}", artifacts.output_file.display()))?;
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

    fn exec_command_for_stage(&self, context: &WorkerContext, schema_content: &str) -> Vec<String> {
        let mut command = vec![
            self.config.binary.clone(),
            "-p".to_string(),
            "-".to_string(),
        ];

        if self.config.dangerously_skip_permissions {
            command.push("--dangerously-skip-permissions".to_string());
        }

        command.extend([
            "--output-format".to_string(),
            "json".to_string(),
            "--model".to_string(),
            self.config.model.clone(),
            "-C".to_string(),
            context.workspace.display().to_string(),
            "--json-schema".to_string(),
            schema_content.to_string(),
        ]);

        command
    }

    fn resume_command_for_stage(&self, session_id: &str) -> Vec<String> {
        let mut command = vec![
            self.config.binary.clone(),
            "--resume".to_string(),
            session_id.to_string(),
            "-p".to_string(),
            "-".to_string(),
        ];

        if self.config.dangerously_skip_permissions {
            command.push("--dangerously-skip-permissions".to_string());
        }

        command.extend(["--output-format".to_string(), "json".to_string()]);

        command
    }
}

async fn tee_stream_to_file<R: tokio::io::AsyncRead + Unpin>(
    reader: R,
    path: &Path,
) -> Result<Vec<u8>> {
    let mut buf_reader = BufReader::new(reader);
    let mut file = tokio::fs::File::create(path)
        .await
        .with_context(|| format!("failed to create {}", path.display()))?;
    let mut collected = Vec::new();
    loop {
        let chunk = buf_reader
            .fill_buf()
            .await
            .context("failed to read from child process stream")?;
        if chunk.is_empty() {
            break;
        }
        let len = chunk.len();
        file.write_all(chunk)
            .await
            .with_context(|| format!("failed to write to {}", path.display()))?;
        collected.extend_from_slice(chunk);
        buf_reader.consume(len);
    }
    file.flush()
        .await
        .with_context(|| format!("failed to flush {}", path.display()))?;
    Ok(collected)
}

#[cfg(unix)]
async fn cleanup_process_group(pid: Option<u32>) -> Result<bool> {
    let Some(pid) = pid else {
        return Ok(false);
    };

    if !process_group_exists(pid)? {
        return Ok(false);
    }

    send_signal(pid, libc::SIGTERM)?;
    for _ in 0..10 {
        if !process_group_exists(pid)? {
            return Ok(true);
        }
        sleep(Duration::from_millis(50)).await;
    }

    send_signal(pid, libc::SIGKILL)?;
    for _ in 0..10 {
        if !process_group_exists(pid)? {
            return Ok(true);
        }
        sleep(Duration::from_millis(20)).await;
    }

    anyhow::bail!("claude worker process group {pid} remained alive after cleanup")
}

#[cfg(unix)]
fn process_group_exists(pid: u32) -> Result<bool> {
    let target = -(pid as libc::pid_t);
    let rc = unsafe { libc::kill(target, 0) };
    if rc == 0 {
        return Ok(true);
    }

    let err = io::Error::last_os_error();
    match err.raw_os_error() {
        Some(libc::ESRCH) => Ok(false),
        Some(libc::EPERM) => Ok(true),
        _ => Err(err).with_context(|| format!("failed to probe claude worker process group {pid}")),
    }
}

#[cfg(unix)]
fn send_signal(pid: u32, signal: libc::c_int) -> Result<()> {
    let target = -(pid as libc::pid_t);
    let rc = unsafe { libc::kill(target, signal) };
    if rc == 0 {
        return Ok(());
    }

    let err = io::Error::last_os_error();
    if err.raw_os_error() == Some(libc::ESRCH) {
        return Ok(());
    }

    Err(err).with_context(|| {
        format!("failed to deliver signal {signal} to claude worker process group {pid}")
    })
}

#[cfg(unix)]
fn log_cleanup_result(
    stage: WorkerStage,
    attempt: usize,
    pid: Option<u32>,
    cleanup_result: &Result<bool>,
) {
    match cleanup_result {
        Ok(true) => info!(
            stage = stage.as_str(),
            attempt,
            pid = pid.unwrap_or_default(),
            "cleaned up lingering claude worker descendant processes"
        ),
        Ok(false) => {}
        Err(error) => warn!(
            stage = stage.as_str(),
            attempt,
            pid = pid.unwrap_or_default(),
            error = %error,
            "failed to clean up claude worker descendant processes"
        ),
    }
}

#[async_trait]
impl WorkerAdapter for ClaudeCliWorker {
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
        contract: &loopsmith_core::domain::FeatureContract,
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

fn extract_session_and_output(stdout: &[u8]) -> (Option<String>, Option<Vec<u8>>) {
    let text = match std::str::from_utf8(stdout) {
        Ok(text) => text,
        Err(_) => return (None, None),
    };

    let mut session_id = None;
    let mut result_json = None;

    for line in text.lines() {
        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => continue,
        };

        if session_id.is_none() {
            if let Some(id) = value.get("session_id").and_then(Value::as_str) {
                session_id = Some(id.to_string());
            }
        }

        if let Some(result) = value.get("result") {
            result_json = Some(serde_json::to_vec_pretty(result).unwrap_or_default());
        }
    }

    (session_id, result_json)
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
        "claude stage {} failed with status {}",
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
    use super::{extract_session_and_output, render_output_excerpt};

    #[test]
    fn extract_session_id_from_json_stdout() {
        let stdout = br#"{"session_id":"sess-abc-123","type":"message"}
{"result":{"ok":true}}
"#;

        let (session_id, result_json) = extract_session_and_output(stdout);
        assert_eq!(session_id.as_deref(), Some("sess-abc-123"));
        assert!(result_json.is_some());

        let parsed: serde_json::Value =
            serde_json::from_slice(&result_json.unwrap()).expect("valid json");
        assert_eq!(parsed.get("ok").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn extract_session_id_missing_when_absent() {
        let stdout = br#"{"type":"message"}
"#;
        let (session_id, _) = extract_session_and_output(stdout);
        assert!(session_id.is_none());
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
