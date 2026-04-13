import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import ts from "typescript";

const moduleUnderTest = await loadModule(
  path.resolve(import.meta.dirname, "../src/runDetailLogModel.ts"),
);

const {
  applyStageLogStreamChunk,
  decodeUtf8,
  encodeUtf8,
  parseLogEvents,
  subscribeToStageLogStream,
} = moduleUnderTest;

test("streamed JSONL updates keep command, file-change, and message rendering intact", () => {
  let buffer = new Uint8Array();

  const startedCommand = line({
    type: "item.started",
    item: {
      id: "cmd-1",
      type: "command_execution",
      command: "/bin/sh -lc 'cargo test'",
    },
  });

  buffer = expectChunk(
    applyStageLogStreamChunk(buffer, "snapshot", chunkPayload(0, startedCommand)),
  );

  let events = parseLogEvents(decodeUtf8(buffer));
  assert.equal(events.length, 1);
  assert.equal(events[0]?.kind, "command");
  assert.equal(events[0]?.status, "running");

  const completedCommand = line({
    type: "item.completed",
    item: {
      id: "cmd-1",
      type: "command_execution",
      command: "/bin/sh -lc 'cargo test'",
      exit_code: 0,
      aggregated_output: "ok\n",
    },
  });
  const fileChange = line({
    type: "item.completed",
    item: {
      id: "fc-1",
      type: "file_change",
      changes: [{ path: "/tmp/example.rs", kind: "modified" }],
    },
  });
  const agentMessage = line({
    type: "item.completed",
    item: {
      id: "msg-1",
      type: "agent_message",
      text: JSON.stringify({
        summary: "updated the streaming reducer",
        changed_files: ["apps/harness-ui/src/RunDetail.tsx"],
      }),
    },
  });

  const appendPayload = completedCommand + fileChange + agentMessage;
  buffer = expectChunk(
    applyStageLogStreamChunk(
      buffer,
      "append",
      chunkPayload(buffer.length, appendPayload),
    ),
  );

  events = parseLogEvents(decodeUtf8(buffer));
  assert.equal(events.length, 3);
  assert.equal(events[0]?.kind, "command");
  assert.equal(events[0]?.status, "done");
  assert.equal(events[0]?.output, "ok\n");
  assert.deepEqual(events[1], {
    kind: "file_change",
    changes: [{ path: "/tmp/example.rs", kind: "modified" }],
  });
  assert.deepEqual(events[2], {
    kind: "message",
    payload: {
      shape: "build",
      summary: "updated the streaming reducer",
      changedFiles: ["apps/harness-ui/src/RunDetail.tsx"],
    },
  });
});

test("snapshot replacement drops stale content and append offsets are byte-based", () => {
  const previous = encodeUtf8('{"type":"thread.started","thread_id":"old"}\n');
  const replacement = "你好\n";

  const buffer = expectChunk(
    applyStageLogStreamChunk(
      previous,
      "snapshot",
      chunkPayload(0, replacement),
    ),
  );

  assert.equal(decodeUtf8(buffer), replacement);

  const mismatched = applyStageLogStreamChunk(
    buffer,
    "append",
    chunkPayload(replacement.length, "later\n"),
  );
  assert.equal(mismatched, null);

  const appended = expectChunk(
    applyStageLogStreamChunk(
      buffer,
      "append",
      chunkPayload(buffer.length, "later\n"),
    ),
  );
  assert.equal(decodeUtf8(appended), "你好\nlater\n");
});

test("closed SSE stream falls back to disk reads", async () => {
  let buffer = new Uint8Array();
  let diskReads = 0;
  let source = null;

  const unsubscribe = subscribeToStageLogStream({
    loadStreamUrl: async () => "http://127.0.0.1:7777/stage-log",
    createEventSource(url) {
      source = new FakeEventSource(url);
      return source;
    },
    readFromDisk() {
      diskReads += 1;
    },
    getCurrentBytes() {
      return buffer;
    },
    setBytes(nextBytes) {
      buffer = nextBytes;
    },
  });

  await flushPromises();
  assert.notEqual(source, null);
  assert.equal(source.url, "http://127.0.0.1:7777/stage-log");

  source.emit("snapshot", chunkPayload(0, "alpha\n"));
  assert.equal(decodeUtf8(buffer), "alpha\n");

  source.readyState = 2;
  source.onerror?.();
  assert.equal(diskReads, 1);

  unsubscribe();
  assert.equal(source.closed, true);
});

test("stream URL resolution failure falls back to disk reads", async () => {
  let diskReads = 0;

  subscribeToStageLogStream({
    loadStreamUrl: async () => {
      throw new Error("offline");
    },
    createEventSource() {
      throw new Error("should not create event source when URL lookup fails");
    },
    readFromDisk() {
      diskReads += 1;
    },
    getCurrentBytes() {
      return new Uint8Array();
    },
    setBytes() {},
  });

  await flushPromises();
  assert.equal(diskReads, 1);
});

function line(value) {
  return `${JSON.stringify(value)}\n`;
}

function chunkPayload(byteOffset, content) {
  return JSON.stringify({
    byte_offset: byteOffset,
    content_b64: Buffer.from(content, "utf8").toString("base64"),
  });
}

function expectChunk(value) {
  assert.notEqual(value, null);
  return value;
}

async function flushPromises() {
  await Promise.resolve();
  await Promise.resolve();
}

class FakeEventSource {
  constructor(url) {
    this.url = url;
    this.readyState = 1;
    this.closed = false;
    this.onerror = null;
    this.listeners = new Map();
  }

  addEventListener(eventName, listener) {
    this.listeners.set(eventName, listener);
  }

  emit(eventName, data) {
    const listener = this.listeners.get(eventName);
    assert.ok(listener, `missing listener for ${eventName}`);
    listener({ data });
  }

  close() {
    this.closed = true;
    this.readyState = 2;
  }
}

async function loadModule(filePath) {
  const source = await readFile(filePath, "utf8");
  const transpiled = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.ES2020,
      target: ts.ScriptTarget.ES2020,
    },
    fileName: filePath,
  });

  const moduleUrl = `data:text/javascript;base64,${Buffer.from(
    transpiled.outputText,
    "utf8",
  ).toString("base64")}`;
  return import(moduleUrl);
}
