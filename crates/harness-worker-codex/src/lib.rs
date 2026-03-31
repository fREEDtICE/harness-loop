use std::{fmt::Write as _, fs, io, path::Path, process::Stdio};

use anyhow::{Context, Result};
use async_trait::async_trait;
use loopsmith_core::{
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
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::time::{Duration, sleep};
use tracing::{info, warn};

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

        #[cfg(unix)]
        {
            process.process_group(0);
        }

        let mut child = process
            .spawn()
            .with_context(|| {
                format!(
                    "failed to execute `{}`. Is the Codex CLI installed and available in PATH?",
                    self.config.binary
                )
            })?;
        let child_pid = child.id();

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(prompt.as_bytes())
                .await
                .context("failed to write prompt to codex stdin")?;
        }

        let child_stdout = child
            .stdout
            .take()
            .context("failed to capture codex stdout")?;
        let child_stderr = child
            .stderr
            .take()
            .context("failed to capture codex stderr")?;

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
            .context("failed to stream codex stdout")?;
        let stderr_bytes = stderr_handle
            .await
            .context("stderr tee task panicked")?
            .context("failed to stream codex stderr")?;

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
            "completed codex worker stage"
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

    anyhow::bail!("codex worker process group {pid} remained alive after cleanup")
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
        _ => Err(err).with_context(|| format!("failed to probe codex worker process group {pid}")),
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
        format!("failed to deliver signal {signal} to codex worker process group {pid}")
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
            "cleaned up lingering codex worker descendant processes"
        ),
        Ok(false) => {}
        Err(error) => warn!(
            stage = stage.as_str(),
            attempt,
            pid = pid.unwrap_or_default(),
            error = %error,
            "failed to clean up codex worker descendant processes"
        ),
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

    #[cfg(unix)]
    if status.code() == Some(127) {
        let _ = write!(
            message,
            "\n\nExit code 127 means \"command not found\". This usually indicates that \
             the CLI binary (or a dependency like `node`) is not available in PATH.\n\
             Current PATH: {}",
            std::env::var("PATH").unwrap_or_else(|_| "(not set)".to_string())
        );
    }

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
    use std::{
        fs,
        path::{Path, PathBuf},
        process::{Command as StdCommand, Stdio as StdStdio},
        time::Duration,
    };

    use anyhow::Result;
    use loopsmith_core::{
        artifacts::{FileArtifactStore, RunLayout},
        config::CodexWorkerConfig,
        domain::{PlanningRequest, WorkerStage},
        worker::{WorkerAdapter, WorkerContext},
    };
    use tempfile::tempdir;
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

    #[cfg(unix)]
    #[tokio::test]
    async fn plan_stage_cleans_up_spawned_descendants() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        struct PidGuard(Option<u32>);

        impl Drop for PidGuard {
            fn drop(&mut self) {
                if let Some(pid) = self.0.take() {
                    let _ = StdCommand::new("kill")
                        .arg("-KILL")
                        .arg(pid.to_string())
                        .stdout(StdStdio::null())
                        .stderr(StdStdio::null())
                        .status();
                }
            }
        }

        let temp = tempdir()?;
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&workspace)?;

        let prompts_dir = temp.path().join("prompts");
        let schemas_dir = temp.path().join("schemas");
        fs::create_dir_all(&prompts_dir)?;
        fs::create_dir_all(&schemas_dir)?;
        fs::write(prompts_dir.join("planner.md"), "planner prompt\n")?;
        fs::write(prompts_dir.join("builder.md"), "builder prompt\n")?;
        fs::write(prompts_dir.join("evaluator.md"), "evaluator prompt\n")?;
        fs::write(schemas_dir.join("planner.json"), "{}\n")?;
        fs::write(schemas_dir.join("builder.json"), "{}\n")?;
        fs::write(schemas_dir.join("qa.json"), "{}\n")?;

        let pid_file = temp.path().join("codex-descendant.pid");
        let script_path = temp.path().join("fake-codex.sh");
        fs::write(
            &script_path,
            format!(
                r#"#!/bin/sh
set -eu
OUTPUT=""
PREV=""
for ARG in "$@"; do
  if [ "$PREV" = "o" ]; then
    OUTPUT="$ARG"
    PREV=""
    continue
  fi
  if [ "$ARG" = "-o" ]; then
    PREV="o"
  fi
done
cat >/dev/null
nohup /bin/sh -c 'echo $$ > "{pid_file}"; trap "exit 0" TERM INT; while true; do sleep 1; done' >/dev/null 2>&1 &
for _ in 1 2 3 4 5 6 7 8 9 10; do
  [ -f "{pid_file}" ] && break
  sleep 0.05
done
if [ -n "$OUTPUT" ]; then
  printf '{{"ok":true}}\n' > "$OUTPUT"
fi
printf '%s\n' '{{"type":"thread.started","thread_id":"session-123"}}'
"#,
                pid_file = pid_file.display(),
            ),
        )?;
        let mut permissions = fs::metadata(&script_path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions)?;

        let store = FileArtifactStore::new(temp.path().join(".loopsmith-runs"));
        let layout = store.initialize(Uuid::new_v4())?;
        let context = WorkerContext {
            run_id: Uuid::nil(),
            workspace,
            layout: layout.clone(),
            planner_prompt: prompts_dir.join("planner.md"),
            builder_prompt: prompts_dir.join("builder.md"),
            evaluator_prompt: prompts_dir.join("evaluator.md"),
            planner_schema: schemas_dir.join("planner.json"),
            builder_schema: schemas_dir.join("builder.json"),
            qa_schema: schemas_dir.join("qa.json"),
        };
        let artifacts = layout.stage_artifacts(WorkerStage::Plan, 1);
        let worker = CodexCliWorker::new(CodexWorkerConfig {
            binary: script_path.display().to_string(),
            model: "gpt-5.4".to_string(),
            sandbox: "workspace-write".to_string(),
            full_auto: true,
            skip_git_repo_check: true,
            resume_sessions: true,
        });

        let result = worker
            .plan(
                &context,
                &artifacts,
                &PlanningRequest {
                    user_request: "Build a harness".to_string(),
                    feature_limit: 1,
                    feature_limit_is_hard: false,
                    service_names: Vec::new(),
                    verification_commands: Vec::new(),
                },
            )
            .await?;
        assert_eq!(result.session_id.as_deref(), Some("session-123"));

        let mut descendant_pid = None;
        for _ in 0..20 {
            if pid_file.exists() {
                descendant_pid = Some(fs::read_to_string(&pid_file)?.trim().parse::<u32>()?);
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let descendant_pid = descendant_pid.expect("expected descendant pid file");
        let mut guard = PidGuard(Some(descendant_pid));

        let mut exited = false;
        for _ in 0..20 {
            let status = StdCommand::new("kill")
                .arg("-0")
                .arg(descendant_pid.to_string())
                .stdout(StdStdio::null())
                .stderr(StdStdio::null())
                .status()?;
            if !status.success() {
                exited = true;
                guard.0 = None;
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }

        assert!(
            exited,
            "expected codex worker descendant process {descendant_pid} to exit during cleanup"
        );

        Ok(())
    }
}
