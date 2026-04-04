import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { readError } from "./utils";

type WorkerKind = "codex_cli" | "claude_cli" | "gemini_cli";

interface ToolStatus {
  kind: "Found";
  path: string;
  version: string | null;
  warnings: string[];
}

interface ToolStatusNotFound {
  kind: "NotFound";
}

interface ToolProbe {
  name: string;
  display_name: string;
  binary_name: string;
  status: ToolStatus | ToolStatusNotFound;
}

interface RuntimeProbe {
  name: string;
  status: { kind: "Found"; path: string; version: string } | { kind: "NotFound" };
}

interface EnvironmentReport {
  tools: ToolProbe[];
  node: RuntimeProbe;
}

interface SetupWizardProps {
  onComplete: () => void;
  onCancel?: () => void;
}

const MODELS: Record<WorkerKind, { value: string; label: string }[]> = {
  codex_cli: [
    { value: "gpt-5.4", label: "gpt-5.4" },
    { value: "gpt-5.4-mini", label: "gpt-5.4-mini" },
    { value: "gpt-5.3-codex", label: "gpt-5.3-codex" },
    { value: "gpt-5.2-codex", label: "gpt-5.2-codex" },
    { value: "gpt-5.2", label: "gpt-5.2" },
    { value: "gpt-5.1-codex-max", label: "gpt-5.1-codex-max" },
    { value: "gpt-5.1-codex-mini", label: "gpt-5.1-codex-mini" },
  ],
  claude_cli: [
    { value: "claude-sonnet-4-20250514", label: "claude-sonnet-4-20250514" },
    { value: "claude-opus-4-20250514", label: "claude-opus-4-20250514" },
    { value: "claude-sonnet-4.5-20250514", label: "claude-sonnet-4.5-20250514" },
  ],
  gemini_cli: [
    { value: "gemini-2.5-pro", label: "gemini-2.5-pro" },
    { value: "gemini-2.5-flash", label: "gemini-2.5-flash" },
  ],
};

const DEFAULT_BINARIES: Record<WorkerKind, string> = {
  codex_cli: "codex",
  claude_cli: "claude",
  gemini_cli: "gemini",
};

const SCAN_STEP_KEYS = [
  "setup.scanSteps.detectShellEnvironment",
  "setup.scanSteps.detectNode",
  "setup.scanSteps.detectCodex",
  "setup.scanSteps.detectClaude",
  "setup.scanSteps.detectGemini",
  "setup.scanSteps.resolveVersions",
] as const;

function toolKindForName(name: string): WorkerKind | null {
  if (name === "codex_cli") return "codex_cli";
  if (name === "claude_cli") return "claude_cli";
  if (name === "gemini_cli") return "gemini_cli";
  return null;
}

export default function SetupWizard({ onComplete, onCancel }: SetupWizardProps) {
  const { t } = useTranslation();
  const [report, setReport] = useState<EnvironmentReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selectedKind, setSelectedKind] = useState<WorkerKind>("codex_cli");
  const [selectedModel, setSelectedModel] = useState<string>(MODELS.codex_cli[0].value);
  const [saving, setSaving] = useState(false);
  const [scanStep, setScanStep] = useState(0);
  const [scanDone, setScanDone] = useState(false);

  useEffect(() => {
    const interval = setInterval(() => {
      setScanStep((s) => {
        if (s >= SCAN_STEP_KEYS.length - 1) {
          clearInterval(interval);
          return s;
        }
        return s + 1;
      });
    }, 600);

    const minDelay = new Promise<void>((r) => setTimeout(r, SCAN_STEP_KEYS.length * 600 + 400));
    const probe = probeEnvironment();

    void Promise.all([probe, minDelay]).then(() => {
      setScanStep(SCAN_STEP_KEYS.length);
      setTimeout(() => setScanDone(true), 300);
    });

    return () => clearInterval(interval);
  }, []);

  async function probeEnvironment() {
    try {
      const env = await invoke<EnvironmentReport>("probe_environment");
      setReport(env);

      const firstFound = env.tools.find((tp) => tp.status.kind === "Found");
      if (firstFound) {
        const kind = toolKindForName(firstFound.name);
        if (kind) {
          setSelectedKind(kind);
          setSelectedModel(MODELS[kind][0].value);
        }
      }
    } catch (err) {
      setError(readError(err));
    }
  }

  function handleKindChange(kind: WorkerKind) {
    setSelectedKind(kind);
    setSelectedModel(MODELS[kind][0].value);
  }

  function selectedToolProbe(): ToolProbe | undefined {
    return report?.tools.find((tp) => tp.name === selectedKind);
  }

  function selectedBinary(): string {
    const probe = selectedToolProbe();
    if (probe?.status.kind === "Found") return probe.status.path;
    return DEFAULT_BINARIES[selectedKind];
  }

  function warnings(): string[] {
    const probe = selectedToolProbe();
    const result: string[] = [];
    if (probe?.status.kind === "NotFound") {
      result.push(t("setup.toolNotFound", { name: probe.display_name }));
    } else if (probe?.status.kind === "Found" && probe.status.warnings.length > 0) {
      result.push(...probe.status.warnings);
    }
    if (
      selectedKind === "codex_cli" &&
      report?.node.status.kind === "NotFound"
    ) {
      result.push(t("setup.nodeNotFound"));
    }
    return result;
  }

  async function handleComplete() {
    setSaving(true);
    try {
      await invoke("save_setup_config", {
        kind: selectedKind,
        binary: selectedBinary(),
        model: selectedModel,
      });
      onComplete();
    } catch (err) {
      setError(readError(err));
    } finally {
      setSaving(false);
    }
  }

  const currentWarnings = warnings();

  return (
    <div
      className="setup-overlay"
      data-testid="setup-wizard"
      onClick={(event) => {
        event.stopPropagation();
        if (event.target === event.currentTarget) {
          onCancel?.();
        }
      }}
    >
      <div className="setup-panel" onClick={(event) => event.stopPropagation()}>
        <div className="setup-header">
          <h2>{t("setup.title")}</h2>
          <p className="setup-subtitle">{t("setup.subtitle")}</p>
        </div>

        <div className="setup-body">
          {!scanDone && !error && (
            <div className="setup-scanning">
              <div className="setup-scanning-spinner" />
              <div className="setup-scanning-title">{t("setup.scanning")}</div>
              <div className="setup-scanning-steps">
                {SCAN_STEP_KEYS.map((stepKey, i) => (
                  <div
                    key={stepKey}
                    className={`setup-scanning-step ${
                      i < scanStep ? "done" : i === scanStep ? "active" : ""
                    }`}
                  >
                    <span className="setup-scanning-step-dot">
                      {i < scanStep ? "✓" : i === scanStep ? "⟳" : "·"}
                    </span>
                    {t(stepKey)}
                  </div>
                ))}
              </div>
            </div>
          )}

          {error && (
            <div className="setup-error">{error}</div>
          )}

          {scanDone && report && (
            <>
              <div className="landing-card setup-section">
                <div className="setup-section-title">{t("setup.environment")}</div>

                <div className="setup-probe-list">
                  {report.tools.map((tool) => (
                    <div
                      key={tool.name}
                      className={`setup-probe-item ${tool.status.kind === "Found" ? "found" : "not-found"}`}
                    >
                      <span className="setup-probe-dot">
                        {tool.status.kind === "Found" ? "✓" : "✗"}
                      </span>
                      <div className="setup-probe-info">
                        <span className="setup-probe-name">{tool.display_name}</span>
                        {tool.status.kind === "Found" ? (
                          <span className="setup-probe-detail">
                            {tool.status.path}
                            {tool.status.version ? ` (${tool.status.version})` : ""}
                          </span>
                        ) : (
                          <span className="setup-probe-detail not-found">
                            {t("setup.notInstalled")}
                          </span>
                        )}
                        {tool.status.kind === "Found" &&
                          tool.status.warnings.map((w, i) => (
                            <span key={i} className="setup-probe-warning">⚠ {w}</span>
                          ))}
                      </div>
                    </div>
                  ))}

                  <div
                    className={`setup-probe-item ${report.node.status.kind === "Found" ? "found" : "not-found"}`}
                  >
                    <span className="setup-probe-dot">
                      {report.node.status.kind === "Found" ? "✓" : "✗"}
                    </span>
                    <div className="setup-probe-info">
                      <span className="setup-probe-name">{report.node.name}</span>
                      {report.node.status.kind === "Found" ? (
                        <span className="setup-probe-detail">
                          {report.node.status.path} ({report.node.status.version})
                        </span>
                      ) : (
                        <span className="setup-probe-detail not-found">
                          {t("setup.notInstalled")}
                        </span>
                      )}
                    </div>
                  </div>
                </div>
              </div>

              <div className="landing-card setup-section">
                <div className="setup-section-title">{t("setup.selectTool")}</div>
                <div className="setup-radio-group">
                  {(["codex_cli", "claude_cli", "gemini_cli"] as WorkerKind[]).map((kind) => {
                    const probe = report.tools.find((tp) => tp.name === kind);
                    return (
                      <label
                        key={kind}
                        className={`setup-radio-item ${selectedKind === kind ? "selected" : ""}`}
                      >
                        <input
                          type="radio"
                          name="setup-tool"
                          value={kind}
                          checked={selectedKind === kind}
                          onChange={() => handleKindChange(kind)}
                        />
                        <span className="setup-radio-label">
                          {probe?.display_name ?? kind}
                        </span>
                        {probe?.status.kind === "Found" && (
                          <span className="setup-radio-status found">✓</span>
                        )}
                        {probe?.status.kind === "NotFound" && (
                          <span className="setup-radio-status not-found">✗</span>
                        )}
                      </label>
                    );
                  })}
                </div>
              </div>

              <div className="landing-card setup-section">
                <div className="setup-section-title">{t("setup.selectModel")}</div>
                <select
                  className="setup-model-select"
                  value={selectedModel}
                  onChange={(e) => setSelectedModel(e.target.value)}
                >
                  {MODELS[selectedKind].map((m) => (
                    <option key={m.value} value={m.value}>{m.label}</option>
                  ))}
                </select>
              </div>

              {currentWarnings.length > 0 && (
                <div className="setup-warnings">
                  {currentWarnings.map((w, i) => (
                    <div key={i} className="setup-warning-item">⚠ {w}</div>
                  ))}
                </div>
              )}

              <div className="setup-actions">
                {onCancel && (
                  <button className="secondary-button" onClick={onCancel}>
                    {t("actions.cancel")}
                  </button>
                )}
                <button
                  className="primary-button"
                  data-testid="setup-complete"
                  disabled={saving}
                  onClick={() => void handleComplete()}
                >
                  {saving ? t("setup.saving") : t("setup.complete")}
                </button>
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
