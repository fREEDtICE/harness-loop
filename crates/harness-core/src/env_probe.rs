use std::process::Command as StdCommand;

use serde::Serialize;
use tracing::{debug, info};

/// Result of probing the local environment for supported CLI tools.
#[derive(Debug, Clone, Serialize)]
pub struct EnvironmentReport {
    pub tools: Vec<ToolProbe>,
    pub node: RuntimeProbe,
}

/// Probe result for a single CLI tool (codex, claude, gemini).
#[derive(Debug, Clone, Serialize)]
pub struct ToolProbe {
    pub name: &'static str,
    pub display_name: &'static str,
    pub binary_name: &'static str,
    pub status: ToolStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind")]
pub enum ToolStatus {
    Found {
        path: String,
        version: Option<String>,
        warnings: Vec<String>,
    },
    NotFound,
}

/// Probe result for a runtime dependency (e.g. Node.js).
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeProbe {
    pub name: &'static str,
    pub status: RuntimeStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind")]
pub enum RuntimeStatus {
    Found { path: String, version: String },
    NotFound,
}

struct ToolSpec {
    name: &'static str,
    display_name: &'static str,
    binary: &'static str,
    needs_node: bool,
}

const TOOL_SPECS: &[ToolSpec] = &[
    ToolSpec {
        name: "codex_cli",
        display_name: "Codex CLI",
        binary: "codex",
        needs_node: true,
    },
    ToolSpec {
        name: "claude_cli",
        display_name: "Claude Code",
        binary: "claude",
        needs_node: true,
    },
    ToolSpec {
        name: "gemini_cli",
        display_name: "Gemini CLI",
        binary: "gemini",
        needs_node: true,
    },
];

/// Probes the local environment and returns a structured report describing
/// which CLI tools are installed, their resolved paths, versions, and any
/// issues found (e.g. missing Node.js runtime).
pub fn probe_environment() -> EnvironmentReport {
    info!("probing local environment for CLI tools");

    let node = probe_node();
    let tools: Vec<ToolProbe> = TOOL_SPECS.iter().map(|spec| probe_tool(spec, &node)).collect();

    let report = EnvironmentReport { tools, node };

    for tool in &report.tools {
        match &tool.status {
            ToolStatus::Found {
                path,
                version,
                warnings,
            } => {
                info!(
                    tool = tool.name,
                    path = %path,
                    version = version.as_deref().unwrap_or("unknown"),
                    warnings = ?warnings,
                    "tool found"
                );
            }
            ToolStatus::NotFound => {
                debug!(tool = tool.name, "tool not found");
            }
        }
    }

    report
}

fn probe_node() -> RuntimeProbe {
    let lookup = if cfg!(windows) { "where" } else { "which" };

    let path = StdCommand::new(lookup)
        .arg("node")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string());

    let path = match path {
        Some(p) if !p.is_empty() => p,
        _ => {
            return RuntimeProbe {
                name: "node",
                status: RuntimeStatus::NotFound,
            }
        }
    };

    let version = StdCommand::new(&path)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    RuntimeProbe {
        name: "node",
        status: RuntimeStatus::Found { path, version },
    }
}

fn probe_tool(spec: &ToolSpec, node: &RuntimeProbe) -> ToolProbe {
    let lookup = if cfg!(windows) { "where" } else { "which" };

    let resolved_path = StdCommand::new(lookup)
        .arg(spec.binary)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string());

    let resolved_path = match resolved_path {
        Some(p) if !p.is_empty() => p,
        _ => {
            return ToolProbe {
                name: spec.name,
                display_name: spec.display_name,
                binary_name: spec.binary,
                status: ToolStatus::NotFound,
            }
        }
    };

    let mut warnings = Vec::new();

    if spec.needs_node {
        if matches!(node.status, RuntimeStatus::NotFound) {
            warnings.push(format!(
                "{} is a Node.js tool but `node` was not found in PATH. \
                 It will fail at runtime with exit code 127.",
                spec.display_name
            ));
        }
    }

    let version = StdCommand::new(&resolved_path)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let stdout = String::from_utf8_lossy(&o.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
            let combined = if stdout.is_empty() { stderr } else { stdout };
            if combined.is_empty() {
                None
            } else {
                Some(combined)
            }
        });

    ToolProbe {
        name: spec.name,
        display_name: spec.display_name,
        binary_name: spec.binary,
        status: ToolStatus::Found {
            path: resolved_path,
            version,
            warnings,
        },
    }
}

impl ToolProbe {
    pub fn is_found(&self) -> bool {
        matches!(self.status, ToolStatus::Found { .. })
    }

    pub fn resolved_path(&self) -> Option<&str> {
        match &self.status {
            ToolStatus::Found { path, .. } => Some(path.as_str()),
            ToolStatus::NotFound => None,
        }
    }

    pub fn has_warnings(&self) -> bool {
        match &self.status {
            ToolStatus::Found { warnings, .. } => !warnings.is_empty(),
            ToolStatus::NotFound => false,
        }
    }
}

impl EnvironmentReport {
    pub fn find_tool(&self, name: &str) -> Option<&ToolProbe> {
        self.tools.iter().find(|t| t.name == name)
    }

    pub fn any_tool_found(&self) -> bool {
        self.tools.iter().any(|t| t.is_found())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_environment_returns_all_tools() {
        let report = probe_environment();
        assert_eq!(report.tools.len(), TOOL_SPECS.len());

        let names: Vec<&str> = report.tools.iter().map(|t| t.name).collect();
        assert!(names.contains(&"codex_cli"));
        assert!(names.contains(&"claude_cli"));
        assert!(names.contains(&"gemini_cli"));
    }

    #[test]
    fn tool_probe_accessors_work_for_not_found() {
        let probe = ToolProbe {
            name: "test",
            display_name: "Test",
            binary_name: "test-bin",
            status: ToolStatus::NotFound,
        };
        assert!(!probe.is_found());
        assert!(probe.resolved_path().is_none());
        assert!(!probe.has_warnings());
    }

    #[test]
    fn tool_probe_accessors_work_for_found() {
        let probe = ToolProbe {
            name: "test",
            display_name: "Test",
            binary_name: "test-bin",
            status: ToolStatus::Found {
                path: "/usr/bin/test".to_string(),
                version: Some("1.0".to_string()),
                warnings: vec![],
            },
        };
        assert!(probe.is_found());
        assert_eq!(probe.resolved_path(), Some("/usr/bin/test"));
        assert!(!probe.has_warnings());
    }
}
