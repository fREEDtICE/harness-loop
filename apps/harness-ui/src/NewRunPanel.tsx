import { useMemo, useState } from "react";
import type {
  PlannerConversationResponse,
  PlannerConversationTurn,
  PromptOverrides,
  PromptSnapshot,
} from "./types";
import { currentOverrides, readError } from "./utils";
import { PromptEditorModal } from "./ui-components";
import { useTranslation } from "react-i18next";

type PlannerMessage = PlannerConversationTurn & {
  response?: PlannerConversationResponse;
};

export default function NewRunPanel({
  workspaceName,
  promptDefaults,
  promptEffective,
  isRunning,
  onClose,
  onStart,
  onPlannerChat,
}: {
  workspaceName: string;
  promptDefaults: PromptSnapshot | null;
  promptEffective: PromptSnapshot | null;
  isRunning: boolean;
  onClose: () => void;
  onStart: (
    requestDraft: string,
    promptOverrides: PromptOverrides,
    featureLimit: number | null,
  ) => void;
  onPlannerChat: (
    requestDraft: string,
    promptOverrides: PromptOverrides,
    featureLimit: number | null,
    conversation: PlannerConversationTurn[],
  ) => Promise<PlannerConversationResponse>;
}) {
  const { t } = useTranslation();
  const [requestDraft, setRequestDraft] = useState("");
  const [plannerPrompt, setPlannerPrompt] = useState(promptEffective?.planner ?? "");
  const [builderPrompt, setBuilderPrompt] = useState(promptEffective?.builder ?? "");
  const [evaluatorPrompt, setEvaluatorPrompt] = useState(promptEffective?.evaluator ?? "");
  const [featureLimit, setFeatureLimit] = useState("");
  const [showPrompts, setShowPrompts] = useState(false);
  const [editingPrompt, setEditingPrompt] = useState<"planner" | "builder" | "evaluator" | null>(null);
  const [plannerInput, setPlannerInput] = useState("");
  const [plannerMessages, setPlannerMessages] = useState<PlannerMessage[]>([]);
  const [plannerError, setPlannerError] = useState<string | null>(null);
  const [plannerBusy, setPlannerBusy] = useState(false);

  const overrides = useMemo(() => currentOverrides({
    requestDraft,
    plannerPrompt,
    builderPrompt,
    evaluatorPrompt,
  }, promptDefaults), [requestDraft, plannerPrompt, builderPrompt, evaluatorPrompt, promptDefaults]);

  function handleStart() {
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

  async function handlePlannerChat() {
    const trimmedInput = plannerInput.trim();
    if (!trimmedInput || plannerBusy) return;

    const nextConversation: PlannerConversationTurn[] = [
      ...plannerMessages.map(({ role, content }) => ({ role, content })),
      { role: "user", content: trimmedInput },
    ];
    setPlannerMessages((current) => [...current, { role: "user", content: trimmedInput }]);
    setPlannerInput("");
    setPlannerBusy(true);
    setPlannerError(null);

    try {
      const response = await onPlannerChat(
        requestDraft,
        overrides,
        featureLimit ? Number(featureLimit) : null,
        nextConversation,
      );
      setPlannerMessages((current) => [
        ...current,
        { role: "planner", content: response.reply_markdown, response },
      ]);
    } catch (error) {
      setPlannerError(readError(error));
    } finally {
      setPlannerBusy(false);
    }
  }

  function applySuggestedRequest(response: PlannerConversationResponse) {
    setRequestDraft(response.revised_request);
    if (!featureLimit && response.suggested_feature_limit) {
      setFeatureLimit(String(response.suggested_feature_limit));
    }
  }

  const latestPlannerResponse = [...plannerMessages]
    .reverse()
    .find((message) => message.role === "planner" && message.response)?.response ?? null;

  return (
    <div className="settings-overlay" data-testid="new-run-overlay" onClick={onClose}>
      <div className="settings-panel" data-testid="new-run-panel" onClick={(e) => e.stopPropagation()}>
        <div className="settings-header">
          <div>
            <h2>{t("newRun.title")}</h2>
            <p className="settings-subtitle">{workspaceName}</p>
          </div>
          <button className="settings-close" onClick={onClose}>×</button>
        </div>

        <div className="settings-body new-run-body">
          <div className="new-run-request">
            <label>{t("newRun.request")}</label>
            <textarea
              data-testid="launch-request-textarea"
              value={requestDraft}
              onChange={(e) => setRequestDraft(e.target.value)}
              rows={9}
              placeholder={t("newRun.placeholder")}
            />
          </div>

          <div className="new-run-planner-chat">
            <div className="new-run-planner-chat-header">
              <div>
                <h3>{t("newRun.plannerChat")}</h3>
                <p>{t("newRun.plannerChatHelp")}</p>
              </div>
              <span className={`state-pill state-${latestPlannerResponse?.readiness ?? "running"}`}>
                {latestPlannerResponse
                  ? t(`newRun.readiness.${latestPlannerResponse.readiness}`)
                  : t("newRun.readiness.idle")}
              </span>
            </div>

            <div className="planner-chat-log" data-testid="planner-chat-log">
              {plannerMessages.length === 0 ? (
                <div className="empty-state">
                  {t("newRun.plannerChatEmpty")}
                </div>
              ) : (
                plannerMessages.map((message, index) => (
                  <div
                    key={`${message.role}-${index}`}
                    className={`planner-chat-message planner-chat-message-${message.role}`}
                  >
                    <span className="planner-chat-role">
                      {message.role === "user" ? t("newRun.you") : t("newRun.plannerAssistant")}
                    </span>
                    <div className="planner-chat-content">{message.content}</div>
                    {message.response ? (
                      <div className="planner-chat-response-meta">
                        {message.response.suggested_features.length > 0 ? (
                          <div className="planner-chat-meta-group">
                            <span>{t("newRun.suggestedFeatures")}</span>
                            <ul>
                              {message.response.suggested_features.map((feature) => (
                                <li key={feature}>{feature}</li>
                              ))}
                            </ul>
                          </div>
                        ) : null}
                        {message.response.confirmation_points.length > 0 ? (
                          <div className="planner-chat-meta-group">
                            <span>{t("newRun.confirmationPoints")}</span>
                            <ul>
                              {message.response.confirmation_points.map((point) => (
                                <li key={point}>{point}</li>
                              ))}
                            </ul>
                          </div>
                        ) : null}
                        {message.response.open_questions.length > 0 ? (
                          <div className="planner-chat-meta-group">
                            <span>{t("newRun.openQuestions")}</span>
                            <ul>
                              {message.response.open_questions.map((question) => (
                                <li key={question}>{question}</li>
                              ))}
                            </ul>
                          </div>
                        ) : null}
                        <button
                          className="secondary-button"
                          type="button"
                          onClick={() => applySuggestedRequest(message.response!)}
                        >
                          {t("actions.applySuggestedRequest")}
                        </button>
                      </div>
                    ) : null}
                  </div>
                ))
              )}
            </div>

            <div className="planner-chat-composer">
              <textarea
                value={plannerInput}
                onChange={(e) => setPlannerInput(e.target.value)}
                rows={3}
                placeholder={t("newRun.plannerChatPlaceholder")}
              />
              <div className="planner-chat-actions">
                {plannerError ? <span className="field-error">{plannerError}</span> : null}
                <button
                  className="secondary-button"
                  type="button"
                  onClick={() => void handlePlannerChat()}
                  disabled={plannerBusy || !plannerInput.trim()}
                >
                  {plannerBusy ? t("newRun.askingPlanner") : t("actions.askPlanner")}
                </button>
              </div>
            </div>
          </div>

          <div className="new-run-prompts">
            <div
              className="cfg-group-toggle"
              onClick={() => setShowPrompts((v) => !v)}
            >
              <span className={`cfg-toggle-arrow${showPrompts ? " open" : ""}`}>▶</span>
              <span>{t("newRun.promptOverrides")}</span>
            </div>

            {showPrompts && (
              <>
                <div className="settings-prompt-grid">
                  <div className="settings-prompt-item" onClick={() => setEditingPrompt("planner")}>
                    <label>{t("newRun.planner")}</label>
                    <span className="prompt-preview">{plannerPrompt || "—"}</span>
                    <button className="prompt-expand-btn" onClick={(e) => { e.stopPropagation(); setEditingPrompt("planner"); }}>{t("actions.edit")}</button>
                  </div>
                  <div className="settings-prompt-item" onClick={() => setEditingPrompt("builder")}>
                    <label>{t("newRun.builder")}</label>
                    <span className="prompt-preview">{builderPrompt || "—"}</span>
                    <button className="prompt-expand-btn" onClick={(e) => { e.stopPropagation(); setEditingPrompt("builder"); }}>{t("actions.edit")}</button>
                  </div>
                  <div className="settings-prompt-item" onClick={() => setEditingPrompt("evaluator")}>
                    <label>{t("newRun.evaluator")}</label>
                    <span className="prompt-preview">{evaluatorPrompt || "—"}</span>
                    <button className="prompt-expand-btn" onClick={(e) => { e.stopPropagation(); setEditingPrompt("evaluator"); }}>{t("actions.edit")}</button>
                  </div>
                </div>
                <button
                  className="secondary-button"
                  data-testid="prompt-reset-overrides"
                  onClick={handleReset}
                >
                  {t("actions.resetDefaults")}
                </button>
              </>
            )}
          </div>

          <div className="field-group">
            <label>{t("newRun.featureLimit")}</label>
            <input
              type="number"
              min={1}
              value={featureLimit}
              onChange={(e) => setFeatureLimit(e.target.value)}
              placeholder={t("newRun.allFeatures")}
            />
          </div>

          <div className="settings-actions">
            <button className="secondary-button" onClick={onClose}>{t("actions.cancel")}</button>
            <button
              className="primary-button"
              data-testid="launch-start-button"
              onClick={handleStart}
              disabled={isRunning}
            >
              {isRunning ? t("actions.runningEllipsis") : t("actions.startRun")}
            </button>
          </div>
        </div>
      </div>

      {editingPrompt ? (
        <PromptEditorModal
          title={t(`newRun.${editingPrompt}`)}
          markdown={editingPrompt === "planner" ? plannerPrompt : editingPrompt === "builder" ? builderPrompt : evaluatorPrompt}
          onChange={(v) => {
            if (editingPrompt === "planner") setPlannerPrompt(v);
            else if (editingPrompt === "builder") setBuilderPrompt(v);
            else setEvaluatorPrompt(v);
          }}
          onClose={() => setEditingPrompt(null)}
        />
      ) : null}
    </div>
  );
}
