import { useState } from "react";
import type { PromptOverrides, PromptSnapshot } from "./types";
import { currentOverrides, emptyOverrides } from "./utils";
import { PromptEditor } from "./ui-components";
import { useTranslation } from "react-i18next";

export default function NewRunPanel({
  workspaceName,
  promptDefaults,
  promptEffective,
  isRunning,
  onClose,
  onStart,
}: {
  workspaceName: string;
  promptDefaults: PromptSnapshot | null;
  promptEffective: PromptSnapshot | null;
  isRunning: boolean;
  onClose: () => void;
  onStart: (requestDraft: string, promptOverrides: PromptOverrides, featureLimit: number | null) => void;
}) {
  const { t } = useTranslation();
  const [requestDraft, setRequestDraft] = useState("");
  const [plannerPrompt, setPlannerPrompt] = useState(promptEffective?.planner ?? "");
  const [builderPrompt, setBuilderPrompt] = useState(promptEffective?.builder ?? "");
  const [evaluatorPrompt, setEvaluatorPrompt] = useState(promptEffective?.evaluator ?? "");
  const [featureLimit, setFeatureLimit] = useState("");
  const [showPrompts, setShowPrompts] = useState(false);

  function handleStart() {
    const editors = {
      requestDraft,
      plannerPrompt,
      builderPrompt,
      evaluatorPrompt,
    };
    const overrides = currentOverrides(editors, promptDefaults);
    onStart(requestDraft, overrides, featureLimit ? Number(featureLimit) : null);
  }

  function handleReset() {
    if (promptDefaults) {
      setPlannerPrompt(promptDefaults.planner);
      setBuilderPrompt(promptDefaults.builder);
      setEvaluatorPrompt(promptDefaults.evaluator);
    } else {
      setPlannerPrompt("");
      setBuilderPrompt("");
      setEvaluatorPrompt("");
    }
  }

  return (
    <div className="settings-overlay" data-testid="new-run-overlay" onClick={onClose}>
      <div className="settings-panel" data-testid="new-run-panel" onClick={(e) => e.stopPropagation()}>
        <div className="settings-header">
          <h2>{t('newRun.title')}</h2>
          <button className="settings-close" onClick={onClose}>×</button>
        </div>

        <div className="settings-body">
          <div className="new-run-request">
            <label>{t('newRun.request')}</label>
            <textarea
              data-testid="launch-request-textarea"
              value={requestDraft}
              onChange={(e) => setRequestDraft(e.target.value)}
              rows={9}
              placeholder={t('newRun.placeholder')}
            />
          </div>

          <div className="new-run-prompts">
            <div
              className="cfg-group-toggle"
              onClick={() => setShowPrompts((v) => !v)}
            >
              <span className={`cfg-toggle-arrow${showPrompts ? " open" : ""}`}>▶</span>
              <span>{t('newRun.promptOverrides')}</span>
            </div>

            {showPrompts && (
              <>
                <div className="prompt-grid">
                  <PromptEditor
                    testid="prompt-editor-planner"
                    title={t('newRun.planner')}
                    value={plannerPrompt}
                    onChange={setPlannerPrompt}
                  />
                  <PromptEditor
                    testid="prompt-editor-builder"
                    title={t('newRun.builder')}
                    value={builderPrompt}
                    onChange={setBuilderPrompt}
                  />
                  <PromptEditor
                    testid="prompt-editor-evaluator"
                    title={t('newRun.evaluator')}
                    value={evaluatorPrompt}
                    onChange={setEvaluatorPrompt}
                  />
                </div>
                <button
                  className="secondary-button"
                  data-testid="prompt-reset-overrides"
                  onClick={handleReset}
                >
                  {t('actions.resetDefaults')}
                </button>
              </>
            )}
          </div>

          <div className="field-group">
            <label>{t('newRun.featureLimit')}</label>
            <input
              type="number"
              min={1}
              value={featureLimit}
              onChange={(e) => setFeatureLimit(e.target.value)}
              placeholder={t('newRun.allFeatures')}
            />
          </div>

          <div className="settings-actions">
            <button className="secondary-button" onClick={onClose}>{t('actions.cancel')}</button>
            <button
              className="primary-button"
              data-testid="launch-start-button"
              onClick={handleStart}
              disabled={isRunning}
            >
              {isRunning ? t('actions.runningEllipsis') : t('actions.startRun')}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
