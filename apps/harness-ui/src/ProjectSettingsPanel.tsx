import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { parse, stringify } from "smol-toml";
import type { WorkspaceDiscoveryPayload } from "./types";
import { readError } from "./utils";

interface ProjectFormState {
  evaluator: {
    dimensions: string[];
    require_screenshots: boolean;
    commands: string[][];
  };
  project: { root_dir: string };
  storage: { runs_dir: string };
  prompts: { planner: string; builder: string; evaluator: string };
  schemas: { planner_output: string; builder_handoff: string; qa_report: string };
}

function tomlToProjectForm(raw: string): ProjectFormState {
  const doc = parse(raw) as Record<string, unknown>;
  const evaluator = (doc.evaluator ?? {}) as Record<string, unknown>;
  const project = (doc.project ?? {}) as Record<string, unknown>;
  const storage = (doc.storage ?? {}) as Record<string, unknown>;
  const prompts = (doc.prompts ?? {}) as Record<string, unknown>;
  const schemas = (doc.schemas ?? {}) as Record<string, unknown>;

  return {
    evaluator: {
      dimensions: Array.isArray(evaluator.dimensions)
        ? (evaluator.dimensions as string[])
        : ["correctness"],
      require_screenshots: Boolean(evaluator.require_screenshots ?? false),
      commands: Array.isArray(evaluator.commands)
        ? (evaluator.commands as string[][])
        : [],
    },
    project: { root_dir: String(project.root_dir ?? "..") },
    storage: { runs_dir: String(storage.runs_dir ?? ".loopsmith-runs") },
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
  };
}

function projectFormToToml(form: ProjectFormState, originalRaw: string): string {
  let doc: Record<string, unknown>;
  try {
    doc = parse(originalRaw) as Record<string, unknown>;
  } catch {
    doc = {};
  }

  const existingEvaluator = (doc.evaluator ?? {}) as Record<string, unknown>;
  doc.evaluator = {
    ...existingEvaluator,
    dimensions: form.evaluator.dimensions,
    require_screenshots: form.evaluator.require_screenshots,
    commands: form.evaluator.commands,
  };

  doc.project = { ...((doc.project ?? {}) as Record<string, unknown>), root_dir: form.project.root_dir };
  doc.storage = { ...((doc.storage ?? {}) as Record<string, unknown>), runs_dir: form.storage.runs_dir };
  doc.prompts = { ...((doc.prompts ?? {}) as Record<string, unknown>), ...form.prompts };
  doc.schemas = { ...((doc.schemas ?? {}) as Record<string, unknown>), ...form.schemas };

  return stringify(doc);
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

export default function ProjectSettingsPanel({
  workspacePath,
  discovery,
  onClose,
  onError,
}: {
  workspacePath: string;
  discovery: WorkspaceDiscoveryPayload | null;
  onClose: () => void;
  onError: (msg: string | null) => void;
}) {
  const { t } = useTranslation();
  const [rawConfig, setRawConfig] = useState("");
  const [form, setForm] = useState<ProjectFormState | null>(null);
  const [parseError, setParseError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    void loadConfig();
  }, [workspacePath]);

  async function loadConfig() {
    try {
      const config = await invoke<string>("read_workspace_config", { workspacePath });
      setRawConfig(config);
      try {
        setForm(tomlToProjectForm(config));
        setParseError(null);
      } catch (err) {
        setParseError(readError(err));
      }
      setLoaded(true);
    } catch (error) {
      onError(readError(error));
    }
  }

  async function saveConfig() {
    setSaving(true);
    try {
      let content: string;
      if (form && !parseError) {
        content = projectFormToToml(form, rawConfig);
      } else {
        content = rawConfig;
      }
      await invoke("write_workspace_config", { workspacePath, content });
      onError(null);
      onClose();
    } catch (error) {
      onError(readError(error));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="settings-overlay" data-testid="project-settings-overlay" onClick={onClose}>
      <div className="settings-panel" data-testid="project-settings-panel" onClick={(e) => e.stopPropagation()}>
        <div className="settings-header">
          <h2>{t('settings.projectTitle')}</h2>
          <button className="settings-close" onClick={onClose}>×</button>
        </div>

        {!loaded ? (
          <div className="empty-state">{t('settings.loadingSettings')}</div>
        ) : (
          <div className="settings-body">
            {parseError ? (
              <div className="cfg-parse-error">
                <p>{t('settings.configParseError')}</p>
                <p>{parseError}</p>
                <textarea
                  className="settings-textarea config-editor"
                  value={rawConfig}
                  onChange={(e) => {
                    setRawConfig(e.target.value);
                    try {
                      setForm(tomlToProjectForm(e.target.value));
                      setParseError(null);
                    } catch {
                      /* keep showing raw editor */
                    }
                  }}
                  rows={14}
                  spellCheck={false}
                />
              </div>
            ) : form ? (
              <div className="cfg-form">
                {discovery ? (
                  <div className="cfg-group">
                    <div className="cfg-group-title">{t('settings.discovery')}</div>
                    <FormRow>
                      <FormField label={t('settings.profilePath')}>
                        <input value={discovery.status.profile_path} readOnly />
                      </FormField>
                      <FormField label={t('settings.lastRefresh')}>
                        <input value={discovery.status.last_refreshed_at ?? "—"} readOnly />
                      </FormField>
                    </FormRow>
                    <FormRow>
                      <FormField label={t('settings.scanPath')}>
                        <input value={discovery.status.scan_path} readOnly />
                      </FormField>
                      <FormField label={t('settings.refreshError')}>
                        <input value={discovery.status.last_refresh_error ?? "—"} readOnly />
                      </FormField>
                    </FormRow>
                  </div>
                ) : null}

                <div className="cfg-group">
                  <div className="cfg-group-title">{t('settings.evaluator')}</div>
                  <FormRow>
                    <FormField label={t('settings.dimensions')}>
                      <input
                        value={form.evaluator.dimensions.join(", ")}
                        onChange={(e) =>
                          setForm({
                            ...form,
                            evaluator: {
                              ...form.evaluator,
                              dimensions: e.target.value.split(",").map((s) => s.trim()).filter(Boolean),
                            },
                          })
                        }
                      />
                    </FormField>
                    <FormField label={t('settings.requireScreenshots')}>
                      <input
                        type="checkbox"
                        checked={form.evaluator.require_screenshots}
                        onChange={(e) =>
                          setForm({
                            ...form,
                            evaluator: { ...form.evaluator, require_screenshots: e.target.checked },
                          })
                        }
                      />
                    </FormField>
                  </FormRow>
                  <FormRow>
                    <FormField label={t('settings.commands')}>
                      <input
                        value={form.evaluator.commands.map((cmd) => cmd.join(" ")).join("; ")}
                        onChange={(e) =>
                          setForm({
                            ...form,
                            evaluator: {
                              ...form.evaluator,
                              commands: e.target.value
                                .split(";")
                                .map((seg) => seg.trim().split(/\s+/).filter(Boolean))
                                .filter((cmd) => cmd.length > 0),
                            },
                          })
                        }
                      />
                    </FormField>
                  </FormRow>
                </div>

                <div className="cfg-group">
                  <div className="cfg-group-title">{t('settings.paths')}</div>
                  <FormRow>
                    <FormField label={t('settings.projectRoot')}>
                      <input
                        value={form.project.root_dir}
                        onChange={(e) =>
                          setForm({ ...form, project: { ...form.project, root_dir: e.target.value } })
                        }
                      />
                    </FormField>
                    <FormField label={t('settings.runsDir')}>
                      <input
                        value={form.storage.runs_dir}
                        onChange={(e) =>
                          setForm({ ...form, storage: { ...form.storage, runs_dir: e.target.value } })
                        }
                      />
                    </FormField>
                  </FormRow>
                  <FormRow>
                    <FormField label={t('settings.plannerPrompt')}>
                      <input
                        value={form.prompts.planner}
                        onChange={(e) =>
                          setForm({ ...form, prompts: { ...form.prompts, planner: e.target.value } })
                        }
                      />
                    </FormField>
                    <FormField label={t('settings.builderPrompt')}>
                      <input
                        value={form.prompts.builder}
                        onChange={(e) =>
                          setForm({ ...form, prompts: { ...form.prompts, builder: e.target.value } })
                        }
                      />
                    </FormField>
                    <FormField label={t('settings.evaluatorPrompt')}>
                      <input
                        value={form.prompts.evaluator}
                        onChange={(e) =>
                          setForm({ ...form, prompts: { ...form.prompts, evaluator: e.target.value } })
                        }
                      />
                    </FormField>
                  </FormRow>
                  <FormRow>
                    <FormField label={t('settings.plannerSchema')}>
                      <input
                        value={form.schemas.planner_output}
                        onChange={(e) =>
                          setForm({ ...form, schemas: { ...form.schemas, planner_output: e.target.value } })
                        }
                      />
                    </FormField>
                    <FormField label={t('settings.builderSchema')}>
                      <input
                        value={form.schemas.builder_handoff}
                        onChange={(e) =>
                          setForm({ ...form, schemas: { ...form.schemas, builder_handoff: e.target.value } })
                        }
                      />
                    </FormField>
                    <FormField label={t('settings.qaSchema')}>
                      <input
                        value={form.schemas.qa_report}
                        onChange={(e) =>
                          setForm({ ...form, schemas: { ...form.schemas, qa_report: e.target.value } })
                        }
                      />
                    </FormField>
                  </FormRow>
                </div>
              </div>
            ) : null}

            <div className="settings-actions">
              <button className="secondary-button" onClick={onClose}>{t('actions.cancel')}</button>
              <button
                className="primary-button"
                data-testid="project-settings-save"
                onClick={() => void saveConfig()}
                disabled={saving}
              >
                {saving ? t('actions.saving') : t('actions.save')}
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
