import { basename, formatDate } from "./utils";
import type { WorkspacePayload, WorkspaceRunSummary } from "./types";
import { useTranslation } from "react-i18next";

function dotClass(run: WorkspaceRunSummary): string {
  if (run.lifecycle === "running") return "tl-dot running";
  if (run.lifecycle === "passed") return "tl-dot passed";
  if (run.lifecycle === "failed") return "tl-dot failed";
  return "tl-dot";
}

function lifecyclePill(run: WorkspaceRunSummary): string {
  return run.lifecycle;
}

export default function WorkspaceTimeline({
  workspace,
  isRunning,
  onSelectRun,
  onNewRun,
  onProjectSettings,
  onResumeRun,
}: {
  workspace: WorkspacePayload;
  isRunning: boolean;
  onSelectRun: (runRoot: string) => void;
  onNewRun: () => void;
  onProjectSettings: () => void;
  onResumeRun: (runRoot: string) => void;
}) {
  const { t } = useTranslation();
  const sortedRuns = [...workspace.runs].sort(
    (a, b) => new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime(),
  );

  return (
    <div className="ws-timeline" data-testid="ws-timeline">
      <div className="ws-timeline-header">
        <div className="ws-timeline-title-row">
          <h2>{workspace.record.display_name}</h2>
          <code>{workspace.record.workspace_path}</code>
        </div>
        <div className="ws-timeline-actions">
          <button
            className="primary-button"
            data-testid="ws-timeline-new-run"
            onClick={onNewRun}
            disabled={isRunning}
          >
            {t('actions.newRun')}
          </button>
          <button
            className="secondary-button"
            data-testid="ws-timeline-project-settings"
            onClick={onProjectSettings}
          >
            {t('actions.projectSettings')}
          </button>
        </div>
      </div>

      {sortedRuns.length === 0 ? (
        <div className="empty-state" data-testid="ws-timeline-empty">
          {t('timeline.noRuns')}
        </div>
      ) : (
        <div className="tl-list">
          {sortedRuns.map((run, index) => (
            <div key={run.run_root} className="tl-node-wrapper">
              <button
                className="tl-node"
                data-testid={`tl-node-${basename(run.run_root)}`}
                onClick={() => onSelectRun(run.run_root)}
              >
                <span className={dotClass(run)} />
                {index < sortedRuns.length - 1 && <span className="tl-connector" />}
                <div className="tl-body">
                  <span className="tl-date">{formatDate(run.updated_at)}</span>
                  <span className={`state-pill state-${run.lifecycle}`}>
                    {lifecyclePill(run)}
                  </span>
                  <span className="tl-summary">{run.run_title || basename(run.run_root)}</span>
                  <span className="tl-progress">
                    {run.lifecycle === "running" && run.active_stage
                      ? t('timeline.progress', { current: run.current_feature_index, total: run.total_features, stage: run.active_stage.stage, attempt: run.active_stage.attempt })
                      : t('timeline.summary', { total: run.total_features, lifecycle: run.lifecycle })}
                  </span>
                  {run.lifecycle === "running" ? (
                    <button
                      className="primary-button"
                      onClick={(e) => {
                        e.stopPropagation();
                        onResumeRun(run.run_root);
                      }}
                      disabled={isRunning}
                    >
                      {t('actions.resume')}
                    </button>
                  ) : run.lifecycle !== "completed" ? (
                    <button
                      className="primary-button"
                      onClick={(e) => {
                        e.stopPropagation();
                        onResumeRun(run.run_root);
                      }}
                      disabled={isRunning}
                    >
                      {t('actions.retry')}
                    </button>
                  ) : null}
                </div>
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
