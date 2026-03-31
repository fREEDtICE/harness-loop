import { useState, useEffect, useRef, useCallback, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { basename, formatDate } from "./utils";
import { useTranslation } from "react-i18next";
import type { RunState, FeatureRunState, RunStageRecord } from "./types";

function fileBasename(p: string): string {
  const parts = p.replace(/\\/g, "/").split("/");
  return parts[parts.length - 1] || p;
}

function FileLink({ path, label }: { path: string; label?: string }) {
  const { t } = useTranslation();
  if (!path) return null;
  const display = label ?? fileBasename(path);

  function handleOpen(e: React.MouseEvent) {
    e.stopPropagation();
    invoke("open_file", { path }).catch(() => {});
  }

  function handleReveal(e: React.MouseEvent) {
    e.stopPropagation();
    invoke("reveal_in_finder", { path }).catch(() => {});
  }

  return (
    <span className="file-link" title={path}>
      <button className="file-link-name" onClick={handleOpen}>
        {display}
      </button>
      <button className="file-link-reveal" onClick={handleReveal} title={t('actions.revealInFinder')}>
        ⎋
      </button>
    </span>
  );
}

function FileRow({ label, path }: { label: string; path: string }) {
  if (!path) return null;
  return (
    <div className="file-row">
      <span className="file-row-label">{label}</span>
      <FileLink path={path} />
    </div>
  );
}

type PlanFeature = { id: string; title: string; summary: string; acceptance_criteria: string };

type FeatureContract = {
  feature_id: string;
  title: string;
  scope_notes: string[];
  acceptance_criteria: string[];
};

type BuilderHandoff = {
  summary: string;
  changed_files: string[];
  verification: string[];
  open_questions: string[];
};

type QaCheck = { name: string; command: string[]; rationale: string };
type QaReport = {
  status: "pass" | "fail" | "inconclusive";
  summary: string;
  findings: string[];
  next_actions: string[];
  checks: QaCheck[];
};

type PlanDocument = {
  goal: string;
  features: PlanFeature[];
  risks: string[];
  checkpoints: string[];
};

function useArtifact<T>(path: string | undefined): T | null {
  const [data, setData] = useState<T | null>(null);
  const prevPath = useRef(path);

  useEffect(() => {
    if (path !== prevPath.current) {
      prevPath.current = path;
      setData(null);
    }
    if (!path) return;
    invoke<string>("read_stage_log", { path }).then(
      (raw) => {
        if (!raw) return;
        try { setData(JSON.parse(raw) as T); } catch { /* ignore */ }
      },
      () => {},
    );
  }, [path]);

  return data;
}

function LinkedText({ text }: { text: string }) {
  const pathPattern = /(\/[\w.\-/]+(?:\.\w+)(?::\d+(?:-\d+)?)?)(\s|,|$)/g;
  const parts: React.ReactNode[] = [];
  let last = 0;
  let match: RegExpExecArray | null;

  while ((match = pathPattern.exec(text)) !== null) {
    const before = text.slice(last, match.index);
    if (before) parts.push(before);

    const full = match[1];
    const colonIdx = full.lastIndexOf(":");
    const hasLineRef = colonIdx > 0 && /^\d+(-\d+)?$/.test(full.slice(colonIdx + 1));
    const filePath = hasLineRef ? full.slice(0, colonIdx) : full;
    const display = fileBasename(filePath) + (hasLineRef ? full.slice(colonIdx) : "");

    parts.push(
      <button
        key={match.index}
        className="inline-file-link"
        title={full}
        onClick={() => invoke("open_file", { path: filePath }).catch(() => {})}
      >
        {display}
      </button>,
    );
    parts.push(match[2]);
    last = match.index + match[0].length;
  }

  const tail = text.slice(last);
  if (tail) parts.push(tail);

  if (parts.length === 0) return <>{text}</>;
  return <>{parts}</>;
}

function ContractCard({ data }: { data: FeatureContract }) {
  const { t } = useTranslation();
  return (
    <div className="artifact-card artifact-contract">
      <div className="artifact-card-header">
        <span className="artifact-icon">📜</span>
        <span className="artifact-title">{t('artifacts.contract')}</span>
      </div>
      {data.acceptance_criteria.length > 0 ? (
        <div className="artifact-section">
          <span className="artifact-section-label">{t('artifacts.acceptanceCriteria')}</span>
          <ul className="artifact-list">
            {data.acceptance_criteria.map((c, i) => <li key={i}>{c}</li>)}
          </ul>
        </div>
      ) : null}
      {data.scope_notes.length > 0 ? (
        <div className="artifact-section">
          <span className="artifact-section-label">{t('artifacts.scopeNotes')}</span>
          <ul className="artifact-list artifact-list-subtle">
            {data.scope_notes.map((n, i) => <li key={i}>{n}</li>)}
          </ul>
        </div>
      ) : null}
    </div>
  );
}

function HandoffCard({ data, workspacePath }: { data: BuilderHandoff; workspacePath: string }) {
  const { t } = useTranslation();
  function resolvePath(f: string): string {
    if (f.startsWith("/")) return f;
    return workspacePath.replace(/\/$/, "") + "/" + f.replace(/^\.\//, "");
  }

  return (
    <div className="artifact-card artifact-handoff">
      <div className="artifact-card-header">
        <span className="artifact-icon">🔨</span>
        <span className="artifact-title">{t('artifacts.buildHandoff')}</span>
      </div>
      <p className="artifact-summary">{data.summary}</p>
      {data.changed_files.length > 0 ? (
        <div className="artifact-section">
          <span className="artifact-section-label">{t('artifacts.changedFiles')}</span>
          <div className="artifact-file-chips">
            {data.changed_files.map((f, i) => {
              const abs = resolvePath(f);
              return (
                <button key={i} className="artifact-file-chip" title={abs} onClick={() => invoke("open_file", { path: abs }).catch(() => {})}>{shortPath(f)}</button>
              );
            })}
          </div>
        </div>
      ) : null}
      {data.open_questions.length > 0 ? (
        <div className="artifact-section">
          <span className="artifact-section-label">{t('artifacts.openQuestions')}</span>
          <ul className="artifact-list artifact-list-subtle">
            {data.open_questions.map((q, i) => <li key={i}>{q}</li>)}
          </ul>
        </div>
      ) : null}
    </div>
  );
}

function QaReportCard({ data }: { data: QaReport }) {
  const { t } = useTranslation();
  const isPass = data.status === "pass";
  const isFail = data.status === "fail";
  return (
    <div className={`artifact-card artifact-qa ${isPass ? "qa-pass" : isFail ? "qa-fail" : "qa-inconclusive"}`}>
      <div className="artifact-card-header">
        <span className="artifact-icon">{isPass ? "✅" : isFail ? "❌" : "⚠️"}</span>
        <span className="artifact-title">{t('artifacts.qaReport')}</span>
        <span className={`artifact-status-pill ${data.status}`}>{data.status}</span>
      </div>
      <p className="artifact-summary">{data.summary}</p>
      {data.findings.length > 0 ? (
        <div className="artifact-section">
          <span className="artifact-section-label">{t('artifacts.findings')}</span>
          <ul className="artifact-list">
            {data.findings.map((f, i) => <li key={i}><LinkedText text={f} /></li>)}
          </ul>
        </div>
      ) : null}
      {data.next_actions.length > 0 ? (
        <div className="artifact-section">
          <span className="artifact-section-label">{t('artifacts.nextActions')}</span>
          <ul className="artifact-list artifact-list-subtle">
            {data.next_actions.map((a, i) => <li key={i}><LinkedText text={a} /></li>)}
          </ul>
        </div>
      ) : null}
    </div>
  );
}

function PlanCard({ data }: { data: PlanDocument }) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);

  return (
    <div className="artifact-card artifact-plan">
      <div
        className="artifact-card-header artifact-card-toggle"
        role="button"
        tabIndex={0}
        onClick={() => setExpanded((v) => !v)}
        onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") setExpanded((v) => !v); }}
      >
        <span className="artifact-icon">📋</span>
        <span className="artifact-title">{t('artifacts.plan')}</span>
        <span className="artifact-chip">{t('artifacts.featuresCount', { count: data.features.length })}</span>
        {data.risks.length > 0 ? (
          <span className="artifact-chip artifact-chip-risk">{t('artifacts.risksCount', { count: data.risks.length })}</span>
        ) : null}
        <span className="artifact-toggle-arrow">{expanded ? "▾" : "▸"}</span>
      </div>
      <p className="artifact-summary">{data.goal}</p>
      {expanded ? (
        <>
          <div className="artifact-section">
            <span className="artifact-section-label">{t('artifacts.featuresSection', { count: data.features.length })}</span>
            <div className="plan-feature-grid">
              {data.features.map((f) => (
                <div key={f.id} className="plan-feature-mini">
                  <div className="plan-feature-mini-head">
                    <span className="plan-feature-mini-id">{f.id}</span>
                    <span>{f.title}</span>
                  </div>
                  <p className="plan-feature-mini-summary">{f.summary}</p>
                </div>
              ))}
            </div>
          </div>
          {data.risks.length > 0 ? (
            <div className="artifact-section artifact-risk-section">
              <span className="artifact-section-label artifact-risk-label">{t('artifacts.risks')}</span>
              <ul className="artifact-list artifact-risk-list">
                {data.risks.map((r, i) => <li key={i}>{r}</li>)}
              </ul>
            </div>
          ) : null}
        </>
      ) : null}
    </div>
  );
}

function FeatureArtifacts({ feature, workspacePath }: { feature: FeatureRunState; workspacePath: string }) {
  const contract = useArtifact<FeatureContract>(feature.contract_file || undefined);
  const handoff = useArtifact<BuilderHandoff>(feature.builder_handoff_file || undefined);
  const qaReport = useArtifact<QaReport>(feature.qa_report_file || undefined);

  if (!contract && !handoff && !qaReport) return null;

  return (
    <div className="feature-artifacts">
      {contract ? <ContractCard data={contract} /> : null}
      {handoff ? <HandoffCard data={handoff} workspacePath={workspacePath} /> : null}
      {qaReport ? <QaReportCard data={qaReport} /> : null}
    </div>
  );
}

type MessagePayload =
  | { shape: "plan"; goal: string; features: PlanFeature[] }
  | { shape: "build"; summary: string; changedFiles: string[] }
  | { shape: "evaluate"; status: string; summary: string; findings: string[] }
  | { shape: "plain"; text: string };

type LogEvent =
  | { kind: "session"; threadId: string }
  | { kind: "command"; command: string; status: "running" | "done"; exitCode: number | null; output: string }
  | { kind: "file_change"; changes: { path: string; kind: string }[] }
  | { kind: "message"; payload: MessagePayload }
  | { kind: "usage"; input: number; cached: number; output: number };

function parseLogEvents(raw: string): LogEvent[] {
  const events: LogEvent[] = [];
  const pendingCmds = new Map<string, number>();
  const seenFileChanges = new Set<string>();
  for (const line of raw.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    let obj: Record<string, unknown>;
    try { obj = JSON.parse(trimmed); } catch { continue; }
    const type = obj.type as string | undefined;
    const item = (obj.item ?? {}) as Record<string, unknown>;
    const itemType = item.type as string | undefined;
    const itemId = item.id as string | undefined;

    if (type === "thread.started") {
      const threadId = (obj.thread_id ?? obj.session_id ?? "") as string;
      if (threadId) events.push({ kind: "session", threadId });
    } else if (type === "item.started" && itemType === "command_execution") {
      const cmd = (item.command ?? "") as string;
      events.push({ kind: "command", command: cmd, status: "running", exitCode: null, output: "" });
      if (itemId) pendingCmds.set(itemId, events.length - 1);
    } else if (type === "item.completed" && itemType === "command_execution") {
      const cmd = (item.command ?? "") as string;
      const exitCode = (item.exit_code ?? null) as number | null;
      const output = (item.aggregated_output ?? "") as string;
      if (itemId && pendingCmds.has(itemId)) {
        const idx = pendingCmds.get(itemId)!;
        events[idx] = { kind: "command", command: cmd, status: "done", exitCode, output };
        pendingCmds.delete(itemId);
      } else {
        events.push({ kind: "command", command: cmd, status: "done", exitCode, output });
      }
    } else if ((type === "item.started" || type === "item.completed") && itemType === "file_change") {
      if (itemId && seenFileChanges.has(itemId)) continue;
      if (itemId) seenFileChanges.add(itemId);
      const changes = (item.changes ?? []) as { path: string; kind: string }[];
      if (changes.length > 0) {
        events.push({ kind: "file_change", changes });
      }
    } else if (type === "item.completed" && itemType === "agent_message") {
      const text = (item.text ?? "") as string;
      let payload: MessagePayload;
      try {
        const parsed = JSON.parse(text) as Record<string, unknown>;
        if (parsed.goal && Array.isArray(parsed.features)) {
          payload = {
            shape: "plan",
            goal: (parsed.goal ?? "") as string,
            features: (parsed.features as PlanFeature[]).map((f) => ({
              id: f.id ?? "",
              title: f.title ?? "",
              summary: f.summary ?? "",
              acceptance_criteria: f.acceptance_criteria ?? "",
            })),
          };
        } else if (parsed.status !== undefined && parsed.findings !== undefined) {
          payload = {
            shape: "evaluate",
            status: (parsed.status ?? "") as string,
            summary: (parsed.summary ?? "") as string,
            findings: Array.isArray(parsed.findings)
              ? (parsed.findings as string[])
              : [],
          };
        } else if (parsed.summary !== undefined) {
          payload = {
            shape: "build",
            summary: (parsed.summary ?? "") as string,
            changedFiles: Array.isArray(parsed.changed_files)
              ? (parsed.changed_files as string[])
              : [],
          };
        } else {
          payload = { shape: "plain", text };
        }
      } catch {
        payload = { shape: "plain", text };
      }
      events.push({ kind: "message", payload });
    } else if (type === "turn.completed") {
      const usage = (obj.usage ?? {}) as Record<string, number>;
      if (usage.input_tokens || usage.output_tokens) {
        events.push({
          kind: "usage",
          input: usage.input_tokens ?? 0,
          cached: usage.cached_input_tokens ?? 0,
          output: usage.output_tokens ?? 0,
        });
      }
    }
  }
  return events;
}

function shortPath(full: string): string {
  const parts = full.split("/");
  return parts.length > 2 ? parts.slice(-2).join("/") : full;
}

function stripShellWrapper(cmd: string): string {
  const m = cmd.match(/^\/bin\/(?:ba)?sh\s+-\w*c\s+['"](.+)['"]$/s);
  return m ? m[1] : cmd;
}

function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return `${n}`;
}

function MessageEvent({ payload }: { payload: MessagePayload }) {
  const { t } = useTranslation();
  switch (payload.shape) {
    case "plan":
      return (
        <div className="log-evt log-evt-msg log-evt-msg-plan">
          <div className="log-evt-msg-header">

            <span className="log-evt-icon">📋</span>
            <span className="log-evt-label">{t('log.planLabel')}</span>
          </div>
          <p className="log-evt-msg-goal">{payload.goal}</p>
          <div className="log-evt-msg-features">
            {payload.features.map((f) => (
              <div key={f.id} className="log-evt-plan-feat">
                <div className="log-evt-plan-feat-head">
                  <span className="log-evt-plan-feat-id">{f.id}</span>
                  <span>{f.title}</span>
                </div>
                {f.summary ? <p className="log-evt-plan-feat-summary">{f.summary}</p> : null}
              </div>
            ))}
          </div>
        </div>
      );
    case "build":
      return (
        <div className="log-evt log-evt-msg log-evt-msg-build">
          <div className="log-evt-msg-header">
            <span className="log-evt-icon">🔨</span>
            <span className="log-evt-label">{t('log.buildResult')}</span>
          </div>
          <p className="log-evt-msg-text">{payload.summary}</p>
          {payload.changedFiles.length > 0 ? (
            <div className="log-evt-msg-files">
              {payload.changedFiles.map((f, j) => (
                <code key={j}>{shortPath(f)}</code>
              ))}
            </div>
          ) : null}
        </div>
      );
    case "evaluate": {
      const isPass = payload.status === "pass";
      return (
        <div className={`log-evt log-evt-msg log-evt-msg-eval ${isPass ? "pass" : "fail"}`}>
          <div className="log-evt-msg-header">
            <span className="log-evt-icon">{isPass ? "✅" : "❌"}</span>
            <span className="log-evt-label">{t('log.evaluation', { status: payload.status })}</span>
          </div>
          <p className="log-evt-msg-text">{payload.summary}</p>
          {payload.findings.length > 0 ? (
            <ul className="log-evt-msg-findings">
              {payload.findings.map((f, j) => <li key={j}>{f}</li>)}
            </ul>
          ) : null}
        </div>
      );
    }
    case "plain":
      return (
        <div className="log-evt log-evt-msg">
          <span className="log-evt-icon">💬</span>
          <span className="log-evt-msg-text">{payload.text}</span>
        </div>
      );
  }
}

function CommandEvent({ event }: { event: Extract<LogEvent, { kind: "command" }> }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const display = stripShellWrapper(event.command);
  const hasOutput = event.output.length > 0;
  const exitOk = event.exitCode === 0;

  return (
    <div className="log-evt log-evt-cmd">
      <div className="log-evt-row" role="button" tabIndex={0} onClick={() => hasOutput && setOpen(v => !v)} onKeyDown={e => { if ((e.key === "Enter" || e.key === " ") && hasOutput) setOpen(v => !v); }}>
        <span className={`log-evt-icon ${event.status === "running" ? "spin" : exitOk ? "ok" : "err"}`}>
          {event.status === "running" ? "⟳" : exitOk ? "✓" : "✗"}
        </span>
        <code className="log-evt-cmd-text">{display}</code>
        {event.status === "done" && !exitOk ? <span className="log-evt-exit">{t('log.exit', { code: event.exitCode! })}</span> : null}
        {hasOutput ? <span className="log-evt-expand">{open ? "▾" : "▸"}</span> : null}
      </div>
      {open ? <pre className="log-evt-output">{event.output}</pre> : null}
    </div>
  );
}

function LogEventList({ events }: { events: LogEvent[] }) {
  const { t } = useTranslation();
  return (
    <div className="log-evt-list">
      {events.map((evt, i) => {
        switch (evt.kind) {
          case "session":
            return (
              <div key={i} className="log-evt log-evt-session">
                <span className="log-evt-icon">●</span>
                <span className="log-evt-label">{t('log.session')}</span>
                <code className="log-evt-detail">{evt.threadId}</code>
              </div>
            );
          case "command":
            return <CommandEvent key={i} event={evt} />;
          case "file_change":
            return (
              <div key={i} className="log-evt log-evt-file">
                <span className="log-evt-icon">✎</span>
                {evt.changes.map((c, j) => (
                  <span key={j} className="log-evt-file-item">
                    <span className={`log-evt-file-kind ${c.kind}`}>{c.kind}</span>
                    <code>{shortPath(c.path)}</code>
                  </span>
                ))}
              </div>
            );
          case "message":
            return <MessageEvent key={i} payload={evt.payload} />;
          case "usage":
            return (
              <div key={i} className="log-evt log-evt-usage">
                <span className="log-evt-icon">◈</span>
                <span className="log-evt-label">{t('log.tokens')}</span>
                <span className="log-evt-detail">
                  {t('log.inputTokens', { n: formatTokens(evt.input) })}
                  {evt.cached > 0 ? <> {t('log.cachedTokens', { n: formatTokens(evt.cached) })}</> : null}
                  {" · "}{t('log.outputTokens', { n: formatTokens(evt.output) })}
                </span>
              </div>
            );
          default:
            return null;
        }
      })}
    </div>
  );
}

function isFeatureActive(feature: FeatureRunState): boolean {
  return (
    feature.status === "building" ||
    feature.status === "evaluating" ||
    feature.status === "repairing" ||
    feature.status === "running"
  );
}

function dotClass(status: string): string {
  switch (status) {
    case "done":
    case "executed":
    case "pass":
    case "passed":
      return "nested-tl-dot done";
    case "fail":
    case "failed":
      return "nested-tl-dot fail";
    case "running":
    case "building":
    case "evaluating":
    case "repairing":
      return "nested-tl-dot running";
    default:
      return "nested-tl-dot pending";
  }
}

function dotIcon(status: string): string {
  switch (status) {
    case "done":
    case "executed":
    case "pass":
    case "passed":
      return "✓";
    case "fail":
    case "failed":
      return "✗";
    case "running":
    case "building":
    case "evaluating":
    case "repairing":
      return "◉";
    default:
      return "○";
  }
}

function featureDotClass(feature: FeatureRunState): string {
  if (
    feature.status === "done" ||
    feature.status === "passed"
  ) {
    return "nested-tl-dot done";
  }
  if (feature.status === "failed" || feature.status === "fail") {
    return "nested-tl-dot fail";
  }
  if (feature.status === "skipped") {
    return "nested-tl-dot pending";
  }
  if (isFeatureActive(feature)) {
    return "nested-tl-dot running";
  }
  return "nested-tl-dot pending";
}

function featureDotIcon(feature: FeatureRunState): string {
  if (
    feature.status === "done" ||
    feature.status === "passed"
  ) {
    return "✓";
  }
  if (feature.status === "failed" || feature.status === "fail") {
    return "✗";
  }
  if (isFeatureActive(feature)) {
    return "◉";
  }
  return "○";
}

function featureStatusLabel(feature: FeatureRunState): string {
  if (feature.status === "passed" || feature.status === "failed") {
    return feature.status;
  }
  if (feature.last_qa_status) {
    return `${feature.status} / ${feature.last_qa_status}`;
  }
  return feature.status;
}

function computeExpandedSet(features: FeatureRunState[]): Set<string> {
  const expanded = new Set<string>();
  for (const f of features) {
    if (isFeatureActive(f)) {
      expanded.add(f.feature_id);
    }
  }
  return expanded;
}

function stageLogKey(stage: RunStageRecord): string {
  return `${stage.stage}-${stage.attempt}`;
}

function StageLogPanel({
  stage,
  isLive,
}: {
  stage: RunStageRecord;
  isLive: boolean;
}) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(isLive);
  const [logContent, setLogContent] = useState("");
  const scrollRef = useRef<HTMLDivElement>(null);
  const wasAtBottomRef = useRef(true);
  const prevLiveRef = useRef(isLive);

  useEffect(() => {
    if (isLive !== prevLiveRef.current) {
      prevLiveRef.current = isLive;
      setExpanded(isLive);
    }
  }, [isLive]);

  const fetchLog = useCallback(() => {
    if (!stage.stdout_log) return;
    invoke<string>("read_stage_log", { path: stage.stdout_log }).then(
      (content) => {
        setLogContent((prev) => {
          if (content !== prev && scrollRef.current) {
            const el = scrollRef.current;
            wasAtBottomRef.current =
              el.scrollTop + el.clientHeight >= el.scrollHeight - 32;
          }
          return content;
        });
      },
      () => {},
    );
  }, [stage.stdout_log]);

  useEffect(() => {
    if (!expanded) return;
    fetchLog();
    if (!isLive) return;
    const id = setInterval(fetchLog, 1000);
    return () => clearInterval(id);
  }, [expanded, isLive, fetchLog]);

  const events = useMemo(() => parseLogEvents(logContent), [logContent]);

  useEffect(() => {
    if (expanded && scrollRef.current && wasAtBottomRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [events, expanded]);

  return (
    <div className="stage-log-wrapper">
      <button
        className="stage-log-toggle"
        onClick={() => setExpanded((v) => !v)}
      >
        <span className={`cfg-toggle-arrow${expanded ? " open" : ""}`}>▶</span>
        <span>{t('run.logs')}</span>
      </button>
      {expanded ? (
        <div ref={scrollRef} className="stage-log-content">
          {events.length > 0 ? (
            <LogEventList events={events} />
          ) : (
            <p className="log-evt-empty">{t('run.noOutput')}</p>
          )}
        </div>
      ) : null}
    </div>
  );
}

function StageNode({
  stage,
  featureId,
  isLive,
}: {
  stage: RunStageRecord;
  featureId: string;
  isLive: boolean;
}) {
  const { t } = useTranslation();
  return (
    <div
      className="nested-tl-stage-group"
      data-testid={`monitor-stage-${featureId}-${stage.stage}-${stage.attempt}`}
    >
      <div className="nested-tl-stage">
        <span className={dotClass(stage.status)}>
          {dotIcon(stage.status)}
        </span>
        <span className="nested-tl-label">{stage.stage}</span>
        <span className="workspace-meta">
          {t('timeline.attempt', { n: stage.attempt })}
        </span>
        <span className={`state-pill state-${stage.status}`}>
          {stage.status}
        </span>
      </div>
      <StageLogPanel stage={stage} isLive={isLive} />
    </div>
  );
}

function PlanStageNode({
  stage,
  planFile,
  isLive,
}: {
  stage: RunStageRecord;
  planFile: string;
  isLive: boolean;
}) {
  const { t } = useTranslation();
  const plan = useArtifact<PlanDocument>(planFile || undefined);

  return (
    <div className="nested-tl-plan" data-testid="monitor-plan-stage">
      <div className="nested-tl-feature-header">
        <span className={dotClass(stage.status)}>
          {dotIcon(stage.status)}
        </span>
        <span className="nested-tl-label">{t('run.plan')}</span>
        <span className="workspace-meta">
          {t('timeline.attempt', { n: stage.attempt })}
        </span>
        <span className={`state-pill state-${stage.status}`}>
          {stage.status}
        </span>
      </div>
      {plan ? <PlanCard data={plan} /> : null}
      <StageLogPanel stage={stage} isLive={isLive} />
      <div className="nested-tl-connector" />
    </div>
  );
}

export default function RunDetail({
  run,
  isRunning,
  isLive,
  onBack,
  onResume,
}: {
  run: RunState;
  isRunning: boolean;
  isLive: boolean;
  onBack: () => void;
  onResume: (runRoot: string) => void;
}) {
  const { t } = useTranslation();
  const [expandedFeatures, setExpandedFeatures] = useState<Set<string>>(
    () => computeExpandedSet(run.features),
  );

  useEffect(() => {
    setExpandedFeatures((prev) => {
      const next = new Set(prev);
      for (const f of run.features) {
        if (isFeatureActive(f)) {
          next.add(f.feature_id);
        }
      }
      return next;
    });
  }, [run]);

  function toggleFeature(featureId: string) {
    setExpandedFeatures((prev) => {
      const next = new Set(prev);
      if (next.has(featureId)) {
        next.delete(featureId);
      } else {
        next.add(featureId);
      }
      return next;
    });
  }

  const completedCount = run.features.filter(
    (f) => f.status === "done" || f.status === "passed" || f.status === "failed" || f.status === "skipped",
  ).length;

  return (
    <div className="run-detail" data-testid="run-detail">
      <div className="run-detail-breadcrumb">
        <button data-testid="run-detail-back" onClick={onBack}>
          {t('actions.back')}
        </button>
      </div>

      <div className="run-detail-header">
        <div className="run-detail-header-top">
          <h2>{run.run_title || basename(run.run_root)}</h2>
          <span
            className={`state-pill state-${run.lifecycle}`}
            data-testid="run-detail-lifecycle"
          >
            {run.final_status
              ? `${run.lifecycle} / ${run.final_status}`
              : run.lifecycle}
          </span>
          {isLive ? (
            <span className="live-indicator">{t('run.live')}</span>
          ) : null}
        </div>
        <p className="workspace-meta">{formatDate(run.created_at)}</p>
        <p className="workspace-meta">
          {t('timeline.feature', { current: completedCount, total: run.features.length })}
        </p>
        {run.lifecycle === "running" ? (
          <button
            className="primary-button"
            onClick={() => onResume(run.run_root)}
            disabled={isRunning}
          >
            {t('actions.resume')}
          </button>
        ) : run.lifecycle !== "completed" ? (
          <button
            className="primary-button"
            data-testid="run-detail-retry"
            onClick={() => onResume(run.run_root)}
            disabled={isRunning}
          >
            {t('actions.retry')}
          </button>
        ) : null}
      </div>

      <div className="run-detail-files">
        <FileRow label={t('run.request')} path={run.request_file} />
      </div>

      <div className="nested-tl">
        {run.plan_stage ? (
          <PlanStageNode
            stage={run.plan_stage}
            planFile={run.plan_file}
            isLive={
              isLive &&
              run.active_stage?.stage === "plan" &&
              run.active_stage?.attempt === run.plan_stage.attempt
            }
          />
        ) : null}

        {run.features.map((feature, idx) => {
          const expanded = expandedFeatures.has(feature.feature_id);
          const isLast = idx === run.features.length - 1;

          return (
            <div
              key={feature.feature_id}
              className="nested-tl-feature"
              data-testid={`monitor-feature-${feature.feature_id}`}
            >
              <div
                className="nested-tl-feature-header"
                role="button"
                tabIndex={0}
                onClick={() => toggleFeature(feature.feature_id)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    toggleFeature(feature.feature_id);
                  }
                }}
              >
                <span className={featureDotClass(feature)}>
                  {featureDotIcon(feature)}
                </span>
                <span className="nested-tl-label">
                  Feature {feature.index + 1}: "{feature.title}"
                </span>
                <span className={`state-pill state-${feature.status}`}>
                  {featureStatusLabel(feature)}
                </span>
                {feature.repair_attempts_used > 0 ? (
                  <span className="subtle-pill">
                    {t('timeline.repairs', { n: feature.repair_attempts_used })}
                  </span>
                ) : null}
                <span className="nested-tl-toggle">
                  {expanded ? "▾" : "▸"}
                </span>
              </div>

              {expanded ? (
                <div className="nested-tl-feature-stages">
                  <FeatureArtifacts feature={feature} workspacePath={run.source_workspace} />
                  {feature.stages.map((stage) => (
                    <StageNode
                      key={stageLogKey(stage)}
                      stage={stage}
                      featureId={feature.feature_id}
                      isLive={
                        isLive &&
                        run.active_stage?.feature_id === feature.feature_id &&
                        run.active_stage?.stage === stage.stage &&
                        run.active_stage?.attempt === stage.attempt
                      }
                    />
                  ))}
                </div>
              ) : null}

              {!isLast ? <div className="nested-tl-connector" /> : null}
            </div>
          );
        })}
      </div>
    </div>
  );
}
