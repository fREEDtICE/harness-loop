import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { PromptEditorModal } from "./ui-components";
import { useTranslation } from "react-i18next";
import { parse, stringify } from "smol-toml";
import { changeLocale } from "./i18n";
import { readError } from "./utils";
import SetupWizard from "./SetupWizard";

type WorkerKind = "acp" | "simulated";
type IsolationMode = "direct" | "git_worktree";

interface ConfigFormState {
  project: { root_dir: string };
  storage: { runs_dir: string };
  workspace: { isolation: IsolationMode };
  worker: {
    kind: WorkerKind;
    acp?: {
      command: string[];
      agent_name: string;
      resume_sessions: boolean;
    };
    simulation?: {
      evaluator_statuses: string[];
      session_prefix: string;
    };
  };
  prompts: { planner: string; builder: string; evaluator: string };
  schemas: { planner_output: string; builder_handoff: string; qa_report: string };
  runtime: {
    feature_limit: number;
    max_repair_attempts: number;
    continue_after_failure: boolean;
  };
  evaluator: {
    dimensions: string[];
    require_screenshots: boolean;
    commands: string[][];
  };
}

const DEFAULT_ACP = {
  command: ["codex"],
  agent_name: "acp-agent",
  resume_sessions: true,
};

const DEFAULT_SIMULATION = {
  evaluator_statuses: ["pass"],
  session_prefix: "simulated",
};

function tomlToForm(raw: string): ConfigFormState {
  const doc = parse(raw) as Record<string, unknown>;
  const project = (doc.project ?? {}) as Record<string, unknown>;
  const storage = (doc.storage ?? {}) as Record<string, unknown>;
  const workspace = (doc.workspace ?? {}) as Record<string, unknown>;
  const worker = (doc.worker ?? {}) as Record<string, unknown>;
  const prompts = (doc.prompts ?? {}) as Record<string, unknown>;
  const schemas = (doc.schemas ?? {}) as Record<string, unknown>;
  const runtime = (doc.runtime ?? {}) as Record<string, unknown>;
  const evaluator = (doc.evaluator ?? {}) as Record<string, unknown>;

  const kind = (worker.kind ?? "simulated") as WorkerKind;

  return {
    project: { root_dir: String(project.root_dir ?? "..") },
    storage: { runs_dir: String(storage.runs_dir ?? ".loopsmith-runs") },
    workspace: { isolation: (workspace.isolation ?? "direct") as IsolationMode },
    worker: {
      kind,
      acp: worker.acp
        ? { ...DEFAULT_ACP, ...(worker.acp as object) }
        : kind === "acp"
          ? { ...DEFAULT_ACP }
          : undefined,
      simulation: worker.simulation
        ? { ...DEFAULT_SIMULATION, ...(worker.simulation as object) }
        : kind === "simulated"
          ? { ...DEFAULT_SIMULATION }
          : undefined,
    },
    prompts: {
      planner: String(prompts.planner ?? "prompts/planner.md"),
      builder: String(prompts.builder ?? "prompts/builder.md"),
      evaluator: String(prompts.evaluator ?? "prompts/evaluator.md"),
    },
    schemas: {
      planner_output: String(schemas.planner_output ?? "schemas/planner-output.json"),
      builder_handoff: String(schemas.builder_handoff ?? "schemas/builder-handoff.json"),
      qa_report: String(schemas.qa_report ?? "schemas/qa-report.json"),
    },
    runtime: {
      feature_limit: Number(runtime.feature_limit ?? 1),
      max_repair_attempts: Number(runtime.max_repair_attempts ?? 1),
      continue_after_failure: Boolean(runtime.continue_after_failure ?? false),
    },
    evaluator: {
      dimensions: Array.isArray(evaluator.dimensions)
        ? (evaluator.dimensions as string[])
        : ["correctness"],
      require_screenshots: Boolean(evaluator.require_screenshots ?? false),
      commands: Array.isArray(evaluator.commands)
        ? (evaluator.commands as string[][])
        : [],
    },
  };
}

function formToToml(form: ConfigFormState, originalRaw: string): string {
  let doc: Record<string, unknown>;
  try {
    doc = parse(originalRaw) as Record<string, unknown>;
  } catch {
    doc = {};
  }

  doc.project = { root_dir: form.project.root_dir };
  doc.storage = { runs_dir: form.storage.runs_dir };
  doc.workspace = { isolation: form.workspace.isolation };

  const workerObj: Record<string, unknown> = { kind: form.worker.kind };
  const activeConfig = workerConfigForKind(form);
  if (activeConfig) {
    workerObj[workerSectionKey(form.worker.kind)] = activeConfig;
  }

  const existingWorker = (doc.worker ?? {}) as Record<string, unknown>;
  if (existingWorker.planner) {
    workerObj.planner = existingWorker.planner;
  }
  doc.worker = workerObj;

  doc.prompts = { ...form.prompts };
  doc.schemas = { ...form.schemas };

  const existingRuntime = (doc.runtime ?? {}) as Record<string, unknown>;
  doc.runtime = {
    ...existingRuntime,
    feature_limit: form.runtime.feature_limit,
    max_repair_attempts: form.runtime.max_repair_attempts,
    continue_after_failure: form.runtime.continue_after_failure,
  };

  const existingEvaluator = (doc.evaluator ?? {}) as Record<string, unknown>;
  doc.evaluator = {
    ...existingEvaluator,
    dimensions: form.evaluator.dimensions,
    require_screenshots: form.evaluator.require_screenshots,
    commands: form.evaluator.commands,
  };

  return stringify(doc);
}

function workerConfigForKind(form: ConfigFormState) {
  switch (form.worker.kind) {
    case "acp":
      return form.worker.acp ?? DEFAULT_ACP;
    case "simulated":
      return form.worker.simulation ?? DEFAULT_SIMULATION;
  }
}

function workerSectionKey(kind: WorkerKind): string {
  switch (kind) {
    case "acp":
      return "acp";
    case "simulated":
      return "simulation";
  }
}

function FormField({
  label,
  children,
  testid,
}: {
  label: string;
  children: React.ReactNode;
  testid?: string;
}) {
  return (
    <div className="cfg-field" data-testid={testid}>
      <label className="cfg-label">{label}</label>
      {children}
    </div>
  );
}

function FormRow({ children }: { children: React.ReactNode }) {
  return <div className="cfg-row">{children}</div>;
}

function WorkerSection({
  form,
  onChange,
}: {
  form: ConfigFormState;
  onChange: (f: ConfigFormState) => void;
}) {
  const kind = form.worker.kind;
  const { t } = useTranslation();

  function setKind(newKind: WorkerKind) {
    const next = { ...form.worker, kind: newKind };
    if (newKind === "acp" && !next.acp) next.acp = { ...DEFAULT_ACP };
    if (newKind === "simulated" && !next.simulation) next.simulation = { ...DEFAULT_SIMULATION };
    onChange({ ...form, worker: next });
  }

  function updateAcp(patch: Partial<NonNullable<ConfigFormState["worker"]["acp"]>>) {
    onChange({
      ...form,
      worker: {
        ...form.worker,
        acp: { ...(form.worker.acp ?? DEFAULT_ACP), ...patch },
      },
    });
  }

  function updateSimulation(patch: Partial<NonNullable<ConfigFormState["worker"]["simulation"]>>) {
    onChange({
      ...form,
      worker: {
        ...form.worker,
        simulation: { ...(form.worker.simulation ?? DEFAULT_SIMULATION), ...patch },
      },
    });
  }

  return (
    <div className="cfg-group">
      <div className="cfg-group-title">{t('settings.worker')}</div>
      <FormRow>
        <FormField label={t('settings.kind')} testid="cfg-worker-kind">
          <select
            value={kind}
            onChange={(e) => setKind(e.target.value as WorkerKind)}
          >
            <option value="acp">{t('settings.acp')}</option>
            <option value="simulated">{t('settings.simulated')}</option>
          </select>
        </FormField>
      </FormRow>

      {kind === "acp" && form.worker.acp && (
        <>
          <FormRow>
            <FormField label={t('settings.command')} testid="cfg-acp-command">
              <input
                value={form.worker.acp.command.join(", ")}
                onChange={(e) =>
                  updateAcp({
                    command: e.target.value.split(",").map((s) => s.trim()).filter(Boolean),
                  })
                }
              />
            </FormField>
            <FormField label={t('settings.agentName')} testid="cfg-acp-agent-name">
              <input value={form.worker.acp.agent_name} onChange={(e) => updateAcp({ agent_name: e.target.value })} />
            </FormField>
          </FormRow>
          <FormRow>
            <FormField label={t('settings.resumeSessions')} testid="cfg-acp-resume">
              <input type="checkbox" checked={form.worker.acp.resume_sessions} onChange={(e) => updateAcp({ resume_sessions: e.target.checked })} />
            </FormField>
          </FormRow>
        </>
      )}

      {kind === "simulated" && form.worker.simulation && (
        <FormRow>
          <FormField label={t('settings.sessionPrefix')} testid="cfg-sim-prefix">
            <input value={form.worker.simulation.session_prefix} onChange={(e) => updateSimulation({ session_prefix: e.target.value })} />
          </FormField>
          <FormField label={t('settings.evaluatorStatuses')} testid="cfg-sim-statuses">
            <input
              value={form.worker.simulation.evaluator_statuses.join(", ")}
              onChange={(e) =>
                updateSimulation({
                  evaluator_statuses: e.target.value.split(",").map((s) => s.trim()).filter(Boolean),
                })
              }
            />
          </FormField>
        </FormRow>
      )}
    </div>
  );
}

export default function SettingsPanel({
  onClose,
  onError,
}: {
  onClose: () => void;
  onError: (msg: string | null) => void;
}) {
  const [configForm, setConfigForm] = useState<ConfigFormState | null>(null);
  const { t, i18n } = useTranslation();
  const [rawConfig, setRawConfig] = useState("");
  const [parseError, setParseError] = useState<string | null>(null);
  const [plannerPrompt, setPlannerPrompt] = useState("");
  const [builderPrompt, setBuilderPrompt] = useState("");
  const [evaluatorPrompt, setEvaluatorPrompt] = useState("");
  const [saving, setSaving] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [editingPrompt, setEditingPrompt] = useState<"planner" | "builder" | "evaluator" | null>(null);
  const [showWizard, setShowWizard] = useState(false);

  useEffect(() => {
    void loadSettings();
  }, []);

  async function loadSettings() {
    try {
      const [config, planner, builder, evaluator] = await Promise.all([
        invoke<string>("read_global_config"),
        invoke<string>("read_global_prompt", { name: "planner.md" }),
        invoke<string>("read_global_prompt", { name: "builder.md" }),
        invoke<string>("read_global_prompt", { name: "evaluator.md" }),
      ]);
      setRawConfig(config);
      try {
        setConfigForm(tomlToForm(config));
        setParseError(null);
      } catch (err) {
        setParseError(readError(err));
      }
      setPlannerPrompt(planner);
      setBuilderPrompt(builder);
      setEvaluatorPrompt(evaluator);
      setLoaded(true);
    } catch (error) {
      onError(readError(error));
    }
  }

  async function saveSettings() {
    if (!configForm) return;
    setSaving(true);
    try {
      const toml = formToToml(configForm, rawConfig);
      await Promise.all([
        invoke("write_global_config", { content: toml }),
        invoke("write_global_prompt", { name: "planner.md", content: plannerPrompt }),
        invoke("write_global_prompt", { name: "builder.md", content: builderPrompt }),
        invoke("write_global_prompt", { name: "evaluator.md", content: evaluatorPrompt }),
      ]);
      onError(null);
      onClose();
    } catch (error) {
      onError(readError(error));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="settings-overlay" data-testid="settings-overlay" onClick={onClose}>
      <div className="settings-panel" data-testid="settings-panel" onClick={(e) => e.stopPropagation()}>
        <div className="settings-header">
          <h2>{t('settings.globalTitle')}</h2>
          <button className="settings-close" data-testid="settings-close" onClick={onClose}>×</button>
        </div>

        {!loaded ? (
          <div className="empty-state">{t('settings.loadingSettings')}</div>
        ) : (
          <div className="settings-body">
            <div className="settings-section">
              <div className="settings-section-header">
                <h3>{t('settings.defaultConfig')}</h3>
                <button
                  className="quick-setup-btn"
                  onClick={() => setShowWizard(true)}
                  title={t('settings.quickSetup')}
                >
                  ⚡ {t('settings.quickSetup')}
                </button>
              </div>
              <p className="settings-hint">{t('settings.changesApply')}</p>

              {parseError ? (
                <div className="cfg-parse-error" data-testid="cfg-parse-error">
                  <p>{t('settings.configParseError')}</p>
                  <p>{parseError}</p>
                  <textarea
                    data-testid="settings-config-editor"
                    className="settings-textarea config-editor"
                    value={rawConfig}
                    onChange={(e) => {
                      setRawConfig(e.target.value);
                      try {
                        setConfigForm(tomlToForm(e.target.value));
                        setParseError(null);
                      } catch {
                        /* keep showing raw editor */
                      }
                    }}
                    rows={14}
                    spellCheck={false}
                  />
                </div>
              ) : configForm ? (
                <div className="cfg-form" data-testid="cfg-form">
                  <WorkerSection form={configForm} onChange={setConfigForm} />

                  <div className="cfg-group">
                    <div className="cfg-group-title">{t('settings.runtime')}</div>
                    <FormRow>
                      <FormField label={t('settings.featureLimit')} testid="cfg-feature-limit">
                        <input
                          type="number"
                          min={1}
                          value={configForm.runtime.feature_limit}
                          onChange={(e) => {
                            const v = parseInt(e.target.value, 10);
                            if (!isNaN(v)) {
                              setConfigForm({
                                ...configForm,
                                runtime: { ...configForm.runtime, feature_limit: Math.max(1, v) },
                              });
                            }
                          }}
                        />
                      </FormField>
                      <FormField label={t('settings.maxRepairAttempts')} testid="cfg-max-repair">
                        <input
                          type="number"
                          min={0}
                          value={configForm.runtime.max_repair_attempts}
                          onChange={(e) => {
                            const v = parseInt(e.target.value, 10);
                            if (!isNaN(v)) {
                              setConfigForm({
                                ...configForm,
                                runtime: { ...configForm.runtime, max_repair_attempts: Math.max(0, v) },
                              });
                            }
                          }}
                        />
                      </FormField>
                      <FormField label={t('settings.continueAfterFailure')} testid="cfg-continue-failure">
                        <input
                          type="checkbox"
                          checked={configForm.runtime.continue_after_failure}
                          onChange={(e) =>
                            setConfigForm({
                              ...configForm,
                              runtime: { ...configForm.runtime, continue_after_failure: e.target.checked },
                            })
                          }
                        />
                      </FormField>
                    </FormRow>
                  </div>

                  <div className="cfg-group">
                    <div className="cfg-group-title">{t('settings.workspace')}</div>
                    <FormRow>
                      <FormField label={t('settings.isolation')} testid="cfg-isolation">
                        <select
                          value={configForm.workspace.isolation}
                          onChange={(e) =>
                            setConfigForm({
                              ...configForm,
                              workspace: { isolation: e.target.value as IsolationMode },
                            })
                          }
                        >
                          <option value="direct">{t('settings.direct')}</option>
                          <option value="git_worktree">{t('settings.gitWorktree')}</option>
                        </select>
                      </FormField>
                    </FormRow>
                  </div>
                </div>
              ) : null}
            </div>

            <div className="settings-section">
              <h3>{t('settings.defaultPrompts')}</h3>
              <p className="settings-hint">{t('settings.promptsCopied')}</p>
              <div className="settings-prompt-grid">
                <div className="settings-prompt-item" onClick={() => setEditingPrompt("planner")}>
                  <label>{t('settings.planner')}</label>
                  <span className="prompt-preview">{plannerPrompt || "—"}</span>
                  <button className="prompt-expand-btn" onClick={(e) => { e.stopPropagation(); setEditingPrompt("planner"); }}>{t('actions.edit')}</button>
                </div>
                <div className="settings-prompt-item" onClick={() => setEditingPrompt("builder")}>
                  <label>{t('settings.builder')}</label>
                  <span className="prompt-preview">{builderPrompt || "—"}</span>
                  <button className="prompt-expand-btn" onClick={(e) => { e.stopPropagation(); setEditingPrompt("builder"); }}>{t('actions.edit')}</button>
                </div>
                <div className="settings-prompt-item" onClick={() => setEditingPrompt("evaluator")}>
                  <label>{t('settings.evaluator')}</label>
                  <span className="prompt-preview">{evaluatorPrompt || "—"}</span>
                  <button className="prompt-expand-btn" onClick={(e) => { e.stopPropagation(); setEditingPrompt("evaluator"); }}>{t('actions.edit')}</button>
                </div>
              </div>
            </div>

            <div className="settings-section">
              <h3>{t('settings.language')}</h3>
              <FormRow>
                <FormField label={t('settings.language')} testid="cfg-language">
                  <select
                    className="locale-select"
                    value={i18n.language}
                    onChange={(e) => changeLocale(e.target.value)}
                  >
                    <option value="en">English</option>
                    <option value="zh">中文</option>
                  </select>
                </FormField>
              </FormRow>
            </div>

            <div className="settings-actions">
              <button className="secondary-button" onClick={onClose}>{t('actions.cancel')}</button>
              <button className="primary-button" data-testid="settings-save" onClick={() => void saveSettings()} disabled={saving}>
                {saving ? t('actions.saving') : t('actions.save')}
              </button>
            </div>
          </div>
        )}
      </div>

      {editingPrompt ? (
        <PromptEditorModal
          title={t(`settings.${editingPrompt}`)}
          markdown={editingPrompt === "planner" ? plannerPrompt : editingPrompt === "builder" ? builderPrompt : evaluatorPrompt}
          onChange={(v) => {
            if (editingPrompt === "planner") setPlannerPrompt(v);
            else if (editingPrompt === "builder") setBuilderPrompt(v);
            else setEvaluatorPrompt(v);
          }}
          onClose={() => setEditingPrompt(null)}
        />
      ) : null}

      {showWizard && (
        <SetupWizard
          onComplete={() => {
            setShowWizard(false);
            void loadSettings();
          }}
          onCancel={() => setShowWizard(false)}
        />
      )}
    </div>
  );
}
