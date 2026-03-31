import { invoke } from "@tauri-apps/api/core";
import {
  startTransition,
  useEffect,
  useRef,
  useState,
  useTransition,
} from "react";
import UIStateReporter from "./UIStateReporter";
import HarnessLanding from "./HarnessLanding";
import SetupWizard from "./SetupWizard";
import SettingsPanel from "./SettingsPanel";
import WorkspaceTimeline from "./WorkspaceTimeline";
import RunDetail from "./RunDetail";
import NewRunPanel from "./NewRunPanel";
import ProjectSettingsPanel from "./ProjectSettingsPanel";
import { useTranslation } from "react-i18next";
import { basename, currentOverrides, readError } from "./utils";
import type {
  EditorState,
  LaunchDraft,
  PromptOverrides,
  RunState,
  WorkspacePayload,
  WorkspaceRecord,
} from "./types";

export default function App() {
  const { t } = useTranslation();
  const [workspaces, setWorkspaces] = useState<WorkspaceRecord[]>([]);
  const [selectedWorkspacePath, setSelectedWorkspacePath] = useState<string | null>(null);
  const [workspace, setWorkspace] = useState<WorkspacePayload | null>(null);
  const [editors, setEditors] = useState<EditorState | null>(null);
  const [statusMessage, setStatusMessage] = useState<string>(t('app.openToBegin'));
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [isRunning, setIsRunning] = useState(false);
  const [isPending, startUiTransition] = useTransition();
  const [showSettings, setShowSettings] = useState(false);
  const [showNewRun, setShowNewRun] = useState(false);
  const [showProjectSettings, setShowProjectSettings] = useState(false);
  const [selectedRunRoot, setSelectedRunRoot] = useState<string | null>(null);
  const [needsSetup, setNeedsSetup] = useState<boolean | null>(null);
  const pollTimerRef = useRef<number | null>(null);

  useEffect(() => {
    invoke<boolean>("has_default_config")
      .then((hasConfig) => setNeedsSetup(!hasConfig))
      .catch(() => setNeedsSetup(false));
    void refreshWorkspaces();
  }, []);

  const promptDefaults = workspace?.prompts?.defaults ?? null;
  const activeRunRoot = workspace?.current_run?.run_root ?? workspace?.runs.find((run) => run.lifecycle === "running")?.run_root ?? null;

  const selectedRun = selectedRunRoot
    ? workspace?.current_run?.run_root === selectedRunRoot
      ? workspace.current_run
      : null
    : null;

  const isLive = selectedRun?.lifecycle === "running";

  useEffect(() => {
    if (!selectedWorkspacePath) {
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
      if (selectedRunRoot) {
        void inspectRun(selectedRunRoot);
      }
    }, 1250);

    return () => {
      if (pollTimerRef.current !== null) {
        window.clearInterval(pollTimerRef.current);
        pollTimerRef.current = null;
      }
    };
  }, [selectedWorkspacePath, activeRunRoot, isRunning, selectedRunRoot]);

  async function refreshWorkspaces() {
    try {
      const records = await invoke<WorkspaceRecord[]>("load_workspaces");
      setWorkspaces(records);
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

    if (!workspacePath) return;

    setSelectedRunRoot(null);
    startUiTransition(() => {
      setSelectedWorkspacePath(workspacePath);
    });
    await refreshWorkspace(workspacePath, true);
  }

  async function refreshWorkspace(workspacePath: string, announce = false) {
    try {
      const payload = await invoke<WorkspacePayload>("load_workspace", { workspacePath });
      startTransition(() => {
        setWorkspace((current) => {
          if (!current) return payload;
          const next = {
            ...payload,
            current_run: current.current_run,
          };
          if (JSON.stringify(current.record) === JSON.stringify(next.record)
            && JSON.stringify(current.runs) === JSON.stringify(next.runs)
            && current.current_run === next.current_run
            && JSON.stringify(current.prompts) === JSON.stringify(next.prompts)
            && current.config_error === next.config_error) {
            return current;
          }
          return next;
        });
        if (announce) {
          setEditors({
            requestDraft: "",
            plannerPrompt: payload.prompts?.effective.planner ?? "",
            builderPrompt: payload.prompts?.effective.builder ?? "",
            evaluatorPrompt: payload.prompts?.effective.evaluator ?? "",
          });
        }
        setWorkspaces((current) => {
          const idx = current.findIndex(r => r.workspace_path === payload.record.workspace_path);
          if (idx >= 0) {
            if (JSON.stringify(current[idx]) === JSON.stringify(payload.record)) {
              return current;
            }
            const next = [...current];
            next[idx] = payload.record;
            return next;
          }
          return [...current, payload.record];
        });
      });
      if (announce) {
        setStatusMessage(t('app.opened', { name: payload.record.display_name }));
      }
      setErrorMessage(payload.config_error);
    } catch (error) {
      setErrorMessage(readError(error));
    }
  }

  async function removeWorkspace(workspacePath: string) {
    try {
      const records = await invoke<WorkspaceRecord[]>("remove_workspace", { workspacePath });
      setWorkspaces(records);
      if (selectedWorkspacePath === workspacePath) {
        setSelectedWorkspacePath(null);
        setWorkspace(null);
        setEditors(null);
        setSelectedRunRoot(null);
      }
    } catch (error) {
      setErrorMessage(readError(error));
    }
  }

  async function inspectRun(runRoot: string) {
    if (!selectedWorkspacePath) return;
    try {
      const run = await invoke<RunState>("inspect_run", {
        workspacePath: selectedWorkspacePath,
        runRoot,
      });
      setWorkspace((current) => {
        if (!current) return current;
        const prev = current.current_run;
        if (prev && JSON.stringify(prev) === JSON.stringify(run)) {
          return current;
        }
        return { ...current, current_run: run };
      });
    } catch (error) {
      setErrorMessage(readError(error));
    }
  }

  async function handleSelectRun(runRoot: string) {
    setSelectedRunRoot(runRoot);
    await inspectRun(runRoot);
  }

  async function launchRun(requestDraft: string, promptOverrides: PromptOverrides, featureLimit: number | null) {
    if (!workspace || !selectedWorkspacePath) return;

    setShowNewRun(false);
    setIsRunning(true);
    setStatusMessage(t('app.startingRun'));
    setErrorMessage(null);

    const draft: LaunchDraft = {
      workspace_path: workspace.record.workspace_path,
      config_path: workspace.config_path,
      request_draft: requestDraft,
      prompt_overrides: promptOverrides,
      feature_limit: featureLimit,
    };

    try {
      const run = await invoke<RunState>("start_run", { draft });
      setWorkspace((current) =>
        current ? { ...current, current_run: run } : current,
      );
      setSelectedRunRoot(run.run_root);
      setStatusMessage(t('app.runCompleted'));
      await refreshWorkspace(selectedWorkspacePath, false);
    } catch (error) {
      setErrorMessage(readError(error));
      setStatusMessage(t('app.runFailed'));
    } finally {
      setIsRunning(false);
    }
  }

  async function resumeRun(runRoot: string) {
    if (!selectedWorkspacePath) return;
    setIsRunning(true);
    setStatusMessage(t('app.resuming', { name: basename(runRoot) }));
    setErrorMessage(null);

    try {
      const run = await invoke<RunState>("resume_run", {
        workspacePath: selectedWorkspacePath,
        runRoot,
      });
      setWorkspace((current) =>
        current ? { ...current, current_run: run } : current,
      );
      setSelectedRunRoot(run.run_root);
      setStatusMessage(t('app.resumeCompleted'));
      await refreshWorkspace(selectedWorkspacePath, false);
    } catch (error) {
      setErrorMessage(readError(error));
      setStatusMessage(t('app.resumeFailed'));
    } finally {
      setIsRunning(false);
    }
  }

  function goHome() {
    setSelectedWorkspacePath(null);
    setWorkspace(null);
    setEditors(null);
    setSelectedRunRoot(null);
  }

  function renderContent() {
    if (needsSetup) {
      return <SetupWizard onComplete={() => setNeedsSetup(false)} />;
    }

    if (!workspace) return <HarnessLanding />;

    if (selectedRunRoot && selectedRun) {
      return (
        <RunDetail
          run={selectedRun}
          isRunning={isRunning}
          isLive={isLive}
          onBack={() => setSelectedRunRoot(null)}
          onResume={resumeRun}
        />
      );
    }

    return (
      <WorkspaceTimeline
        workspace={workspace}
        isRunning={isRunning}
        onSelectRun={handleSelectRun}
        onNewRun={() => setShowNewRun(true)}
        onProjectSettings={() => setShowProjectSettings(true)}
        onResumeRun={resumeRun}
      />
    );
  }

  return (
    <div className="app-shell" data-testid="app-shell">
      <div className="backdrop backdrop-a" />
      <div className="backdrop backdrop-b" />
      <UIStateReporter
        workspaces={workspaces}
        selectedWorkspacePath={selectedWorkspacePath}
        workspace={workspace}
        editors={editors}
        statusMessage={statusMessage}
        errorMessage={errorMessage}
        isRunning={isRunning}
        isPending={isPending}
      />
      <header className="hero" data-testid="hero">
        <div
          className="hero-brand"
          data-testid="hero-brand"
          role="button"
          tabIndex={0}
          onClick={goHome}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") goHome();
          }}
        >
          <div className="hero-title-row">
            <h1>{t('app.title')}</h1>
            <p className="eyebrow">{t('app.controlRoom')}</p>
          </div>
          <p className="hero-copy">
            {t('app.subtitle')}
          </p>
        </div>
        <div className="hero-status" data-testid="hero-status">
          <button className="settings-button" data-testid="settings-button" onClick={() => setShowSettings(true)}>
            <span className="settings-icon">⚙</span>
            {t('app.settings')}
          </button>
          {errorMessage ? <div className="status-chip error" data-testid="error-message">{errorMessage}</div> : null}
        </div>
      </header>

      <main className="layout">
        <aside className="panel sidebar" data-testid="sidebar">
          <button
            className="sidebar-new-button"
            data-testid="sidebar-open-workspace"
            onClick={() => void openWorkspace()}
          >
            <span className="sidebar-new-icon">+</span>
            {t('app.newProject')}
          </button>

          <div className="sidebar-section-label" data-testid="sidebar-workspace-count">
            {t('app.workspaces', { count: workspaces.length })}
          </div>

          <div className="workspace-list" data-testid="sidebar-workspace-list">
            {workspaces.length === 0 ? (
              <div className="empty-state" data-testid="sidebar-empty-state">
                {t('app.openFolder')}
              </div>
            ) : (
              workspaces.map((record) => {
                const selected = selectedWorkspacePath === record.workspace_path;
                const running = selected && isRunning;
                return (
                  <button
                    key={record.workspace_path}
                    data-testid={`sidebar-workspace-${basename(record.workspace_path)}`}
                    className={`sidebar-item ${selected ? "selected" : ""}`}
                    onClick={() => void openWorkspace(record.workspace_path)}
                  >
                    <span className={`sidebar-item-icon ${running ? "running" : "idle"}`}>
                      {running ? "◉" : "○"}
                    </span>
                    <div className="sidebar-item-text">
                      <span className="sidebar-item-name">{record.display_name}</span>
                      <span className="sidebar-item-desc">
                        {running
                          ? t('app.running')
                          : selected && workspace
                            ? t('app.runs', { count: workspace.runs.length })
                            : ""}
                      </span>
                    </div>
                    <span
                      className="sidebar-item-remove"
                      data-testid={`sidebar-remove-${basename(record.workspace_path)}`}
                      onClick={(event) => {
                        event.stopPropagation();
                        void removeWorkspace(record.workspace_path);
                      }}
                    >
                      ×
                    </span>
                  </button>
                );
              })
            )}
          </div>
        </aside>

        <section className="content-area">
          {renderContent()}
        </section>
      </main>

      {showSettings ? (
        <SettingsPanel onClose={() => setShowSettings(false)} onError={setErrorMessage} />
      ) : null}

      {showNewRun && workspace ? (
        <NewRunPanel
          workspaceName={workspace.record.display_name}
          promptDefaults={promptDefaults}
          promptEffective={workspace.prompts?.effective ?? null}
          isRunning={isRunning}
          onClose={() => setShowNewRun(false)}
          onStart={launchRun}
        />
      ) : null}

      {showProjectSettings && selectedWorkspacePath ? (
        <ProjectSettingsPanel
          workspacePath={selectedWorkspacePath}
          onClose={() => setShowProjectSettings(false)}
          onError={setErrorMessage}
        />
      ) : null}
    </div>
  );
}
