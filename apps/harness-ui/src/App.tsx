import { invoke } from "@tauri-apps/api/core";
import {
  startTransition,
  useEffect,
  useMemo,
  useRef,
  useState,
  useTransition,
} from "react";

type PromptOverrides = {
  planner: string | null;
  builder: string | null;
  evaluator: string | null;
};

type PromptSnapshot = {
  planner: string;
  builder: string;
  evaluator: string;
};

type WorkspaceProfile = {
  workspace_path: string;
  display_name: string;
  preferred_config_path: string | null;
  request_draft: string;
  prompt_overrides: PromptOverrides;
  last_opened_at: string;
  last_run_root: string | null;
  pinned: boolean;
};

type ActiveRunStage = {
  stage: string;
  attempt: number;
  feature_index: number | null;
  feature_id: string | null;
  started_at: string;
};

type WorkspaceRunSummary = {
  run_root: string;
  created_at: string;
  updated_at: string;
  lifecycle: string;
  final_status: string | null;
  current_feature_index: number;
  total_features: number;
  active_stage: ActiveRunStage | null;
};

type RunStageRecord = {
  stage: string;
  attempt: number;
  status: string;
  artifact: string;
  session_id: string | null;
};

type FeatureRunState = {
  index: number;
  feature_id: string;
  title: string;
  feature_root: string;
  contract_file: string;
  builder_handoff_file: string;
  qa_report_file: string;
  status: string;
  phase: string;
  repair_attempts_used: number;
  next_evaluate_attempt: number;
  last_session_id: string | null;
  last_qa_status: string | null;
  stages: RunStageRecord[];
};

type RunState = {
  run_id: string;
  created_at: string;
  updated_at: string;
  run_root: string;
  state_file: string;
  manifest_file: string;
  launch_file: string | null;
  request_file: string;
  plan_file: string;
  runtime_plan_file: string;
  source_workspace: string;
  execution_workspace: string;
  lifecycle: string;
  final_status: string | null;
  current_feature_index: number;
  active_stage: ActiveRunStage | null;
  plan_stage: RunStageRecord | null;
  features: FeatureRunState[];
};

type PromptBundle = {
  defaults: PromptSnapshot;
  effective: PromptSnapshot;
};

type WorkspacePayload = {
  profile: WorkspaceProfile;
  prompts: PromptBundle | null;
  runs: WorkspaceRunSummary[];
  current_run: RunState | null;
  config_error: string | null;
};

type LaunchDraft = {
  workspace_path: string;
  config_path: string;
  request_draft: string;
  prompt_overrides: PromptOverrides;
  feature_limit: number | null;
};

type EditorState = {
  configPath: string;
  requestDraft: string;
  plannerPrompt: string;
  builderPrompt: string;
  evaluatorPrompt: string;
};

const emptyOverrides: PromptOverrides = {
  planner: null,
  builder: null,
  evaluator: null,
};

export default function App() {
  const [profiles, setProfiles] = useState<WorkspaceProfile[]>([]);
  const [selectedWorkspacePath, setSelectedWorkspacePath] = useState<string | null>(null);
  const [workspacePathDraft, setWorkspacePathDraft] = useState("");
  const [workspace, setWorkspace] = useState<WorkspacePayload | null>(null);
  const [editors, setEditors] = useState<EditorState | null>(null);
  const [statusMessage, setStatusMessage] = useState("Open a workspace to begin.");
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [isRunning, setIsRunning] = useState(false);
  const [isPending, startUiTransition] = useTransition();
  const pollTimerRef = useRef<number | null>(null);
  const saveTimerRef = useRef<number | null>(null);

  useEffect(() => {
    void refreshProfiles();
  }, []);

  const selectedProfile = useMemo(() => workspace?.profile ?? null, [workspace]);
  const promptDefaults = workspace?.prompts?.defaults ?? null;
  const activeRun = workspace?.current_run ?? null;
  const activeRunRoot = activeRun?.run_root ?? workspace?.runs.find((run) => run.lifecycle === "running")?.run_root ?? null;

  useEffect(() => {
    if (!selectedWorkspacePath || !selectedProfile || !editors) {
      return;
    }

    if (saveTimerRef.current !== null) {
      window.clearTimeout(saveTimerRef.current);
    }

    const updatedProfile = buildUpdatedProfile(selectedProfile, editors, promptDefaults);
    saveTimerRef.current = window.setTimeout(() => {
      void saveProfile(updatedProfile);
    }, 350);

    return () => {
      if (saveTimerRef.current !== null) {
        window.clearTimeout(saveTimerRef.current);
        saveTimerRef.current = null;
      }
    };
  }, [selectedWorkspacePath, selectedProfile, editors, promptDefaults]);

  useEffect(() => {
    if (!selectedWorkspacePath || !workspace?.profile.preferred_config_path) {
      if (pollTimerRef.current !== null) {
        window.clearInterval(pollTimerRef.current);
        pollTimerRef.current = null;
      }
      return;
    }

    if (!activeRunRoot && !isRunning) {
      if (pollTimerRef.current !== null) {
        window.clearInterval(pollTimerRef.current);
        pollTimerRef.current = null;
      }
      return;
    }

    pollTimerRef.current = window.setInterval(() => {
      void refreshWorkspace(selectedWorkspacePath, false);
    }, 1250);

    return () => {
      if (pollTimerRef.current !== null) {
        window.clearInterval(pollTimerRef.current);
        pollTimerRef.current = null;
      }
    };
  }, [selectedWorkspacePath, workspace?.profile.preferred_config_path, activeRunRoot, isRunning]);

  async function refreshProfiles() {
    try {
      const nextProfiles = await invoke<WorkspaceProfile[]>("load_profiles");
      setProfiles(nextProfiles);
    } catch (error) {
      setErrorMessage(readError(error));
    }
  }

  async function openWorkspace(path?: string) {
    const requestedPath = path?.trim();
    const workspacePath =
      (requestedPath ? requestedPath : null) ??
      (await invoke<string | null>("pick_workspace_folder").catch((error) => {
        setErrorMessage(readError(error));
        return null;
      }));

    if (!workspacePath) {
      return;
    }

    startUiTransition(() => {
      setSelectedWorkspacePath(workspacePath);
      setWorkspacePathDraft(workspacePath);
    });
    await refreshWorkspace(workspacePath, true);
  }

  async function openWorkspaceFromDraft() {
    const workspacePath = workspacePathDraft.trim();
    if (!workspacePath) {
      setErrorMessage("Enter a workspace path.");
      return;
    }

    await openWorkspace(workspacePath);
  }

  async function refreshWorkspace(workspacePath: string, announce = false) {
    try {
      const payload = await invoke<WorkspacePayload>("load_workspace", {
        workspace_path: workspacePath,
      });
      startTransition(() => {
        setWorkspace(payload);
        setWorkspacePathDraft(payload.profile.workspace_path);
        setEditors({
          configPath: payload.profile.preferred_config_path ?? "",
          requestDraft: payload.profile.request_draft,
          plannerPrompt:
            payload.prompts?.effective.planner ??
            payload.profile.prompt_overrides.planner ??
            "",
          builderPrompt:
            payload.prompts?.effective.builder ??
            payload.profile.prompt_overrides.builder ??
            "",
          evaluatorPrompt:
            payload.prompts?.effective.evaluator ??
            payload.profile.prompt_overrides.evaluator ??
            "",
        });
        setProfiles((current) => upsertProfile(current, payload.profile));
      });
      if (announce) {
        setStatusMessage(`Opened ${payload.profile.display_name}.`);
      }
      setErrorMessage(payload.config_error);
    } catch (error) {
      setErrorMessage(readError(error));
    }
  }

  async function saveProfile(profile: WorkspaceProfile) {
    try {
      const nextProfiles = await invoke<WorkspaceProfile[]>("save_profile", { profile });
      setProfiles(nextProfiles);
      setWorkspace((current) =>
        current
          ? {
              ...current,
              profile,
            }
          : current,
      );
    } catch (error) {
      setErrorMessage(readError(error));
    }
  }

  async function removeProfile(workspacePath: string) {
    try {
      const nextProfiles = await invoke<WorkspaceProfile[]>("remove_profile", {
        workspace_path: workspacePath,
      });
      setProfiles(nextProfiles);
      if (selectedWorkspacePath === workspacePath) {
        setSelectedWorkspacePath(null);
        setWorkspace(null);
        setEditors(null);
      }
    } catch (error) {
      setErrorMessage(readError(error));
    }
  }

  async function chooseConfigFile() {
    const configPath = await invoke<string | null>("pick_config_file").catch((error) => {
      setErrorMessage(readError(error));
      return null;
    });
    if (!configPath || !editors) {
      return;
    }
    setEditors({
      ...editors,
      configPath,
    });
    await reloadPrompts(configPath, currentOverrides(editors, promptDefaults));
  }

  async function reloadPrompts(configPath?: string, overrides?: PromptOverrides) {
    const path = configPath ?? editors?.configPath ?? "";
    if (!path.trim()) {
      setErrorMessage("Select a config file first.");
      return;
    }

    try {
      const bundle = await invoke<PromptBundle>("load_prompt_bundle", {
        config_path: path,
        overrides: overrides ?? currentOverrides(editors, promptDefaults),
      });
      setWorkspace((current) =>
        current
          ? {
              ...current,
              prompts: bundle,
              config_error: null,
            }
          : current,
      );
      setEditors((current) =>
        current
          ? {
              ...current,
              plannerPrompt: bundle.effective.planner,
              builderPrompt: bundle.effective.builder,
              evaluatorPrompt: bundle.effective.evaluator,
            }
          : current,
      );
      setErrorMessage(null);
      setStatusMessage("Loaded prompts from config.");
      if (selectedWorkspacePath) {
        await refreshWorkspace(selectedWorkspacePath, false);
      }
    } catch (error) {
      setErrorMessage(readError(error));
    }
  }

  async function resetPromptOverrides() {
    if (!editors) {
      return;
    }
    await reloadPrompts(editors.configPath, emptyOverrides);
  }

  async function inspectRun(runRoot: string) {
    const configPath = editors?.configPath ?? workspace?.profile.preferred_config_path ?? "";
    if (!configPath) {
      setErrorMessage("Select a config file before inspecting a run.");
      return;
    }

    try {
      const run = await invoke<RunState>("inspect_run", {
        config_path: configPath,
        run_root: runRoot,
      });
      setWorkspace((current) =>
        current
          ? {
              ...current,
              current_run: run,
            }
          : current,
      );
    } catch (error) {
      setErrorMessage(readError(error));
    }
  }

  async function launchRun() {
    if (!workspace || !editors) {
      return;
    }

    if (!editors.configPath.trim()) {
      setErrorMessage("Select a config file first.");
      return;
    }

    setIsRunning(true);
    setStatusMessage("Starting harness run.");
    setErrorMessage(null);

    const draft: LaunchDraft = {
      workspace_path: workspace.profile.workspace_path,
      config_path: editors.configPath,
      request_draft: editors.requestDraft,
      prompt_overrides: currentOverrides(editors, promptDefaults),
      feature_limit: null,
    };

    try {
      const run = await invoke<RunState>("start_run", { draft });
      setWorkspace((current) =>
        current
          ? {
              ...current,
              current_run: run,
            }
          : current,
      );
      setStatusMessage("Harness run completed.");
      if (selectedWorkspacePath) {
        await refreshWorkspace(selectedWorkspacePath, false);
      }
    } catch (error) {
      setErrorMessage(readError(error));
      setStatusMessage("Harness run failed.");
    } finally {
      setIsRunning(false);
    }
  }

  async function resumeRun(runRoot: string) {
    const configPath = editors?.configPath ?? workspace?.profile.preferred_config_path ?? "";
    if (!configPath) {
      setErrorMessage("Select a config file before resuming a run.");
      return;
    }

    setIsRunning(true);
    setStatusMessage(`Resuming ${basename(runRoot)}.`);
    setErrorMessage(null);

    try {
      const run = await invoke<RunState>("resume_run", {
        config_path: configPath,
        run_root: runRoot,
      });
      setWorkspace((current) =>
        current
          ? {
              ...current,
              current_run: run,
            }
          : current,
      );
      setStatusMessage("Harness resume completed.");
      if (selectedWorkspacePath) {
        await refreshWorkspace(selectedWorkspacePath, false);
      }
    } catch (error) {
      setErrorMessage(readError(error));
      setStatusMessage("Harness resume failed.");
    } finally {
      setIsRunning(false);
    }
  }

  const runsDeferred = useMemo(() => workspace?.runs ?? [], [workspace?.runs]);

  return (
    <div className="app-shell">
      <div className="backdrop backdrop-a" />
      <div className="backdrop backdrop-b" />
      <header className="hero">
        <div>
          <p className="eyebrow">Harness Control Room</p>
          <h1>Codex Harness UI</h1>
          <p className="hero-copy">
            Workspace launcher, prompt studio, and durable loop monitor for the
            Rust harness.
          </p>
        </div>
        <div className="hero-status">
          <div className="status-chip">{statusMessage}</div>
          {errorMessage ? <div className="status-chip error">{errorMessage}</div> : null}
        </div>
      </header>

      <main className="layout">
        <aside className="panel sidebar">
          <div className="panel-header">
            <div>
              <p className="panel-kicker">Quick Links</p>
              <h2>Workspaces</h2>
            </div>
            <button className="primary-button" onClick={() => void openWorkspace()}>
              Open Workspace
            </button>
          </div>

          <form
            className="sidebar-intake"
            onSubmit={(event) => {
              event.preventDefault();
              void openWorkspaceFromDraft();
            }}
          >
            <div className="split-input">
              <input
                value={workspacePathDraft}
                onChange={(event) => setWorkspacePathDraft(event.target.value)}
                placeholder="/absolute/path/to/.workspace"
                spellCheck={false}
              />
              <button type="submit">Open Path</button>
            </div>
            <p className="sidebar-hint">
              Hidden folder? Paste the full path and open it directly.
            </p>
          </form>

          <div className="workspace-list">
            {profiles.length === 0 ? (
              <div className="empty-state">
                Pick a workspace folder to create the first quick link.
              </div>
            ) : (
              profiles.map((profile) => {
                const selected = selectedWorkspacePath === profile.workspace_path;
                return (
                  <button
                    key={profile.workspace_path}
                    className={`workspace-card ${selected ? "selected" : ""}`}
                    onClick={() => void openWorkspace(profile.workspace_path)}
                  >
                    <div className="workspace-card-top">
                      <span>{profile.display_name}</span>
                      <span
                        className="ghost-link"
                        onClick={(event) => {
                          event.stopPropagation();
                          void removeProfile(profile.workspace_path);
                        }}
                      >
                        Remove
                      </span>
                    </div>
                    <p className="workspace-path">{profile.workspace_path}</p>
                    {profile.last_run_root ? (
                      <p className="workspace-meta">
                        Last run {basename(profile.last_run_root)}
                      </p>
                    ) : (
                      <p className="workspace-meta">No recorded run yet</p>
                    )}
                  </button>
                );
              })
            )}
          </div>
        </aside>

        <section className="content-grid">
          <section className="panel launch-panel">
            <div className="panel-header">
              <div>
                <p className="panel-kicker">Launch Brief</p>
                <h2>
                  {workspace?.profile.display_name ?? "Select a workspace"}
                </h2>
              </div>
              {isPending ? <div className="subtle-pill">Loading</div> : null}
            </div>

            {workspace && editors ? (
              <>
                <div className="field-group">
                  <label>Workspace path</label>
                  <code>{workspace.profile.workspace_path}</code>
                </div>
                <div className="field-group">
                  <label>Config file</label>
                  <div className="split-input">
                    <input
                      value={editors.configPath}
                      onChange={(event) =>
                        setEditors({ ...editors, configPath: event.target.value })
                      }
                      placeholder="config/codex-cli.toml"
                    />
                    <button onClick={() => void chooseConfigFile()}>Choose</button>
                    <button
                      className="secondary-button"
                      onClick={() => void reloadPrompts()}
                    >
                      Reload
                    </button>
                  </div>
                  {workspace.config_error ? (
                    <p className="field-error">{workspace.config_error}</p>
                  ) : null}
                </div>
                <div className="field-group">
                  <label>Request</label>
                  <textarea
                    value={editors.requestDraft}
                    onChange={(event) =>
                      setEditors({ ...editors, requestDraft: event.target.value })
                    }
                    rows={9}
                    placeholder="Describe the feature slice to build."
                  />
                </div>
                <div className="action-row">
                  <button
                    className="primary-button"
                    onClick={() => void launchRun()}
                    disabled={isRunning}
                  >
                    {isRunning ? "Running…" : "Start Run"}
                  </button>
                  <button
                    className="secondary-button"
                    onClick={() => selectedWorkspacePath && void refreshWorkspace(selectedWorkspacePath, false)}
                  >
                    Refresh Workspace
                  </button>
                </div>
              </>
            ) : (
              <div className="empty-state large">
                The control room is empty. Open a workspace from the left rail.
              </div>
            )}
          </section>

          <section className="panel prompt-panel">
            <div className="panel-header">
              <div>
                <p className="panel-kicker">Prompt Studio</p>
                <h2>Planner, Builder, Evaluator</h2>
              </div>
              <button className="secondary-button" onClick={() => void resetPromptOverrides()}>
                Reset Overrides
              </button>
            </div>

            {workspace && editors ? (
              <div className="prompt-grid">
                <PromptEditor
                  title="Planner"
                  value={editors.plannerPrompt}
                  onChange={(value) => setEditors({ ...editors, plannerPrompt: value })}
                />
                <PromptEditor
                  title="Builder"
                  value={editors.builderPrompt}
                  onChange={(value) => setEditors({ ...editors, builderPrompt: value })}
                />
                <PromptEditor
                  title="Evaluator"
                  value={editors.evaluatorPrompt}
                  onChange={(value) => setEditors({ ...editors, evaluatorPrompt: value })}
                />
              </div>
            ) : (
              <div className="empty-state">
                Prompt editors appear once a workspace is selected.
              </div>
            )}
          </section>

          <section className="panel history-panel">
            <div className="panel-header">
              <div>
                <p className="panel-kicker">Run Ledger</p>
                <h2>Workspace History</h2>
              </div>
            </div>

            {runsDeferred.length === 0 ? (
              <div className="empty-state">No runs discovered for this workspace yet.</div>
            ) : (
              <div className="run-list">
                {runsDeferred.map((run) => (
                  <article key={run.run_root} className="run-card">
                    <div className="run-card-top">
                      <strong>{basename(run.run_root)}</strong>
                      <span className={`state-pill state-${run.lifecycle}`}>
                        {run.final_status
                          ? `${run.lifecycle} / ${run.final_status}`
                          : run.lifecycle}
                      </span>
                    </div>
                    <p className="workspace-meta">{formatDate(run.updated_at)}</p>
                    <p className="workspace-meta">
                      Feature {run.current_feature_index} of {run.total_features}
                    </p>
                    {run.active_stage ? (
                      <p className="workspace-meta accent">
                        Active {run.active_stage.stage} attempt {run.active_stage.attempt}
                      </p>
                    ) : null}
                    <div className="action-row compact">
                      <button
                        className="secondary-button"
                        onClick={() => void inspectRun(run.run_root)}
                      >
                        Inspect
                      </button>
                      {run.lifecycle === "running" ? (
                        <button
                          className="primary-button"
                          onClick={() => void resumeRun(run.run_root)}
                          disabled={isRunning}
                        >
                          Resume
                        </button>
                      ) : null}
                    </div>
                  </article>
                ))}
              </div>
            )}
          </section>

          <section className="panel monitor-panel">
            <div className="panel-header">
              <div>
                <p className="panel-kicker">Loop Monitor</p>
                <h2>Current Run</h2>
              </div>
              {activeRun ? (
                <span className={`state-pill state-${activeRun.lifecycle}`}>
                  {activeRun.final_status
                    ? `${activeRun.lifecycle} / ${activeRun.final_status}`
                    : activeRun.lifecycle}
                </span>
              ) : null}
            </div>

            {activeRun ? (
              <>
                <div className="monitor-summary">
                  <StatCard
                    label="Run Root"
                    value={basename(activeRun.run_root)}
                    detail={activeRun.run_root}
                  />
                  <StatCard
                    label="Progress"
                    value={`${activeRun.current_feature_index}/${activeRun.features.length}`}
                    detail="current feature / total features"
                  />
                  <StatCard
                    label="Active Stage"
                    value={
                      activeRun.active_stage
                        ? `${activeRun.active_stage.stage} ${activeRun.active_stage.attempt}`
                        : "idle"
                    }
                    detail={
                      activeRun.active_stage?.feature_id
                        ? `feature ${activeRun.active_stage.feature_id}`
                        : "no active stage"
                    }
                  />
                </div>
                <div className="timeline">
                  {activeRun.plan_stage ? (
                    <StageRow
                      heading="Plan"
                      text={`attempt ${activeRun.plan_stage.attempt} / ${activeRun.plan_stage.status}`}
                    />
                  ) : null}
                  {activeRun.features.map((feature) => (
                    <div key={feature.feature_id} className="feature-block">
                      <div className="feature-block-header">
                        <div>
                          <h3>{feature.title}</h3>
                          <p>
                            {feature.feature_id} · {feature.status} · {feature.phase}
                          </p>
                        </div>
                        <span className="subtle-pill">
                          repairs {feature.repair_attempts_used}
                        </span>
                      </div>
                      <div className="stage-list">
                        {feature.stages.map((stage) => (
                          <StageRow
                            key={`${feature.feature_id}-${stage.stage}-${stage.attempt}`}
                            heading={stage.stage}
                            text={`attempt ${stage.attempt} / ${stage.status}`}
                          />
                        ))}
                      </div>
                    </div>
                  ))}
                </div>
              </>
            ) : (
              <div className="empty-state">Select a run from the ledger to inspect it here.</div>
            )}
          </section>
        </section>
      </main>
    </div>
  );
}

function PromptEditor({
  title,
  value,
  onChange,
}: {
  title: string;
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <section className="prompt-editor">
      <div className="prompt-editor-header">
        <h3>{title}</h3>
      </div>
      <textarea value={value} onChange={(event) => onChange(event.target.value)} rows={9} />
    </section>
  );
}

function StatCard({
  label,
  value,
  detail,
}: {
  label: string;
  value: string;
  detail: string;
}) {
  return (
    <article className="stat-card">
      <span>{label}</span>
      <strong>{value}</strong>
      <p>{detail}</p>
    </article>
  );
}

function StageRow({ heading, text }: { heading: string; text: string }) {
  return (
    <div className="stage-row">
      <strong>{heading}</strong>
      <span>{text}</span>
    </div>
  );
}

function buildUpdatedProfile(
  profile: WorkspaceProfile,
  editors: EditorState,
  defaults: PromptSnapshot | null,
): WorkspaceProfile {
  return {
    ...profile,
    preferred_config_path: editors.configPath.trim() || null,
    request_draft: editors.requestDraft,
    prompt_overrides: currentOverrides(editors, defaults),
  };
}

function currentOverrides(
  editors: EditorState | null,
  defaults: PromptSnapshot | null,
): PromptOverrides {
  if (!editors) {
    return emptyOverrides;
  }

  if (!defaults) {
    return {
      planner: editors.plannerPrompt.trim() ? editors.plannerPrompt : null,
      builder: editors.builderPrompt.trim() ? editors.builderPrompt : null,
      evaluator: editors.evaluatorPrompt.trim() ? editors.evaluatorPrompt : null,
    };
  }

  return {
    planner:
      editors.plannerPrompt !== defaults.planner ? editors.plannerPrompt : null,
    builder:
      editors.builderPrompt !== defaults.builder ? editors.builderPrompt : null,
    evaluator:
      editors.evaluatorPrompt !== defaults.evaluator
        ? editors.evaluatorPrompt
        : null,
  };
}

function upsertProfile(
  profiles: WorkspaceProfile[],
  nextProfile: WorkspaceProfile,
): WorkspaceProfile[] {
  const existing = profiles.filter(
    (profile) => profile.workspace_path !== nextProfile.workspace_path,
  );
  return [nextProfile, ...existing].sort(
    (left, right) =>
      new Date(right.last_opened_at).getTime() -
      new Date(left.last_opened_at).getTime(),
  );
}

function basename(path: string): string {
  const segments = path.split(/[\\/]/).filter(Boolean);
  return segments.length > 0 ? segments[segments.length - 1] : path;
}

function formatDate(value: string): string {
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(new Date(value));
}

function readError(error: unknown): string {
  if (typeof error === "string") {
    return error;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return "Unexpected error";
}
