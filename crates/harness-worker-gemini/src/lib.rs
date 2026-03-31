use std::{fmt::Write as _, fs, io, path::Path, process::Stdio};

use anyhow::{Context, Result};
use async_trait::async_trait;
use loopsmith_core::{
    artifacts::{FeatureLayout, StageArtifactSet},
    config::GeminiWorkerConfig,
    domain::{
        BuilderHandoff, EvaluationRequest, PlanningRequest, QaReport, WorkerResult, WorkerStage,
        WorkerStatus,
    },
    worker::{WorkerAdapter, WorkerContext, render_worker_prompt},
};
use serde::Serialize;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::time::{Duration, sleep};
use tracing::{info, warn};

pub struct GeminiCliWorker {
    config: GeminiWorkerConfig,
}

impl GeminiCliWorker {
    pub fn new(config: GeminiWorkerConfig) -> Self {
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

        let schema = fs::read_to_string(schema_path)
            .with_context(|| format!("failed to read schema {}", schema_path.display()))?;
        let prompt_with_schema = format!(
            "{prompt}\n\nYou MUST respond with ONLY valid JSON matching this schema:\n{schema}\n"
        );

        let command = self.exec_command_for_stage(context);
        self.execute_command(artifacts, command, &prompt_with_schema, artifacts.stage)
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
            "starting gemini worker stage"
        );

        let mut process = Command::new(&self.config.binary);
        process
            .args(command.iter().skip(1))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(unix)]
        {
            process.process_group(0);
        }

        let mut child = process
            .spawn()
            .with_context(|| format!("failed to execute {}", self.config.binary))?;
        let child_pid = child.id();

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(prompt.as_bytes())
                .await
                .context("failed to write prompt to gemini stdin")?;
        }

        let child_stdout = child
            .stdout
            .take()
            .context("failed to capture gemini stdout")?;
        let child_stderr = child
            .stderr
            .take()
            .context("failed to capture gemini stderr")?;

        let stdout_log_path = artifacts.stdout_log.clone();
        let stderr_log_path = artifacts.stderr_log.clone();

        let stdout_handle: tokio::task::JoinHandle<Result<Vec<u8>>> =
            tokio::spawn(async move {
                tee_stream_to_file(child_stdout, &stdout_log_path).await
            });
        let stderr_handle: tokio::task::JoinHandle<Result<Vec<u8>>> =
            tokio::spawn(async move {
                tee_stream_to_file(child_stderr, &stderr_log_path).await
            });

        let wait_result = child.wait().await;

        let stdout_bytes = stdout_handle
            .await
            .context("stdout tee task panicked")?
            .context("failed to stream gemini stdout")?;
        let stderr_bytes = stderr_handle
            .await
            .context("stderr tee task panicked")?
            .context("failed to stream gemini stderr")?;

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

        let session_id = extract_session_id(&stdout_bytes);
        info!(
            stage = stage.as_str(),
            attempt = artifacts.attempt,
            status = %status,
            session_id = session_id.as_deref().unwrap_or("-"),
            "completed gemini worker stage"
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

        fs::write(&artifacts.output_file, &stdout_bytes)
            .with_context(|| format!("failed to write {}", artifacts.output_file.display()))?;

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

    fn exec_command_for_stage(&self, context: &WorkerContext) -> Vec<String> {
        vec![
            self.config.binary.clone(),
            "-p".to_string(),
            "-".to_string(),
            "--model".to_string(),
            self.config.model.clone(),
            "--sandbox".to_string(),
            self.config.sandbox.clone(),
            "--json".to_string(),
            "-C".to_string(),
            context.workspace.display().to_string(),
        ]
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

    anyhow::bail!("gemini worker process group {pid} remained alive after cleanup")
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
        _ => Err(err).with_context(|| format!("failed to probe gemini worker process group {pid}")),
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
        format!("failed to deliver signal {signal} to gemini worker process group {pid}")
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
            "cleaned up lingering gemini worker descendant processes"
        ),
        Ok(false) => {}
        Err(error) => warn!(
            stage = stage.as_str(),
            attempt,
            pid = pid.unwrap_or_default(),
            error = %error,
            "failed to clean up gemini worker descendant processes"
        ),
    }
}

#[async_trait]
impl WorkerAdapter for GeminiCliWorker {
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
        _previous_session_id: Option<&str>,
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

        self.run_exec_stage(
            context,
            Some(feature),
            artifacts,
            &context.builder_prompt,
            &context.builder_schema,
            &payload,
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
        "gemini stage {} failed with status {}",
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

    #[test]
    fn extract_session_id_returns_none_for_empty_stdout() {
        assert_eq!(extract_session_id(b""), None);
    }
}
