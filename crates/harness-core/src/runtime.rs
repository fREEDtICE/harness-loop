use std::{
    fs::{self, File},
    io,
    net::{TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::{
    process::{Child, Command as TokioCommand},
    time::{sleep, timeout},
};

use crate::config::{RuntimeConfig, ScreenshotConfig, ServiceConfig, StackConfig};
use crate::domain::{
    ScreenshotCommandResult, ScreenshotEvidence, ScreenshotStatus, VerificationCommandResult,
    VerificationEvidence, VerificationStatus,
};
use crate::paths::normalize_path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimePlan {
    pub workspace: PathBuf,
    pub stacks: Vec<StackPlan>,
    pub services: Vec<ServicePlan>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackPlan {
    pub name: String,
    pub up: Vec<String>,
    pub down: Vec<String>,
    pub working_dir: PathBuf,
    pub readiness: ServiceReadiness,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServicePlan {
    pub name: String,
    pub start: Vec<String>,
    pub working_dir: PathBuf,
    pub readiness: ServiceReadiness,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceReadiness {
    pub url: Option<String>,
    pub command: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeServiceStatus {
    Starting,
    Ready,
    Failed,
    Stopped,
}

impl RuntimeServiceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeServiceRecord {
    pub name: String,
    pub command: Vec<String>,
    pub working_dir: PathBuf,
    pub ready_url: Option<String>,
    pub ready_command: Option<Vec<String>>,
    pub status: RuntimeServiceStatus,
    pub pid: Option<u32>,
    pub stdout_log: PathBuf,
    pub stderr_log: PathBuf,
    pub record_file: PathBuf,
    pub started_at: DateTime<Utc>,
    pub ready_at: Option<DateTime<Utc>>,
    pub stopped_at: Option<DateTime<Utc>>,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStackRecord {
    pub name: String,
    pub up_command: Vec<String>,
    pub down_command: Vec<String>,
    pub working_dir: PathBuf,
    pub ready_url: Option<String>,
    pub ready_command: Option<Vec<String>>,
    pub status: RuntimeServiceStatus,
    pub up_stdout_log: PathBuf,
    pub up_stderr_log: PathBuf,
    pub down_stdout_log: PathBuf,
    pub down_stderr_log: PathBuf,
    pub record_file: PathBuf,
    pub started_at: DateTime<Utc>,
    pub ready_at: Option<DateTime<Utc>>,
    pub stopped_at: Option<DateTime<Utc>>,
    pub up_exit_code: Option<i32>,
    pub down_exit_code: Option<i32>,
}

pub struct RuntimeSupervisor {
    managed_stacks: Vec<ManagedStack>,
    managed_services: Vec<ManagedService>,
    shutdown_grace_period: Duration,
}

struct ManagedStack {
    plan: StackPlan,
    record: RuntimeStackRecord,
}

struct ManagedService {
    child: Child,
    record: RuntimeServiceRecord,
}

impl RuntimePlan {
    pub fn from_config(workspace: &Path, config: &RuntimeConfig) -> Self {
        let stacks = config
            .stacks
            .iter()
            .map(|stack| map_stack(workspace, stack))
            .collect();
        let services = config
            .services
            .iter()
            .map(|service| map_service(workspace, service))
            .collect();

        Self {
            workspace: workspace.to_path_buf(),
            stacks,
            services,
            notes: vec![
                "Run services outside the worker so restarts do not erase app state.".to_string(),
                "Capture logs, screenshots, and QA artifacts alongside each run.".to_string(),
                "Command-driven stack orchestration can manage Docker Compose or similar runtime groups.".to_string(),
            ],
        }
    }
}

impl RuntimeSupervisor {
    pub async fn start_if_enabled(
        config: &RuntimeConfig,
        plan: &RuntimePlan,
        run_root: &Path,
    ) -> Result<Self> {
        if !config.supervision.enabled || (plan.services.is_empty() && plan.stacks.is_empty()) {
            return Ok(Self {
                managed_stacks: Vec::new(),
                managed_services: Vec::new(),
                shutdown_grace_period: Duration::from_secs(
                    config.supervision.shutdown_grace_period_secs,
                ),
            });
        }

        let services_root = run_root.join("runtime").join("services");
        fs::create_dir_all(&services_root)
            .with_context(|| format!("failed to create directory {}", services_root.display()))?;

        let startup_timeout = Duration::from_secs(config.supervision.startup_timeout_secs);
        let readiness_poll =
            Duration::from_millis(config.supervision.readiness_poll_interval_ms.max(1));
        let shutdown_grace_period =
            Duration::from_secs(config.supervision.shutdown_grace_period_secs);
        let stacks_root = run_root.join("runtime").join("stacks");
        fs::create_dir_all(&stacks_root)
            .with_context(|| format!("failed to create directory {}", stacks_root.display()))?;

        let mut supervisor = Self {
            managed_stacks: Vec::new(),
            managed_services: Vec::new(),
            shutdown_grace_period,
        };

        for stack in &plan.stacks {
            let managed = start_stack(stack, &stacks_root)?;
            supervisor.managed_stacks.push(managed);

            let index = supervisor.managed_stacks.len() - 1;
            if let Err(err) = wait_for_stack_ready(
                &mut supervisor.managed_stacks[index],
                startup_timeout,
                readiness_poll,
            )
            .await
            {
                supervisor.managed_stacks[index].record.status = RuntimeServiceStatus::Failed;
                write_stack_record(&supervisor.managed_stacks[index].record)?;
                let _ = supervisor.shutdown().await;
                return Err(err.context(format!("stack `{}` failed readiness checks", stack.name)));
            }
        }

        for service in &plan.services {
            let managed = spawn_service(service, &services_root)?;
            supervisor.managed_services.push(managed);

            let index = supervisor.managed_services.len() - 1;
            if let Err(err) = wait_for_service_ready(
                &mut supervisor.managed_services[index],
                startup_timeout,
                readiness_poll,
            )
            .await
            {
                supervisor.managed_services[index].record.status = RuntimeServiceStatus::Failed;
                write_service_record(&supervisor.managed_services[index].record)?;
                let _ = supervisor.shutdown().await;
                return Err(err.context(format!(
                    "service `{}` failed readiness checks",
                    service.name
                )));
            }
        }

        Ok(supervisor)
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        let mut errors = Vec::new();

        for managed in self.managed_services.iter_mut().rev() {
            if let Err(err) = shutdown_service(managed, self.shutdown_grace_period).await {
                errors.push(format!("{}: {err:#}", managed.record.name));
            }
        }

        for managed in self.managed_stacks.iter_mut().rev() {
            if let Err(err) = shutdown_stack(managed).await {
                errors.push(format!("{}: {err:#}", managed.record.name));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            bail!("runtime shutdown encountered errors: {}", errors.join("; "))
        }
    }
}

fn map_stack(workspace: &Path, stack: &StackConfig) -> StackPlan {
    let working_dir = stack
        .working_dir
        .as_ref()
        .map(|dir| {
            if dir.is_absolute() {
                normalize_path(dir.clone())
            } else {
                normalize_path(workspace.join(dir))
            }
        })
        .unwrap_or_else(|| normalize_path(workspace.to_path_buf()));

    StackPlan {
        name: stack.name.clone(),
        up: stack.up.clone(),
        down: stack.down.clone(),
        working_dir,
        readiness: ServiceReadiness {
            url: stack.ready_url.clone(),
            command: stack.ready_command.clone(),
        },
    }
}

fn map_service(workspace: &Path, service: &ServiceConfig) -> ServicePlan {
    let working_dir = service
        .working_dir
        .as_ref()
        .map(|dir| {
            if dir.is_absolute() {
                normalize_path(dir.clone())
            } else {
                normalize_path(workspace.join(dir))
            }
        })
        .unwrap_or_else(|| normalize_path(workspace.to_path_buf()));

    ServicePlan {
        name: service.name.clone(),
        start: service.start.clone(),
        working_dir,
        readiness: ServiceReadiness {
            url: service.ready_url.clone(),
            command: service.ready_command.clone(),
        },
    }
}

fn start_stack(stack: &StackPlan, stacks_root: &Path) -> Result<ManagedStack> {
    let stack_root = stacks_root.join(sanitize_for_path(&stack.name));
    fs::create_dir_all(&stack_root)
        .with_context(|| format!("failed to create directory {}", stack_root.display()))?;

    let up_stdout_log = stack_root.join("up.stdout.log");
    let up_stderr_log = stack_root.join("up.stderr.log");
    let down_stdout_log = stack_root.join("down.stdout.log");
    let down_stderr_log = stack_root.join("down.stderr.log");
    let record_file = stack_root.join("stack.json");

    let mut record = RuntimeStackRecord {
        name: stack.name.clone(),
        up_command: stack.up.clone(),
        down_command: stack.down.clone(),
        working_dir: stack.working_dir.clone(),
        ready_url: stack.readiness.url.clone(),
        ready_command: stack.readiness.command.clone(),
        status: RuntimeServiceStatus::Starting,
        up_stdout_log: up_stdout_log.clone(),
        up_stderr_log: up_stderr_log.clone(),
        down_stdout_log,
        down_stderr_log,
        record_file,
        started_at: Utc::now(),
        ready_at: None,
        stopped_at: None,
        up_exit_code: None,
        down_exit_code: None,
    };
    write_stack_record(&record)?;

    let output = run_command_capture(&stack.up, &stack.working_dir)
        .with_context(|| format!("failed to execute stack `{}` up command", stack.name))?;
    fs::write(&up_stdout_log, &output.stdout)
        .with_context(|| format!("failed to write {}", up_stdout_log.display()))?;
    fs::write(&up_stderr_log, &output.stderr)
        .with_context(|| format!("failed to write {}", up_stderr_log.display()))?;
    record.up_exit_code = output.status.code();

    if !output.status.success() {
        record.status = RuntimeServiceStatus::Failed;
        record.stopped_at = Some(Utc::now());
        write_stack_record(&record)?;
        bail!(
            "stack `{}` up command failed with status {:?}",
            stack.name,
            output.status.code()
        );
    }

    write_stack_record(&record)?;
    Ok(ManagedStack {
        plan: stack.clone(),
        record,
    })
}

fn spawn_service(service: &ServicePlan, services_root: &Path) -> Result<ManagedService> {
    let Some(program) = service.start.first() else {
        bail!("service `{}` has an empty start command", service.name);
    };

    let service_root = services_root.join(sanitize_for_path(&service.name));
    fs::create_dir_all(&service_root)
        .with_context(|| format!("failed to create directory {}", service_root.display()))?;

    let stdout_log = service_root.join("stdout.log");
    let stderr_log = service_root.join("stderr.log");
    let record_file = service_root.join("service.json");

    let stdout = File::create(&stdout_log)
        .with_context(|| format!("failed to create {}", stdout_log.display()))?;
    let stderr = File::create(&stderr_log)
        .with_context(|| format!("failed to create {}", stderr_log.display()))?;

    let mut command = TokioCommand::new(program);
    command
        .args(service.start.iter().skip(1))
        .current_dir(&service.working_dir)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));

    #[cfg(unix)]
    {
        // Isolate each supervised service in its own process group so shutdown
        // can reliably terminate shell wrappers and any descendants they spawned.
        command.process_group(0);
    }

    let child = command.spawn().with_context(|| {
        format!(
            "failed to spawn service `{}` with command `{}`",
            service.name, program
        )
    })?;

    let record = RuntimeServiceRecord {
        name: service.name.clone(),
        command: service.start.clone(),
        working_dir: service.working_dir.clone(),
        ready_url: service.readiness.url.clone(),
        ready_command: service.readiness.command.clone(),
        status: RuntimeServiceStatus::Starting,
        pid: child.id(),
        stdout_log,
        stderr_log,
        record_file,
        started_at: Utc::now(),
        ready_at: None,
        stopped_at: None,
        exit_code: None,
    };
    write_service_record(&record)?;

    Ok(ManagedService { child, record })
}

async fn wait_for_stack_ready(
    managed: &mut ManagedStack,
    startup_timeout: Duration,
    readiness_poll: Duration,
) -> Result<()> {
    if managed.record.ready_url.is_none() && managed.record.ready_command.is_none() {
        managed.record.status = RuntimeServiceStatus::Ready;
        managed.record.ready_at = Some(Utc::now());
        write_stack_record(&managed.record)?;
        return Ok(());
    }

    let started = Instant::now();
    loop {
        let ready = if let Some(command) = &managed.record.ready_command {
            probe_command(command, &managed.record.working_dir)?
        } else if let Some(url) = &managed.record.ready_url {
            probe_url(url)?
        } else {
            true
        };

        if ready {
            managed.record.status = RuntimeServiceStatus::Ready;
            managed.record.ready_at = Some(Utc::now());
            write_stack_record(&managed.record)?;
            return Ok(());
        }

        if started.elapsed() >= startup_timeout {
            bail!(
                "stack `{}` did not become ready within {:?}",
                managed.record.name,
                startup_timeout
            );
        }

        sleep(readiness_poll).await;
    }
}

async fn wait_for_service_ready(
    managed: &mut ManagedService,
    startup_timeout: Duration,
    readiness_poll: Duration,
) -> Result<()> {
    if managed.record.ready_url.is_none() && managed.record.ready_command.is_none() {
        managed.record.status = RuntimeServiceStatus::Ready;
        managed.record.ready_at = Some(Utc::now());
        write_service_record(&managed.record)?;
        return Ok(());
    }

    let started = Instant::now();
    loop {
        if let Some(exit_status) = managed
            .child
            .try_wait()
            .context("failed to inspect service child process")?
        {
            managed.record.status = RuntimeServiceStatus::Failed;
            managed.record.exit_code = exit_status.code();
            managed.record.stopped_at = Some(Utc::now());
            write_service_record(&managed.record)?;
            bail!(
                "service `{}` exited before becoming ready with status {:?}",
                managed.record.name,
                exit_status.code()
            );
        }

        let ready = if let Some(command) = &managed.record.ready_command {
            probe_command(command, &managed.record.working_dir)?
        } else if let Some(url) = &managed.record.ready_url {
            probe_url(url)?
        } else {
            true
        };

        if ready {
            managed.record.status = RuntimeServiceStatus::Ready;
            managed.record.ready_at = Some(Utc::now());
            write_service_record(&managed.record)?;
            return Ok(());
        }

        if started.elapsed() >= startup_timeout {
            bail!(
                "service `{}` did not become ready within {:?}",
                managed.record.name,
                startup_timeout
            );
        }

        sleep(readiness_poll).await;
    }
}

async fn shutdown_service(
    managed: &mut ManagedService,
    shutdown_grace_period: Duration,
) -> Result<()> {
    if managed.record.status == RuntimeServiceStatus::Stopped {
        return Ok(());
    }

    if let Some(exit_status) = managed
        .child
        .try_wait()
        .context("failed to inspect service child process during shutdown")?
    {
        managed.record.status = RuntimeServiceStatus::Stopped;
        managed.record.exit_code = exit_status.code();
        managed.record.stopped_at = Some(Utc::now());
        write_service_record(&managed.record)?;
        return Ok(());
    }

    send_terminate_signal(managed.child.id())?;

    match timeout(shutdown_grace_period, managed.child.wait()).await {
        Ok(wait_result) => {
            let exit_status = wait_result.context("failed while waiting for service shutdown")?;
            managed.record.status = RuntimeServiceStatus::Stopped;
            managed.record.exit_code = exit_status.code();
        }
        Err(_) => {
            force_kill_service(managed).await?;
            let exit_status = managed
                .child
                .wait()
                .await
                .context("failed while waiting for force-killed service")?;
            managed.record.status = RuntimeServiceStatus::Stopped;
            managed.record.exit_code = exit_status.code();
        }
    }

    managed.record.stopped_at = Some(Utc::now());
    write_service_record(&managed.record)?;
    Ok(())
}

async fn shutdown_stack(managed: &mut ManagedStack) -> Result<()> {
    if managed.record.status == RuntimeServiceStatus::Stopped {
        return Ok(());
    }

    let output =
        run_command_capture(&managed.plan.down, &managed.plan.working_dir).with_context(|| {
            format!(
                "failed to execute stack `{}` down command",
                managed.record.name
            )
        })?;
    fs::write(&managed.record.down_stdout_log, &output.stdout).with_context(|| {
        format!(
            "failed to write {}",
            managed.record.down_stdout_log.display()
        )
    })?;
    fs::write(&managed.record.down_stderr_log, &output.stderr).with_context(|| {
        format!(
            "failed to write {}",
            managed.record.down_stderr_log.display()
        )
    })?;
    managed.record.down_exit_code = output.status.code();
    managed.record.stopped_at = Some(Utc::now());

    if output.status.success() {
        managed.record.status = RuntimeServiceStatus::Stopped;
        write_stack_record(&managed.record)?;
        Ok(())
    } else {
        managed.record.status = RuntimeServiceStatus::Failed;
        write_stack_record(&managed.record)?;
        bail!(
            "stack `{}` down command failed with status {:?}",
            managed.record.name,
            output.status.code()
        );
    }
}

fn send_terminate_signal(pid: Option<u32>) -> Result<()> {
    #[cfg(unix)]
    {
        send_signal(pid, libc::SIGTERM)
    }

    #[cfg(not(unix))]
    {
        let _ = pid;
        Ok(())
    }
}

async fn force_kill_service(managed: &mut ManagedService) -> Result<()> {
    #[cfg(unix)]
    {
        send_signal(managed.child.id(), libc::SIGKILL)
    }

    #[cfg(not(unix))]
    {
        managed
            .child
            .kill()
            .await
            .context("failed to force-kill service process")
    }
}

#[cfg(unix)]
fn send_signal(pid: Option<u32>, signal: libc::c_int) -> Result<()> {
    let Some(pid) = pid else {
        return Ok(());
    };

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
        format!("failed to deliver signal {signal} to supervised service process group {pid}")
    })
}

fn probe_command(command: &[String], working_dir: &Path) -> Result<bool> {
    let Some(program) = command.first() else {
        bail!("readiness command was empty");
    };

    let output = run_command_capture(command, working_dir)
        .with_context(|| format!("failed to execute readiness command `{program}`"))?;

    Ok(output.status.success())
}

fn probe_url(url: &str) -> Result<bool> {
    let (host, port) = parse_host_port(url)?;
    let addresses = (host.as_str(), port)
        .to_socket_addrs()
        .with_context(|| format!("failed to resolve readiness url `{url}`"))?;

    for address in addresses {
        if TcpStream::connect_timeout(&address, Duration::from_millis(250)).is_ok() {
            return Ok(true);
        }
    }

    Ok(false)
}

fn parse_host_port(url: &str) -> Result<(String, u16)> {
    let (default_port, rest) = if let Some(rest) = url.strip_prefix("http://") {
        (80, rest)
    } else if let Some(rest) = url.strip_prefix("https://") {
        (443, rest)
    } else {
        bail!("unsupported readiness url `{url}`");
    };

    let authority = rest.split('/').next().unwrap_or(rest);
    if authority.is_empty() {
        bail!("readiness url `{url}` is missing a host");
    }

    if let Some(host) = authority.strip_prefix('[') {
        let Some((ipv6_host, remainder)) = host.split_once(']') else {
            bail!("invalid ipv6 readiness url `{url}`");
        };

        let port = if let Some(port) = remainder.strip_prefix(':') {
            port.parse()
                .with_context(|| format!("invalid port in readiness url `{url}`"))?
        } else {
            default_port
        };

        return Ok((ipv6_host.to_string(), port));
    }

    if let Some((host, port)) = authority.rsplit_once(':') {
        if !host.is_empty() && port.chars().all(|ch| ch.is_ascii_digit()) {
            return Ok((
                host.to_string(),
                port.parse()
                    .with_context(|| format!("invalid port in readiness url `{url}`"))?,
            ));
        }
    }

    Ok((authority.to_string(), default_port))
}

fn write_service_record(record: &RuntimeServiceRecord) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(record).context("failed to serialize service record")?;
    fs::write(&record.record_file, bytes)
        .with_context(|| format!("failed to write {}", record.record_file.display()))
}

fn write_stack_record(record: &RuntimeStackRecord) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(record).context("failed to serialize stack record")?;
    fs::write(&record.record_file, bytes)
        .with_context(|| format!("failed to write {}", record.record_file.display()))
}

fn run_command_capture(command: &[String], working_dir: &Path) -> Result<std::process::Output> {
    let Some(program) = command.first() else {
        bail!("command was empty");
    };

    Command::new(program)
        .args(command.iter().skip(1))
        .current_dir(working_dir)
        .output()
        .with_context(|| format!("failed to execute command `{program}`"))
}

fn sanitize_for_path(value: &str) -> String {
    let mut sanitized = String::with_capacity(value.len());

    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            sanitized.push(ch.to_ascii_lowercase());
        } else if !sanitized.ends_with('-') {
            sanitized.push('-');
        }
    }

    let trimmed = sanitized.trim_matches('-');
    if trimmed.is_empty() {
        "service".to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn run_verification_commands(
    workspace: &Path,
    evidence_root: &Path,
    attempt: usize,
    commands: &[Vec<String>],
) -> Result<VerificationEvidence> {
    fs::create_dir_all(evidence_root)
        .with_context(|| format!("failed to create directory {}", evidence_root.display()))?;

    let mut results = Vec::with_capacity(commands.len());

    for (index, command) in commands.iter().enumerate() {
        let Some(program) = command.first() else {
            bail!("verification command {} was empty", index + 1);
        };

        let name = format!("check-{:02}", index + 1);
        let stdout_log = evidence_root.join(format!("{name}.stdout.log"));
        let stderr_log = evidence_root.join(format!("{name}.stderr.log"));

        let output = Command::new(program)
            .args(command.iter().skip(1))
            .current_dir(workspace)
            .output()
            .with_context(|| format!("failed to execute verification command `{program}`"))?;

        fs::write(&stdout_log, &output.stdout)
            .with_context(|| format!("failed to write {}", stdout_log.display()))?;
        fs::write(&stderr_log, &output.stderr)
            .with_context(|| format!("failed to write {}", stderr_log.display()))?;

        let status = if output.status.success() {
            VerificationStatus::Passed
        } else {
            VerificationStatus::Failed
        };

        results.push(VerificationCommandResult {
            name,
            command: command.clone(),
            working_dir: workspace.to_path_buf(),
            status,
            exit_code: output.status.code(),
            stdout_log,
            stderr_log,
        });
    }

    Ok(VerificationEvidence {
        attempt,
        report_file: evidence_root.join("report.json"),
        results,
    })
}

pub fn run_screenshot_commands(
    workspace: &Path,
    evidence_root: &Path,
    attempt: usize,
    commands: &[ScreenshotConfig],
) -> Result<ScreenshotEvidence> {
    fs::create_dir_all(evidence_root)
        .with_context(|| format!("failed to create directory {}", evidence_root.display()))?;

    let mut results = Vec::with_capacity(commands.len());

    for (index, screenshot) in commands.iter().enumerate() {
        let Some(program) = screenshot.command.first() else {
            bail!("screenshot command {} was empty", index + 1);
        };

        let stem = format!(
            "shot-{:02}-{}",
            index + 1,
            sanitize_for_path(&screenshot.name)
        );
        let output_file = evidence_root.join(format!("{stem}.png"));
        let stdout_log = evidence_root.join(format!("{stem}.stdout.log"));
        let stderr_log = evidence_root.join(format!("{stem}.stderr.log"));
        let output_path = output_file.as_os_str().to_string_lossy().into_owned();
        let materialized_command = screenshot
            .command
            .iter()
            .map(|arg| arg.replace("{output}", &output_path))
            .collect::<Vec<_>>();

        let output = Command::new(program)
            .args(materialized_command.iter().skip(1))
            .env("CODEX_HARNESS_SCREENSHOT_OUTPUT", &output_file)
            .env("CODEX_HARNESS_SCREENSHOT_DIR", evidence_root)
            .env("CODEX_HARNESS_SCREENSHOT_NAME", &screenshot.name)
            .env("CODEX_HARNESS_ATTEMPT", attempt.to_string())
            .current_dir(workspace)
            .output()
            .with_context(|| format!("failed to execute screenshot command `{program}`"))?;

        fs::write(&stdout_log, &output.stdout)
            .with_context(|| format!("failed to write {}", stdout_log.display()))?;
        fs::write(&stderr_log, &output.stderr)
            .with_context(|| format!("failed to write {}", stderr_log.display()))?;

        let bytes = fs::metadata(&output_file)
            .ok()
            .map(|metadata| metadata.len());
        let status = if output.status.success() && bytes.unwrap_or(0) > 0 {
            ScreenshotStatus::Captured
        } else {
            ScreenshotStatus::Failed
        };

        results.push(ScreenshotCommandResult {
            name: screenshot.name.clone(),
            command: materialized_command,
            working_dir: workspace.to_path_buf(),
            output_file,
            status,
            exit_code: output.status.code(),
            bytes,
            stdout_log,
            stderr_log,
        });
    }

    Ok(ScreenshotEvidence {
        attempt,
        report_file: evidence_root.join("report.json"),
        results,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::{
        config::{
            RuntimeConfig, RuntimeSupervisionConfig, ScreenshotConfig, ServiceConfig, StackConfig,
        },
        domain::{ScreenshotStatus, VerificationStatus},
    };

    use super::{
        RuntimePlan, RuntimeServiceStatus, RuntimeSupervisor, parse_host_port,
        run_screenshot_commands, run_verification_commands,
    };

    #[test]
    fn verification_commands_capture_pass_and_fail_results() {
        let temp = tempdir().expect("tempdir");
        let evidence_root = temp.path().join("runtime/verification/evaluate-01");
        let evidence = run_verification_commands(
            temp.path(),
            &evidence_root,
            1,
            &[
                vec![
                    "/usr/bin/env".to_string(),
                    "printf".to_string(),
                    "ok\n".to_string(),
                ],
                vec!["/usr/bin/env".to_string(), "false".to_string()],
            ],
        )
        .expect("verification evidence");

        assert_eq!(evidence.results.len(), 2);
        assert_eq!(evidence.results[0].status, VerificationStatus::Passed);
        assert_eq!(evidence.results[1].status, VerificationStatus::Failed);
        assert!(evidence.results[0].stdout_log.exists());
        assert!(evidence.results[1].stderr_log.exists());
    }

    #[test]
    fn screenshot_commands_capture_output_files() {
        let temp = tempdir().expect("tempdir");
        let evidence_root = temp.path().join("runtime/screenshots/evaluate-01");
        let evidence = run_screenshot_commands(
            temp.path(),
            &evidence_root,
            1,
            &[ScreenshotConfig {
                name: "home".to_string(),
                command: vec![
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    "printf '\\211PNG\\r\\n\\032\\n' > \"$CODEX_HARNESS_SCREENSHOT_OUTPUT\""
                        .to_string(),
                ],
            }],
        )
        .expect("screenshot evidence");

        assert_eq!(evidence.results.len(), 1);
        assert_eq!(evidence.results[0].status, ScreenshotStatus::Captured);
        assert_eq!(evidence.results[0].bytes, Some(8));
        assert!(evidence.results[0].output_file.exists());
    }

    #[test]
    fn readiness_url_parser_handles_hosts_and_ports() {
        assert_eq!(
            parse_host_port("http://127.0.0.1:3000/health").expect("http url"),
            ("127.0.0.1".to_string(), 3000)
        );
        assert_eq!(
            parse_host_port("https://example.com").expect("https url"),
            ("example.com".to_string(), 443)
        );
        assert_eq!(
            parse_host_port("http://[::1]:8080/").expect("ipv6 url"),
            ("::1".to_string(), 8080)
        );
    }

    #[tokio::test]
    async fn runtime_supervisor_starts_and_stops_ready_command_services() {
        let temp = tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        let run_root = temp.path().join("run");
        fs::create_dir_all(&workspace).expect("workspace");
        fs::create_dir_all(&run_root).expect("run root");

        let ready_file = workspace.join("ready.txt");
        let service = ServiceConfig {
            name: "web".to_string(),
            start: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                format!(
                    "printf 'booted\\n'; touch '{}'; trap 'exit 0' TERM INT; while true; do sleep 1; done",
                    ready_file.display()
                ),
            ],
            working_dir: Some(workspace.clone()),
            ready_url: None,
            ready_command: Some(vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                format!("test -f '{}'", ready_file.display()),
            ]),
        };
        let config = RuntimeConfig {
            feature_limit: 1,
            max_repair_attempts: 1,
            continue_after_failure: false,
            confirm_before_build: false,
            supervision: RuntimeSupervisionConfig {
                enabled: true,
                startup_timeout_secs: 5,
                readiness_poll_interval_ms: 100,
                shutdown_grace_period_secs: 1,
            },
            services: vec![service],
            stacks: Vec::<StackConfig>::new(),
        };
        let plan = RuntimePlan::from_config(&workspace, &config);

        let mut supervisor = RuntimeSupervisor::start_if_enabled(&config, &plan, &run_root)
            .await
            .expect("runtime supervisor");
        let record_path = run_root.join("runtime/services/web/service.json");
        let ready_record: serde_json::Value =
            serde_json::from_slice(&fs::read(&record_path).expect("record file"))
                .expect("record json");
        assert_eq!(ready_record["status"], "ready");
        assert_eq!(ready_record["name"], "web");
        assert!(run_root.join("runtime/services/web/stdout.log").exists());

        supervisor.shutdown().await.expect("shutdown supervisor");
        let stopped_record: serde_json::Value =
            serde_json::from_slice(&fs::read(&record_path).expect("record file after shutdown"))
                .expect("record json after shutdown");
        assert_eq!(
            stopped_record["status"],
            RuntimeServiceStatus::Stopped.as_str()
        );
        assert!(stopped_record["stopped_at"].is_string());
    }
}
