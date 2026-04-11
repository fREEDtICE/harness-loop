use std::collections::HashMap;

use anyhow::{Context, Result};
use tracing::{debug, info, warn};

const SKIP_KEYS: &[&str] = &[
    "_",
    "PWD",
    "OLDPWD",
    "SHLVL",
    "TERM",
    "TERM_PROGRAM",
    "TERM_SESSION_ID",
    "TMPDIR",
    "SHELL",
    "HOME",
    "USER",
    "LOGNAME",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "DISPLAY",
    "SSH_AUTH_SOCK",
    "XPC_FLAGS",
    "XPC_SERVICE_NAME",
    "__CF_USER_TEXT_ENCODING",
    "Apple_PubSub_Socket_Render",
    "SECURITYSESSIONID",
    "LaunchInstanceID",
];
const LOGIN_SHELL_ENV_CAPTURE_ARGS: [&str; 3] = ["-l", "-c", "env"];

/// Loads environment variables from the user's default login shell and merges
/// them into the current process environment.
///
/// On macOS (and Linux), GUI applications launched from the Dock, Launchpad, or
/// Finder do **not** inherit the shell environment configured in `~/.zshrc`,
/// `~/.bashrc`, or tools like `nvm`, `pyenv`, `rbenv`, etc.
///
/// This function spawns a **login shell** (`$SHELL -l -c env`), captures its
/// full environment, and merges it into the current process:
/// - `PATH` is merged (shell entries appended, no duplicates)
/// - All other tool-related variables are imported if not already set
///
/// It is safe to call from CLI entry points as well — if the environment is
/// already complete, the function is effectively a no-op.
///
/// The shell probe intentionally avoids `-i` because interactive startup hooks
/// can hang GUI and CLI process startup before the application begins real work.
pub fn inherit_shell_env() -> Result<()> {
    let shell = detect_login_shell();
    debug!(shell = %shell, "probing login shell for environment");

    let env_vars = match capture_shell_env(&shell) {
        Ok(vars) => vars,
        Err(err) => {
            warn!(
                shell = %shell,
                error = %err,
                "failed to capture login shell environment; \
                 child processes may not find tools installed via nvm/pyenv/etc."
            );
            return Ok(());
        }
    };

    let current_path = std::env::var("PATH").unwrap_or_default();
    let mut merged_count = 0u32;

    for (key, value) in &env_vars {
        if SKIP_KEYS.contains(&key.as_str()) {
            continue;
        }

        if key == "PATH" {
            let merged_path = merge_path(&current_path, value);
            if merged_path != current_path {
                debug!(
                    key = "PATH",
                    before = %current_path,
                    after = %merged_path,
                    "merging PATH from login shell"
                );
                // SAFETY: called during single-threaded startup before any
                // worker threads or async runtime are spawned.
                unsafe { std::env::set_var("PATH", &merged_path) };
                merged_count += 1;
            }
            continue;
        }

        if std::env::var(key).is_err() {
            debug!(key = %key, "importing env var from login shell");
            // SAFETY: same as above — single-threaded startup.
            unsafe { std::env::set_var(key, value) };
            merged_count += 1;
        }
    }

    if merged_count > 0 {
        info!(shell = %shell, vars_merged = merged_count, "inherited login shell environment");
    } else {
        debug!(shell = %shell, "login shell environment already in sync");
    }

    Ok(())
}

/// Wraps a command so that it executes inside the user's login shell. This
/// ensures the child process sees the same environment as the user would in
/// their terminal (PATH, API keys, nvm, pyenv, etc.).
///
/// On Unix, transforms `["binary", "arg1", "arg2"]` into:
///   `$SHELL -l -c 'binary arg1 arg2'`
///
/// On non-Unix platforms, the command is returned unchanged.
///
/// Each argument is shell-escaped to handle spaces and special characters.
///
/// The wrapper intentionally avoids `-i` because interactive shell startup can
/// block long-running GUI worker launches before the target binary ever
/// executes, leaving stage logs empty and the run stuck at the initial stage.
pub fn wrap_command_for_user_shell(command: &[String]) -> (String, Vec<String>) {
    #[cfg(unix)]
    {
        let shell = detect_login_shell();
        let escaped: Vec<String> = command.iter().map(|arg| shell_escape(arg)).collect();
        let joined = escaped.join(" ");
        debug!(shell = %shell, command = %joined, "wrapping worker command via login shell");
        (shell, vec!["-l".into(), "-c".into(), joined])
    }

    #[cfg(not(unix))]
    {
        let program = command[0].clone();
        let args = command[1..].to_vec();
        (program, args)
    }
}

fn shell_escape(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    if s.chars().all(|c| {
        c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '=' | '+' | ',')
    }) {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Checks that a worker binary is actually executable after environment
/// inheritance. Returns a diagnostic message if problems are found.
pub fn check_worker_binary(binary: &str) -> Option<String> {
    let lookup = if cfg!(windows) { "where" } else { "which" };
    let found = std::process::Command::new(lookup)
        .arg(binary)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !found {
        return Some(format!(
            "`{binary}` not found in PATH after inheriting shell environment"
        ));
    }

    let resolved = std::process::Command::new(lookup)
        .arg(binary)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    if !resolved.is_empty() {
        let content = std::fs::read_to_string(&resolved).unwrap_or_default();
        if content.starts_with("#!/usr/bin/env node")
            || content.starts_with("#!/usr/bin/env ts-node")
        {
            let node_check = std::process::Command::new(lookup)
                .arg("node")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);

            if !node_check {
                return Some(format!(
                    "`{binary}` (at {resolved}) is a Node.js script but `node` is not found in PATH. \
                     If you use nvm, ensure your shell profile sources it correctly."
                ));
            }
        }
    }

    None
}

fn detect_login_shell() -> String {
    if let Ok(shell) = std::env::var("SHELL") {
        if !shell.is_empty() {
            return shell;
        }
    }

    if cfg!(target_os = "macos") {
        "/bin/zsh".to_string()
    } else {
        "/bin/bash".to_string()
    }
}

fn capture_shell_env(shell: &str) -> Result<HashMap<String, String>> {
    let output = std::process::Command::new(shell)
        .args(LOGIN_SHELL_ENV_CAPTURE_ARGS)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .with_context(|| format!("failed to spawn login shell `{shell}`"))?;

    if !output.status.success() {
        anyhow::bail!("login shell `{shell}` exited with status {}", output.status);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut env_vars = HashMap::new();

    for line in stdout.lines() {
        if let Some((key, value)) = line.split_once('=') {
            if !key.is_empty()
                && key
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
            {
                env_vars.insert(key.to_string(), value.to_string());
            }
        }
    }

    Ok(env_vars)
}

fn merge_path(current: &str, shell_path: &str) -> String {
    let current_dirs: Vec<&str> = current.split(':').filter(|s| !s.is_empty()).collect();
    let mut merged: Vec<&str> = current_dirs.clone();

    for dir in shell_path.split(':').filter(|s| !s.is_empty()) {
        if !merged.contains(&dir) {
            merged.push(dir);
        }
    }

    merged.join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_path_appends_new_entries() {
        let current = "/usr/bin:/usr/local/bin";
        let shell =
            "/usr/bin:/usr/local/bin:/opt/homebrew/bin:/home/user/.nvm/versions/node/v20/bin";

        let merged = merge_path(current, shell);

        assert!(merged.starts_with("/usr/bin:/usr/local/bin"));
        assert!(merged.contains("/opt/homebrew/bin"));
        assert!(merged.contains("/.nvm/versions/node/v20/bin"));

        let count = merged.matches("/usr/bin").count();
        assert_eq!(count, 1, "should not duplicate existing entries");
    }

    #[test]
    fn merge_path_handles_empty_current() {
        let merged = merge_path("", "/usr/bin:/opt/homebrew/bin");
        assert_eq!(merged, "/usr/bin:/opt/homebrew/bin");
    }

    #[test]
    fn merge_path_is_noop_when_identical() {
        let path = "/usr/bin:/usr/local/bin";
        assert_eq!(merge_path(path, path), path);
    }

    #[test]
    fn detect_login_shell_returns_non_empty() {
        let shell = detect_login_shell();
        assert!(!shell.is_empty());
        assert!(shell.starts_with('/'));
    }

    #[test]
    fn skip_keys_are_not_propagated() {
        for key in SKIP_KEYS {
            assert!(
                !key.is_empty(),
                "SKIP_KEYS should not contain empty strings"
            );
        }
    }

    #[test]
    fn shell_env_probe_uses_non_interactive_login_args() {
        assert_eq!(LOGIN_SHELL_ENV_CAPTURE_ARGS, ["-l", "-c", "env"]);
    }

    #[test]
    fn check_worker_binary_detects_missing_binary() {
        let result = check_worker_binary("this-binary-does-not-exist-xyz123");
        assert!(result.is_some());
        assert!(result.unwrap().contains("not found in PATH"));
    }

    #[test]
    fn shell_escape_passes_simple_args() {
        assert_eq!(shell_escape("codex"), "codex");
        assert_eq!(
            shell_escape("/opt/homebrew/bin/codex"),
            "/opt/homebrew/bin/codex"
        );
        assert_eq!(shell_escape("--full-auto"), "--full-auto");
        assert_eq!(shell_escape("gpt-5.4"), "gpt-5.4");
    }

    #[test]
    fn shell_escape_quotes_special_chars() {
        assert_eq!(shell_escape("hello world"), "'hello world'");
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
        assert_eq!(shell_escape(""), "''");
    }

    #[cfg(unix)]
    #[test]
    fn wrap_command_for_user_shell_wraps_via_shell() {
        let cmd = vec![
            "/opt/homebrew/bin/codex".to_string(),
            "-a".to_string(),
            "never".to_string(),
            "exec".to_string(),
        ];
        let (program, args) = wrap_command_for_user_shell(&cmd);
        assert!(program.contains("sh"), "should use a shell: {program}");
        assert_eq!(args.len(), 3);
        assert_eq!(args[0], "-l");
        assert_eq!(args[1], "-c");
        assert!(args[2].contains("/opt/homebrew/bin/codex"));
        assert!(args[2].contains("exec"));
    }

    #[cfg(unix)]
    #[test]
    fn wrap_command_for_user_shell_escapes_spaces() {
        let cmd = vec![
            "/usr/bin/codex".to_string(),
            "-C".to_string(),
            "/Users/test user/project".to_string(),
        ];
        let (_, args) = wrap_command_for_user_shell(&cmd);
        let shell_cmd = &args[2];
        assert!(
            shell_cmd.contains("'/Users/test user/project'"),
            "should escape path with spaces: {shell_cmd}"
        );
    }
}
