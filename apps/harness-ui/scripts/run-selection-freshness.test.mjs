import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import ts from "typescript";

const moduleUnderTest = await loadModule(
  path.resolve(import.meta.dirname, "../src/runSelectionFreshness.ts"),
);

const {
  canCommitInspectRunResponse,
  createInspectRunRequest,
  createSelectedRunInspectTarget,
  invalidateInspectRunRequestIfCurrent,
  updateSelectedRunInspectTarget,
} = moduleUnderTest;

test("unchanged selections preserve generation while changed selections bump it", () => {
  const initial = createSelectedRunInspectTarget();
  assert.equal(initial.generation, 0);

  const firstSelection = updateSelectedRunInspectTarget(
    initial,
    "/tmp/workspace-a",
    "/tmp/run-a",
  );
  assert.equal(firstSelection.generation, 1);

  const unchangedSelection = updateSelectedRunInspectTarget(
    firstSelection,
    "/tmp/workspace-a",
    "/tmp/run-a",
  );
  assert.equal(unchangedSelection, firstSelection);
  assert.equal(unchangedSelection.generation, 1);

  const changedRun = updateSelectedRunInspectTarget(
    unchangedSelection,
    "/tmp/workspace-a",
    "/tmp/run-b",
  );
  assert.equal(changedRun.generation, 2);

  const changedWorkspace = updateSelectedRunInspectTarget(
    changedRun,
    "/tmp/workspace-b",
    "/tmp/run-b",
  );
  assert.equal(changedWorkspace.generation, 3);
});

test("inspect requests are only created for the current selection", () => {
  const target = updateSelectedRunInspectTarget(
    createSelectedRunInspectTarget(),
    "/tmp/workspace-a",
    "/tmp/run-a",
  );

  assert.deepEqual(createInspectRunRequest(target, "/tmp/run-a"), {
    workspacePath: "/tmp/workspace-a",
    runRoot: "/tmp/run-a",
    generation: 1,
  });
  assert.equal(createInspectRunRequest(target, "/tmp/run-b"), null);

  const clearedSelection = updateSelectedRunInspectTarget(
    target,
    "/tmp/workspace-a",
    null,
  );
  assert.equal(createInspectRunRequest(clearedSelection, "/tmp/run-a"), null);
});

test("stale responses are rejected after selection changes and invalidation", () => {
  let target = updateSelectedRunInspectTarget(
    createSelectedRunInspectTarget(),
    "/tmp/workspace-a",
    "/tmp/run-a",
  );
  const requestA = expectRequest(createInspectRunRequest(target, "/tmp/run-a"));
  assert.equal(canCommitInspectRunResponse(target, requestA, "/tmp/run-a"), true);

  target = updateSelectedRunInspectTarget(target, "/tmp/workspace-a", "/tmp/run-b");
  assert.equal(canCommitInspectRunResponse(target, requestA, "/tmp/run-a"), false);

  const requestB = expectRequest(createInspectRunRequest(target, "/tmp/run-b"));
  assert.equal(canCommitInspectRunResponse(target, requestB, "/tmp/run-b"), true);

  const invalidated = invalidateInspectRunRequestIfCurrent(
    target,
    "/tmp/workspace-a",
    "/tmp/run-b",
  );
  assert.equal(invalidated.generation, target.generation + 1);
  assert.equal(canCommitInspectRunResponse(invalidated, requestB, "/tmp/run-b"), false);

  const refreshedRequest = expectRequest(
    createInspectRunRequest(invalidated, "/tmp/run-b"),
  );
  assert.equal(
    canCommitInspectRunResponse(invalidated, refreshedRequest, "/tmp/run-b"),
    true,
  );

  const untouched = invalidateInspectRunRequestIfCurrent(
    invalidated,
    "/tmp/workspace-a",
    "/tmp/run-c",
  );
  assert.equal(untouched, invalidated);
});

function expectRequest(value) {
  assert.notEqual(value, null);
  return value;
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
