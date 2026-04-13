type PlanFeature = {
  id: string;
  title: string;
  summary: string;
  acceptance_criteria: string;
};

export type MessagePayload =
  | { shape: "plan"; goal: string; features: PlanFeature[] }
  | { shape: "build"; summary: string; changedFiles: string[] }
  | { shape: "evaluate"; status: string; summary: string; findings: string[] }
  | { shape: "plain"; text: string };

export type LogEvent =
  | { kind: "session"; threadId: string }
  | {
      kind: "command";
      command: string;
      status: "running" | "done";
      exitCode: number | null;
      output: string;
    }
  | { kind: "file_change"; changes: { path: string; kind: string }[] }
  | { kind: "message"; payload: MessagePayload }
  | { kind: "usage"; input: number; cached: number; output: number };

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();

type StageLogStreamChunk = {
  byte_offset: number;
  content_b64: string;
};

export function encodeUtf8(text: string): Uint8Array {
  return textEncoder.encode(text);
}

export function decodeUtf8(bytes: Uint8Array): string {
  return textDecoder.decode(bytes);
}

export function applyStageLogStreamChunk(
  previous: Uint8Array,
  eventName: string,
  rawData: string,
): Uint8Array | null {
  const chunk = parseStageLogStreamChunk(rawData);
  if (!chunk) {
    return null;
  }

  if (eventName === "snapshot") {
    return chunk.bytes;
  }

  if (eventName !== "append") {
    return previous;
  }

  if (chunk.byte_offset !== previous.length) {
    return null;
  }

  return concatBytes(previous, chunk.bytes);
}

export type StageLogStreamEvent = { data: string };

export type StageLogEventSource = {
  addEventListener(
    eventName: "snapshot" | "append",
    listener: (event: StageLogStreamEvent) => void,
  ): void;
  close(): void;
  onerror: ((...args: any[]) => unknown) | null;
  readyState: number;
};

export type StageLogStreamSubscription = {
  loadStreamUrl: () => Promise<string>;
  createEventSource: (url: string) => StageLogEventSource;
  readFromDisk: () => void;
  getCurrentBytes: () => Uint8Array;
  setBytes: (bytes: Uint8Array) => void;
};

const EVENT_SOURCE_CLOSED = 2;

export function subscribeToStageLogStream(
  subscription: StageLogStreamSubscription,
): () => void {
  let cancelled = false;
  let source: StageLogEventSource | null = null;

  const handleChunk =
    (eventName: "snapshot" | "append") => (event: StageLogStreamEvent) => {
      if (cancelled) {
        return;
      }

      const nextBytes = applyStageLogStreamChunk(
        subscription.getCurrentBytes(),
        eventName,
        event.data,
      );

      if (nextBytes === null) {
        subscription.readFromDisk();
        return;
      }

      subscription.setBytes(nextBytes);
    };

  subscription.loadStreamUrl().then(
    (url) => {
      if (cancelled) {
        return;
      }

      source = subscription.createEventSource(url);
      source.addEventListener("snapshot", handleChunk("snapshot"));
      source.addEventListener("append", handleChunk("append"));
      source.onerror = () => {
        if (!cancelled && source?.readyState === EVENT_SOURCE_CLOSED) {
          subscription.readFromDisk();
        }
      };
    },
    () => {
      if (!cancelled) {
        subscription.readFromDisk();
      }
    },
  );

  return () => {
    cancelled = true;
    source?.close();
  };
}

function parseStageLogStreamChunk(
  rawData: string,
): { byte_offset: number; bytes: Uint8Array } | null {
  let parsed: StageLogStreamChunk;
  try {
    parsed = JSON.parse(rawData) as StageLogStreamChunk;
  } catch {
    return null;
  }

  if (
    !Number.isInteger(parsed.byte_offset) ||
    parsed.byte_offset < 0 ||
    typeof parsed.content_b64 !== "string"
  ) {
    return null;
  }

  const bytes = decodeBase64(parsed.content_b64);
  if (!bytes) {
    return null;
  }

  return { byte_offset: parsed.byte_offset, bytes };
}

function decodeBase64(input: string): Uint8Array | null {
  try {
    const binary = globalThis.atob(input);
    const bytes = new Uint8Array(binary.length);
    for (let index = 0; index < binary.length; index += 1) {
      bytes[index] = binary.charCodeAt(index);
    }
    return bytes;
  } catch {
    return null;
  }
}

function concatBytes(left: Uint8Array, right: Uint8Array): Uint8Array {
  const joined = new Uint8Array(left.length + right.length);
  joined.set(left, 0);
  joined.set(right, left.length);
  return joined;
}

export function parseLogEvents(raw: string): LogEvent[] {
  const events: LogEvent[] = [];
  const pendingCmds = new Map<string, number>();
  const seenFileChanges = new Set<string>();
  for (const line of raw.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    let obj: Record<string, unknown>;
    try {
      obj = JSON.parse(trimmed) as Record<string, unknown>;
    } catch {
      continue;
    }
    const type = obj.type as string | undefined;
    const item = (obj.item ?? {}) as Record<string, unknown>;
    const itemType = item.type as string | undefined;
    const itemId = item.id as string | undefined;

    if (type === "thread.started") {
      const threadId = (obj.thread_id ?? obj.session_id ?? "") as string;
      if (threadId) events.push({ kind: "session", threadId });
    } else if (type === "item.started" && itemType === "command_execution") {
      const cmd = (item.command ?? "") as string;
      events.push({
        kind: "command",
        command: cmd,
        status: "running",
        exitCode: null,
        output: "",
      });
      if (itemId) pendingCmds.set(itemId, events.length - 1);
    } else if (type === "item.completed" && itemType === "command_execution") {
      const cmd = (item.command ?? "") as string;
      const exitCode = (item.exit_code ?? null) as number | null;
      const output = (item.aggregated_output ?? "") as string;
      if (itemId && pendingCmds.has(itemId)) {
        const idx = pendingCmds.get(itemId)!;
        events[idx] = {
          kind: "command",
          command: cmd,
          status: "done",
          exitCode,
          output,
        };
        pendingCmds.delete(itemId);
      } else {
        events.push({
          kind: "command",
          command: cmd,
          status: "done",
          exitCode,
          output,
        });
      }
    } else if (
      (type === "item.started" || type === "item.completed") &&
      itemType === "file_change"
    ) {
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
            features: (parsed.features as PlanFeature[]).map((feature) => ({
              id: feature.id ?? "",
              title: feature.title ?? "",
              summary: feature.summary ?? "",
              acceptance_criteria: feature.acceptance_criteria ?? "",
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
