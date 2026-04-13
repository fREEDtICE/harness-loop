use std::{
    io,
    path::Path,
    process::Stdio,
    sync::{Arc, Mutex},
};

use agent_client_protocol::{
    Agent, Client, ClientCapabilities, ClientSideConnection, ContentBlock,
    Error as AcpError, InitializeRequest, LoadSessionRequest, NewSessionRequest,
    PermissionOptionId, PromptRequest, ProtocolVersion, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, SessionId, SessionNotification,
    SelectedPermissionOutcome, SessionUpdate, StopReason, TextContent,
};
use anyhow::{Context, Result};
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;
use tokio::time::{Duration, sleep};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{debug, info, warn};

struct LoggingClient {
    collected_text: Arc<Mutex<Vec<String>>>,
    stdout_log: Arc<Mutex<Option<std::fs::File>>>,
}

impl LoggingClient {
    fn new() -> Self {
        Self {
            collected_text: Arc::new(Mutex::new(Vec::new())),
            stdout_log: Arc::new(Mutex::new(None)),
        }
    }
}

impl Client for LoggingClient {
    fn request_permission<'life0, 'async_trait>(
        &'life0 self,
        args: RequestPermissionRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<RequestPermissionResponse, AcpError>>
                + 'async_trait,
        >,
    >
    where
        Self: 'async_trait,
        'life0: 'async_trait,
    {
        let option_id = args
            .options
            .first()
            .map(|opt| opt.option_id.clone())
            .unwrap_or_else(|| PermissionOptionId::from("allow".to_string()));
        Box::pin(async move {
            Ok(RequestPermissionResponse::new(
                RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id)),
            ))
        })
    }

    fn session_notification<'life0, 'async_trait>(
        &'life0 self,
        args: SessionNotification,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), AcpError>> + 'async_trait>,
    >
    where
        Self: 'async_trait,
        'life0: 'async_trait,
    {
        let collected = Arc::clone(&self.collected_text);
        let log_file = Arc::clone(&self.stdout_log);
        Box::pin(async move {
            if let SessionUpdate::AgentMessageChunk(content) = &args.update {
                if let ContentBlock::Text(text) = &content.content {
                    if let Ok(mut texts) = collected.lock() {
                        texts.push(text.text.clone());
                    }
                    if let Ok(mut file_guard) = log_file.lock() {
                        if let Some(ref mut f) = *file_guard {
                            use std::io::Write;
                            let _ = write!(f, "{}", text.text);
                        }
                    }
                }
            }
            Ok(())
        })
    }
}

pub struct AcpSession {
    pub session_id: String,
    pub stop_reason: StopReason,
    pub output: String,
}

pub async fn run_acp_prompt(
    command: &[String],
    workspace: &Path,
    prompt_text: &str,
    stdout_log: &Path,
    previous_session_id: Option<&str>,
    resume_sessions: bool,
) -> Result<AcpSession> {
    let program = command
        .first()
        .context("ACP command must have at least one element")?;

    let (shell_program, shell_args) =
        loopsmith_core::shell_env::wrap_command_for_user_shell(command);

    let mut process = Command::new(&shell_program);
    process
        .args(&shell_args)
        .current_dir(workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(unix)]
    {
        process.process_group(0);
    }

    let mut child = process.spawn().with_context(|| {
        format!(
            "failed to spawn ACP agent process `{program}`. Is the agent binary installed?"
        )
    })?;
    let child_pid = child.id();

    let child_stdin = child.stdin.take().context("failed to get agent stdin")?;
    let child_stdout = child.stdout.take().context("failed to get agent stdout")?;
    let child_stderr = child.stderr.take().context("failed to get agent stderr")?;

    tokio::spawn(async move {
        let reader = tokio::io::BufReader::new(child_stderr);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            debug!(target: "acp_agent_stderr", "{}", line);
        }
    });

    info!(
        pid = child_pid.unwrap_or(0),
        command = command.join(" "),
        "spawned ACP agent process"
    );

    let logging_client = Arc::new(LoggingClient::new());

    let log_file = std::fs::File::create(stdout_log)
        .with_context(|| format!("failed to create stdout log {}", stdout_log.display()))?;
    if let Ok(mut guard) = logging_client.stdout_log.lock() {
        *guard = Some(log_file);
    }

    let client_clone = Arc::clone(&logging_client);
    let compat_stdin = child_stdin.compat_write();
    let compat_stdout = child_stdout.compat();

    let prompt_owned = prompt_text.to_string();
    let prev_session_owned = previous_session_id.map(str::to_string);
    let workspace_abs = workspace.canonicalize().unwrap_or_else(|_| workspace.to_path_buf());

    let result = tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build ACP tokio runtime");

        let local_set = tokio::task::LocalSet::new();
        local_set.block_on(&rt, async {
            let (connection, io_future) = ClientSideConnection::new(
                client_clone,
                compat_stdin,
                compat_stdout,
                |future| {
                    tokio::task::spawn_local(future);
                },
            );

            tokio::task::spawn_local(async move {
                if let Err(e) = io_future.await {
                    warn!(error = %e, "ACP IO task ended with error");
                }
            });

            connection
                .initialize(
                    InitializeRequest::new(ProtocolVersion::LATEST)
                        .client_capabilities(ClientCapabilities::new().terminal(true)),
                )
                .await
                .map_err(|e| anyhow::anyhow!("ACP initialize failed: {e}"))?;

            info!("ACP agent initialized");

            let session_id = if let Some(prev_id) = prev_session_owned.filter(|_| resume_sessions)
            {
                let sid = SessionId::from(prev_id.clone());
                match connection
                    .load_session(
                        LoadSessionRequest::new(sid.clone(), workspace_abs.clone())
                            .mcp_servers(Vec::new()),
                    )
                    .await
                {
                    Ok(_resp) => {
                        info!(session_id = %sid, "ACP session loaded");
                        sid
                    }
                    Err(e) => {
                        warn!(error = %e, "failed to load ACP session, creating new one");
                        let resp = connection
                            .new_session(
                                NewSessionRequest::new(workspace_abs.clone())
                                    .mcp_servers(Vec::new()),
                            )
                            .await
                            .map_err(|e| anyhow::anyhow!("ACP session/new failed: {e}"))?;
                        info!(session_id = %resp.session_id, "ACP session created");
                        resp.session_id
                    }
                }
            } else {
                let resp = connection
                    .new_session(
                        NewSessionRequest::new(workspace_abs.clone()).mcp_servers(Vec::new()),
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("ACP session/new failed: {e}"))?;
                info!(session_id = %resp.session_id, "ACP session created");
                resp.session_id
            };

            let response = connection
                .prompt(PromptRequest::new(
                    session_id.clone(),
                    vec![ContentBlock::Text(TextContent::new(prompt_owned))],
                ))
                .await
                .map_err(|e| anyhow::anyhow!("ACP session/prompt failed: {e}"))?;

            Ok::<(SessionId, StopReason), anyhow::Error>((session_id, response.stop_reason))
        })
    })
    .await
    .map_err(|e| anyhow::anyhow!("ACP task panicked: {e}"))??;

    let (session_id, stop_reason) = result;

    let mut output = String::new();
    if let Ok(texts) = logging_client.collected_text.lock() {
        for t in texts.iter() {
            output.push_str(t);
        }
    }

    #[cfg(unix)]
    {
        // Give the agent a brief chance to exit on its own before using killpg.
        // This reduces races where the process group is already winding down.
        match child.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) => {
                let _ = tokio::time::timeout(Duration::from_millis(500), child.wait()).await;
            }
            Err(error) => {
                warn!(error = %error, "failed to poll ACP agent exit status before cleanup");
            }
        }

        let cleanup_result = cleanup_process_group(child_pid).await;
        match &cleanup_result {
            Ok(true) => info!("cleaned up lingering ACP agent descendant processes"),
            Ok(false) => {}
            Err(error) => warn!(
                error = %error,
                "failed to clean up ACP agent descendant processes"
            ),
        }
    }

    Ok(AcpSession {
        session_id: session_id.to_string(),
        stop_reason,
        output,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_notification_accepts_usage_updates() {
        let notification: SessionNotification = serde_json::from_str(
            r#"{
                "sessionId":"sess_123",
                "update":{
                    "sessionUpdate":"usage_update",
                    "used":54549,
                    "size":258400
                }
            }"#,
        )
        .expect("usage_update notification should deserialize");

        match notification.update {
            SessionUpdate::UsageUpdate(update) => {
                assert_eq!(update.used, 54_549);
                assert_eq!(update.size, 258_400);
            }
            other => panic!("expected usage update, got {other:?}"),
        }
    }
}

#[cfg(unix)]
pub(crate) async fn cleanup_process_group(pid: Option<u32>) -> Result<bool> {
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

    anyhow::bail!("ACP agent process group {pid} remained alive after cleanup")
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
        _ => Err(err).with_context(|| format!("failed to probe ACP agent process group {pid}")),
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
    let raw_os_error = err.raw_os_error();
    if err.raw_os_error() == Some(libc::ESRCH) {
        return Ok(());
    }

    Err(err).with_context(|| {
        format!(
            "failed to deliver signal {signal} to ACP agent process group {pid} (os_error={:?})",
            raw_os_error
        )
    })
}
