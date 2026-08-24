import assert from "node:assert/strict";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { dispatchMembraneHookEvent } from "./membrane-hook-runtime.mjs";
import { createWorkspaceMemoryOperations, durableWorkspaceFile } from "./membrane-workspace-operations.mjs";
import { runHook } from "./membrane-hook-entrypoint.mjs";

function fixture() {
  const root = mkdtempSync(join(tmpdir(), "membrane-hook-"));
  mkdirSync(join(root, "memory"), { recursive: true });
  const file = join(root, "memory", "note.md");
  writeFileSync(file, "memory", "utf8");
  return { root, file };
}

function operations(root, calls) {
  return createWorkspaceMemoryOperations({
    rootFor: () => root,
    probeStatus: async () => { calls.push("status"); return true; },
    contextAdapter: {
      buildRequest: (payload, receivedRoot) => { calls.push(`recall:${receivedRoot}`); return { task: payload.prompt }; },
      runClient: () => ({ state: "context_enforced", payload: { packet: {} } }),
      render: () => "recalled context",
    },
    runCortex: async (_root, args) => { calls.push(args.join(" ")); return true; },
  });
}

test("production entrypoint dispatches Membrane-owned memory operations", async () => {
  const { root, file } = fixture(); const calls = [];
  const result = await runHook({ hook_event_name: "UserPromptSubmit", cwd: root, prompt: "inspect" }, { operations: operations(root, calls) });
  assert.deepEqual(calls, [`recall:${root}`]);
  assert.equal(result.hookSpecificOutput.additionalContext, "recalled context");
  await dispatchMembraneHookEvent({ event: "PreCompact", cwd: root, session_id: "s" }, operations(root, calls));
  await dispatchMembraneHookEvent({ event: "PostCompact", cwd: root, session_id: "s", compact_summary: "summary" }, operations(root, calls));
  await dispatchMembraneHookEvent({ event: "PostToolUse", cwd: root, tool_name: "Write", tool_input: { file_path: file } }, operations(root, calls));
  assert.ok(calls.includes("checkpoint save"));
  assert.ok(calls.some((value) => value.startsWith("put note --scope global --file")));
  assert.ok(calls.some((value) => value.includes("--producer membrane_hook --record-type markdown_memory --authority A0 --influence-class data_only")));
});

test("production entrypoint executes as a cross-platform Node script", () => {
  const entrypoint = new URL("./membrane-hook-entrypoint.mjs", import.meta.url);
  const child = spawnSync(process.execPath, [fileURLToPath(entrypoint)], {
    encoding: "utf8",
    input: JSON.stringify({ hook_event_name: "Unknown", cwd: process.cwd() }),
  });
  assert.equal(child.status, 0, child.stderr);
  assert.equal(JSON.parse(child.stdout).hookSpecificOutput.hookEventName, "Unknown");
});

test("ingest accepts only durable memory & Blueprint artifacts, including host memory", () => {
  const { root, file } = fixture();
  const unrelated = join(root, "README.md");
  writeFileSync(unrelated, "not durable", "utf8");
  const home = mkdtempSync(join(tmpdir(), "membrane-home-"));
  const slug = root.replaceAll(":", "-").replaceAll("\\", "-").replaceAll("/", "-");
  const hostMemory = join(home, ".claude", "projects", slug, "memory", "host.md");
  mkdirSync(join(hostMemory, ".."), { recursive: true });
  writeFileSync(hostMemory, "memory", "utf8");
  assert.equal(durableWorkspaceFile(root, file, home), file);
  assert.equal(durableWorkspaceFile(root, hostMemory, home), hostMemory);
  assert.equal(durableWorkspaceFile(root, unrelated, home), null);
});

test("checkpoint summaries redact secret-bearing lines before Cortex", async () => {
  const { root } = fixture(); const inputs = [];
  const ops = createWorkspaceMemoryOperations({
    rootFor: () => root,
    contextAdapter: {}, probeStatus: async () => true,
    runCortex: async (_root, _args, input) => { inputs.push(input); return true; },
  });
  await dispatchMembraneHookEvent({ event: "PreCompact", cwd: root, session_id: "redact" }, ops);
  await dispatchMembraneHookEvent({ event: "PostCompact", cwd: root, session_id: "redact", compact_summary: "safe\ntoken=private\nsafe too" }, ops);
  assert.equal(JSON.parse(inputs[0]).summary, "safe\n[redacted sensitive summary line]\nsafe too");
});

test("installed operations degrade typed without an injected Cortex subprocess seam", async () => {
  const { root, file } = fixture();
  const ops = createWorkspaceMemoryOperations({ rootFor: () => root, contextAdapter: {}, probeStatus: async () => false });
  await dispatchMembraneHookEvent({ event: "PreCompact", cwd: root, session_id: "no-subprocess" }, ops);
  const compact = await dispatchMembraneHookEvent({ event: "PostCompact", cwd: root, session_id: "no-subprocess" }, ops);
  assert.equal(compact.results.find(({ id }) => id === "membrane.memory-post-compact").output.reason, "continuity_service_unavailable");
  const ingest = await dispatchMembraneHookEvent({ event: "PostToolUse", cwd: root, tool_name: "Write", tool_input: { file_path: file } }, ops);
  assert.equal(ingest.results.find(({ id }) => id === "membrane.memory-ingest").output.reason, "memory_service_unavailable");
});

test("SessionStart performs health only & never starts or kicks Cortex", async () => {
  const { root } = fixture(); const calls = [];
  const result = await runHook({ hook_event_name: "SessionStart", cwd: root }, { operations: operations(root, calls) });
  assert.deepEqual(calls, ["status"]);
  assert.equal(result.membraneHook.results[0].output.detail.lifecycle, "hub-child");
  assert.equal(result.membraneHook.results[1].output.reason, "event_not_applicable");
  const manifest = JSON.parse(readFileSync(new URL("../../package.json", import.meta.url), "utf8"));
  assert.equal(manifest.bin["membrane-workspace-hook"], "mcp/hooks/membrane-hook-entrypoint.mjs");
});

test("Membrane owns rearm, access bump, conflict, observer, & nag behavior", async () => {
  const { root } = fixture();
  const home = mkdtempSync(join(tmpdir(), "membrane-home-"));
  const slug = root.replaceAll(":", "-").replaceAll("\\", "-").replaceAll("/", "-");
  const memory = join(home, ".claude", "projects", slug, "memory");
  mkdirSync(memory, { recursive: true });
  const target = join(memory, "target.md");
  writeFileSync(target, "---\nname: Target\ndomain: seam\nlast_accessed: 2020-01-01\n---\nbody\n", "utf8");
  writeFileSync(join(memory, "sibling.md"), "---\nname: Sibling\ndomain: seam\n---\nbody\n", "utf8");
  const session = "session-one";
  const traceDir = join(root, "tools", ".cache", "memory", "active-traces");
  mkdirSync(traceDir, { recursive: true });
  const { createHash } = await import("node:crypto");
  writeFileSync(join(traceDir, `${createHash("sha256").update(session).digest("hex")}.json`), '{"trace_id":"trace-one"}\n', "utf8");
  const transcript = join(root, "transcript.jsonl");
  writeFileSync(transcript, `${JSON.stringify({ type: "user", message: { content: "From now on use this process." } })}\n`, "utf8");
  const observed = [];
  const ops = createWorkspaceMemoryOperations({ rootFor: () => root, home, contextAdapter: {}, probeStatus: async () => true,
    runCortex: async () => true, postObservation: async (_root, _event, trace) => { observed.push(trace); return true; } });

  const database = join(root, "cortex-engine.db");
  const seen = join(root, "recall-seen", `${session}.json`);
  mkdirSync(join(seen, ".."), { recursive: true });
  writeFileSync(seen, "{}\n", "utf8");
  const priorDatabase = process.env.CORTEX_DB;
  process.env.CORTEX_DB = database;
  const rearm = await dispatchMembraneHookEvent({ event: "SessionStart", source: "compact", session_id: session }, ops);
  if (priorDatabase === undefined) delete process.env.CORTEX_DB; else process.env.CORTEX_DB = priorDatabase;
  assert.equal(rearm.results.find(({ id }) => id === "membrane.memory-rearm").output.reason, "recall_rearmed");
  assert.equal(existsSync(seen), false);

  const bump = await dispatchMembraneHookEvent({ event: "PreToolUse", tool_name: "Read", tool_input: { file_path: target } }, ops);
  assert.equal(bump.results.find(({ id }) => id === "membrane.memory-bump").output.reason, "memory_access_bumped");
  assert.match(readFileSync(target, "utf8"), new RegExp(`last_accessed: ${new Date().toISOString().slice(0, 10)}`));

  const conflict = await runHook({ event: "PreToolUse", tool_name: "Write", tool_input: { file_path: target, content: "---\nname: New\ndomain: seam\n---\n" } }, { operations: ops });
  assert.match(conflict.hookSpecificOutput.additionalContext, /same-domain sibling/);

  const observer = await dispatchMembraneHookEvent({ event: "PostToolUse", session_id: session, tool_name: "Bash", tool_response: { ok: true } }, ops);
  assert.equal(observer.results.find(({ id }) => id === "membrane.tool-observer").output.reason, "tool_observed");
  assert.deepEqual(observed, ["trace-one"]);

  const nag = await runHook({ event: "Stop", transcript_path: transcript }, { operations: ops });
  assert.match(nag.hookSpecificOutput.additionalContext, /durable correction/);
});

test("PostToolUseFailure, TaskCompleted, and SessionEnd are covered with typed, bounded, secret-safe handlers", async () => {
  const { root } = fixture();
  const ops = createWorkspaceMemoryOperations({
    rootFor: () => root,
    contextAdapter: {},
    probeStatus: async () => true,
    runCortex: async () => true,
  });

  const failure = await dispatchMembraneHookEvent({
    event: "PostToolUseFailure",
    session_id: "s",
    tool_name: "Bash",
    error: { message: "failed while api_key=SECRET token=hunter2" },
  }, ops);
  const failureOut = failure.results.find(({ id }) => id === "membrane.memory-failure").output;
  assert.equal(failureOut.reason, "failure_observed");
  assert.equal(failureOut.detail.contentFree, true);
  assert.ok(!failureOut.detail.summaryLength || failureOut.detail.summaryLength >= 0);

  const episode = await dispatchMembraneHookEvent({
    event: "TaskCompleted",
    session_id: "s",
    outcomes: [{ id: "a", ok: true }],
  }, ops);
  const episodeOut = episode.results.find(({ id }) => id === "membrane.memory-episode").output;
  assert.equal(episodeOut.reason, "episode_captured");
  assert.match(episodeOut.detail.outcomeDigest, /^sha256:[0-9a-f]{64}$/);
  assert.equal(episodeOut.detail.contentFree, true);

  const sessionEnd = await dispatchMembraneHookEvent({ event: "SessionEnd", session_id: "s" }, ops);
  const sessionEndOut = sessionEnd.results.find(({ id }) => id === "membrane.memory-session-end").output;
  assert.equal(sessionEndOut.reason, "session_closed");
});

function fenceFixture() {
  const base = fixture();
  mkdirSync(join(base.root, ".agent"), { recursive: true });
  const init = spawnSync("git", ["init", "-q", base.root], { encoding: "utf8" });
  assert.equal(init.status, 0, init.stderr);
  const add = spawnSync("git", ["-C", base.root, "add", "-A"], { encoding: "utf8" });
  assert.equal(add.status, 0, add.stderr);
  const commit = spawnSync("git", ["-C", base.root, "-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "fixture"], { encoding: "utf8" });
  assert.equal(commit.status, 0, commit.stderr);
  return base;
}

test("fence enforcement is opt-in and skipped without env or marker", async () => {
  const { root } = fenceFixture();
  const ops = createWorkspaceMemoryOperations({ rootFor: () => root });
  delete process.env.MEMBRANE_DIAGNOSTICS_ENFORCE;
  try {
    const result = await runHook({ event: "PreToolUse", tool_name: "Bash", tool_input: { command: "pnpm test" }, cwd: root }, { operations: ops });
    assert.equal(result.membraneHook.results.find(({ id }) => id === "membrane.diagnostics-fence").output.state, "skipped");
    assert.equal(result.decision, undefined, "un-enforced workspaces must never be denied");
  } finally {
    delete process.env.MEMBRANE_DIAGNOSTICS_ENFORCE;
  }
});

test("enforced PreToolUse boundary denies tests/builds while the fence is uncleared (fail-closed)", async () => {
  const { root } = fenceFixture();
  process.env.MEMBRANE_DIAGNOSTICS_ENFORCE = "1";
  try {
    const unreachableOps = createWorkspaceMemoryOperations({
      rootFor: () => root,
      diagnosticsPost: async () => { throw new Error("resident down"); },
    });
    const blockedUnreachable = await runHook(
      { hook_event_name: "PreToolUse", tool_name: "Bash", tool_input: { command: "pnpm test" }, cwd: root },
      { operations: unreachableOps },
    );
    assert.equal(blockedUnreachable.decision, "block", "fail-closed: resident unreachable must deny the boundary");
    assert.equal(blockedUnreachable.hookSpecificOutput.permissionDecision, "deny");

    // Workspace not open (status not ok) must also block, not skip.
    const notOpenOps = createWorkspaceMemoryOperations({
      rootFor: () => root,
      diagnosticsPost: async () => ({ ok: false, status: 404, error: { code: "workspace_not_open", detail: "not open" } }),
    });
    const blockedNotOpen = await runHook(
      { hook_event_name: "PreToolUse", tool_name: "Bash", tool_input: { command: "cargo build" }, cwd: root },
      { operations: notOpenOps },
    );
    assert.equal(blockedNotOpen.decision, "block");
    assert.equal(blockedNotOpen.hookSpecificOutput.permissionDecision, "deny");

    // Fence not cleared must block.
    const unclearedOps = createWorkspaceMemoryOperations({
      rootFor: () => root,
      diagnosticsPost: async (path) => ({
        ok: true,
        body: path.includes("/workspace/status") ? { fenceCleared: false, latestSealedEpoch: 3, projectRoot: root } : {},
      }),
    });
    const blockedUncleared = await runHook(
      { hook_event_name: "PreToolUse", tool_name: "Bash", tool_input: { command: "pnpm test" }, cwd: root },
      { operations: unclearedOps },
    );
    assert.equal(blockedUncleared.decision, "block");

    // A cleared fence allows the same command through.
    const clearedOps = createWorkspaceMemoryOperations({
      rootFor: () => root,
      diagnosticsPost: async (path) => ({
        ok: true,
        body: path.includes("/workspace/status") ? { fenceCleared: true, latestSealedEpoch: 3, projectRoot: root }
          : path.includes("/diagnostics/reconcile") ? { classification: "cleared" } : {},
      }),
    });
    const allowed = await runHook(
      { hook_event_name: "PreToolUse", tool_name: "Bash", tool_input: { command: "cargo build" }, cwd: root },
      { operations: clearedOps },
    );
    assert.equal(allowed.membraneHook.results.find(({ id }) => id === "membrane.diagnostics-fence").output.state, "available");
    assert.equal(allowed.decision, undefined);

    // Non-verification commands never reach the fence gate: the module
    // matcher skips them before the operation is invoked.
    const untouched = await runHook(
      { hook_event_name: "PreToolUse", tool_name: "Bash", tool_input: { command: "ls -la" }, cwd: root },
      { operations: unreachableOps },
    );
    assert.equal(untouched.membraneHook.results.find(({ id }) => id === "membrane.diagnostics-fence").output.reason, "event_not_applicable");
    assert.equal(untouched.decision, undefined);
    const catUntouched = await runHook(
      { hook_event_name: "PreToolUse", tool_name: "Bash", tool_input: { command: "cat src/main.ts" }, cwd: root },
      { operations: unreachableOps },
    );
    assert.equal(catUntouched.membraneHook.results.find(({ id }) => id === "membrane.diagnostics-fence").output.reason, "event_not_applicable");
    assert.equal(catUntouched.decision, undefined);
  } finally {
    delete process.env.MEMBRANE_DIAGNOSTICS_ENFORCE;
  }
});

test("Stop completion boundary blocks when fence not cleared (fail-closed)", async () => {
  const { root } = fenceFixture();
  writeFileSync(join(root, ".agent", "diagnostics-enforce.json"), "{}\n", "utf8");
  const blockedOps = createWorkspaceMemoryOperations({
    rootFor: () => root,
    diagnosticsPost: async (path) => ({
      ok: true,
      body: path.includes("/workspace/status") ? { fenceCleared: false, latestSealedEpoch: 7, projectRoot: root } : {},
    }),
  });
  const blocked = await runHook({ hook_event_name: "Stop", cwd: root }, { operations: blockedOps });
  assert.equal(blocked.decision, "block");
  assert.equal(blocked.hookSpecificOutput.hookEventName, "Stop");

  // Fail-closed: even with no sealed epoch, an opted-in workspace blocks at completion when fence is not cleared.
  const noEpochOps = createWorkspaceMemoryOperations({
    rootFor: () => root,
    diagnosticsPost: async (path) => ({
      ok: true,
      body: path.includes("/workspace/status") ? { fenceCleared: false, latestSealedEpoch: null, projectRoot: root } : {},
    }),
  });
  const blockedNoEpoch = await runHook({ hook_event_name: "Stop", cwd: root }, { operations: noEpochOps });
  assert.equal(blockedNoEpoch.decision, "block");
  assert.equal(blockedNoEpoch.hookSpecificOutput.permissionDecision, "deny");

  // Clean clearance allows completion.
    const clearedOps = createWorkspaceMemoryOperations({
      rootFor: () => root,
      diagnosticsPost: async (path) => ({
        ok: true,
        body: path.includes("/workspace/status") ? { fenceCleared: true, latestSealedEpoch: 7, projectRoot: root }
          : path.includes("/diagnostics/reconcile") ? { classification: "cleared" } : {},
      }),
  });
  const allowed = await runHook({ hook_event_name: "Stop", cwd: root }, { operations: clearedOps });
  assert.equal(allowed.decision, undefined);
  assert.equal(allowed.membraneHook.results.find(({ id }) => id === "membrane.diagnostics-completion-fence").output.state, "available");
});

test("fence verifies bound projectRoot matches current root (fail-closed on mismatch/missing)", async () => {
  const { root: oldRoot } = fenceFixture();
  const { root: newRoot } = fenceFixture();
  // Mismatched root must BLOCK even when fenceCleared true
  const mismatchOps = createWorkspaceMemoryOperations({
    rootFor: () => newRoot,
    diagnosticsPost: async (path) => ({
      ok: true,
      body: path.includes("/workspace/status") ? { fenceCleared: true, latestSealedEpoch: 5, projectRoot: oldRoot } : {},
    }),
  });
  process.env.MEMBRANE_DIAGNOSTICS_ENFORCE = "1";
  try {
    const blocked = await runHook({ hook_event_name: "PreToolUse", tool_name: "Bash", tool_input: { command: "pnpm test" }, cwd: newRoot }, { operations: mismatchOps });
    assert.equal(blocked.decision, "block");
    assert.equal(blocked.hookSpecificOutput.permissionDecision, "deny");
  } finally { delete process.env.MEMBRANE_DIAGNOSTICS_ENFORCE; }

  // Missing projectRoot must BLOCK
  const missingOps = createWorkspaceMemoryOperations({
    rootFor: () => newRoot,
    diagnosticsPost: async (path) => ({
      ok: true,
      body: path.includes("/workspace/status") ? { fenceCleared: true, latestSealedEpoch: 5 } : {},
    }),
  });
  process.env.MEMBRANE_DIAGNOSTICS_ENFORCE = "1";
  try {
    const blockedMissing = await runHook({ hook_event_name: "PreToolUse", tool_name: "Bash", tool_input: { command: "pnpm test" }, cwd: newRoot }, { operations: missingOps });
    assert.equal(blockedMissing.decision, "block");
  } finally { delete process.env.MEMBRANE_DIAGNOSTICS_ENFORCE; }

  // Matching canonical root + fenceCleared true still allows
    const matchingOps = createWorkspaceMemoryOperations({
      rootFor: () => newRoot,
      diagnosticsPost: async (path) => ({
        ok: true,
        body: path.includes("/workspace/status") ? { fenceCleared: true, latestSealedEpoch: 5, projectRoot: newRoot }
          : path.includes("/diagnostics/reconcile") ? { classification: "cleared" } : {},
      }),
  });
  process.env.MEMBRANE_DIAGNOSTICS_ENFORCE = "1";
  try {
    const allowed = await runHook({ hook_event_name: "PreToolUse", tool_name: "Bash", tool_input: { command: "pnpm test" }, cwd: newRoot }, { operations: matchingOps });
    assert.equal(allowed.decision, undefined);
    assert.equal(allowed.membraneHook.results.find(({ id }) => id === "membrane.diagnostics-fence").output.state, "available");
  } finally { delete process.env.MEMBRANE_DIAGNOSTICS_ENFORCE; }
});

async function runEnforcedBoundary(root, diagnosticsPost) {
  const prior = process.env.MEMBRANE_DIAGNOSTICS_ENFORCE;
  process.env.MEMBRANE_DIAGNOSTICS_ENFORCE = "1";
  try {
    const operations = createWorkspaceMemoryOperations({ rootFor: () => root, diagnosticsPost });
    return await runHook({ event: "PreToolUse", tool_name: "Bash", tool_input: { command: "pnpm test" }, cwd: root }, { operations });
  } finally {
    if (prior === undefined) delete process.env.MEMBRANE_DIAGNOSTICS_ENFORCE;
    else process.env.MEMBRANE_DIAGNOSTICS_ENFORCE = prior;
  }
}

test("enforced boundary reconciles an unchanged current manifest", async () => {
  const { root } = fenceFixture();
  const calls = [];
  const result = await runEnforcedBoundary(root, async (path, options) => {
    calls.push([path, options]);
    if (path.includes("/workspace/status")) return { ok: true, body: { fenceCleared: true, projectRoot: root } };
    if (path === "/diagnostics/reconcile") return { ok: true, body: { classification: "cleared" } };
    return { ok: false, error: { code: "unexpected" } };
  });
  assert.equal(result.decision, undefined);
  const reconcile = calls.find(([path]) => path === "/diagnostics/reconcile");
  assert.ok(reconcile);
  assert.deepEqual(reconcile[1].body.hashes, []);
  assert.match(reconcile[1].body.manifestDigest, /^sha256:/);
});

test("unobserved modification, addition, & deletion block after clean clearance", async () => {
  for (const change of ["modify", "add", "delete"]) {
    const { root } = fenceFixture();
    const target = join(root, "memory", "note.md");
    if (change === "modify") writeFileSync(target, "changed\n", "utf8");
    if (change === "add") writeFileSync(join(root, "new-file.txt"), "new\n", "utf8");
    if (change === "delete") {
      const { unlinkSync: remove } = await import("node:fs");
      remove(target);
    }
    const result = await runEnforcedBoundary(root, async (path, options) => {
      if (path.includes("/workspace/status")) return { ok: true, body: { fenceCleared: true, projectRoot: root } };
      if (path === "/diagnostics/reconcile") {
        assert.ok(options.body.manifestDigest.startsWith("sha256:"));
        return { ok: true, body: { classification: "unknown_conflict" } };
      }
      return { ok: false, error: { code: "unexpected" } };
    });
    assert.equal(result.decision, "block", change);
  }
});

test("manifest generation failure blocks before reconciliation", async () => {
  const { root } = fixture();
  const calls = [];
  const result = await runEnforcedBoundary(root, async (path) => {
    calls.push(path);
    if (path.includes("/workspace/status")) return { ok: true, body: { fenceCleared: true, projectRoot: root } };
    throw new Error("unexpected reconciliation");
  });
  assert.equal(result.decision, "block");
  assert.equal(calls.filter((path) => path === "/diagnostics/reconcile").length, 0);
});

test("reconciliation endpoint failure blocks", async () => {
  const { root } = fenceFixture();
  const result = await runEnforcedBoundary(root, async (path) => {
    if (path.includes("/workspace/status")) return { ok: true, body: { fenceCleared: true, projectRoot: root } };
    throw new Error("resident down");
  });
  assert.equal(result.decision, "block");
});

test("disabled enforcement does not inspect Git or reconcile", async () => {
  const { root } = fixture();
  const calls = [];
  const prior = process.env.MEMBRANE_DIAGNOSTICS_ENFORCE;
  delete process.env.MEMBRANE_DIAGNOSTICS_ENFORCE;
  try {
    const operations = createWorkspaceMemoryOperations({ rootFor: () => root, diagnosticsPost: async (path) => {
      calls.push(path);
      throw new Error("must not call diagnostics");
    } });
    const result = await runHook({ event: "PreToolUse", tool_name: "Bash", tool_input: { command: "pnpm test" }, cwd: root }, { operations });
    assert.equal(result.decision, undefined);
    assert.equal(calls.length, 0);
  } finally {
    if (prior !== undefined) process.env.MEMBRANE_DIAGNOSTICS_ENFORCE = prior;
  }
});

test("observeMutation binds the workspace to its canonical root before registering", async () => {
  const { root } = fenceFixture();
  const file = join(root, "src", "edited.ts");
  mkdirSync(join(root, "src"), { recursive: true });
  writeFileSync(file, "export {};\n", "utf8");
  const calls = [];
  const ops = createWorkspaceMemoryOperations({
    rootFor: () => root,
    diagnosticsPost: async (path, options) => {
      calls.push([path, options?.body ?? null]);
      if (path.includes("/workspace/open")) return { ok: true, body: {} };
      if (path.includes("/registerObserved")) return { ok: true, body: { observedEpoch: 1 } };
      return { ok: false, status: 404, error: { code: "unexpected", detail: path } };
    },
  });
  const result = await dispatchMembraneHookEvent(
    { event: "PostToolUse", session_id: "s", tool_name: "Edit", tool_input: { file_path: file } },
    ops,
  );
  const observeOut = result.results.find(({ id }) => id === "membrane.diagnostics-observe").output;
  assert.equal(observeOut.state, "available");
  assert.equal(observeOut.reason, "mutation_observed");
  const paths = calls.map(([p]) => p.split("?")[0]);
  assert.deepEqual(paths, [
    "/diagnostics/workspace/status",
    "/diagnostics/workspace/open",
    "/diagnostics/mutation/registerObserved",
  ], `unexpected call sequence: ${paths.join(", ")}`);
  assert.equal(calls[1][1].projectRoot, root, "open must bind the exact canonical project root");
  assert.equal(calls[1][1].epoch, undefined, "open carries identity only");
});

test("observeMutation handles Delete File and invalidates old clearance even when hashes missing", async () => {
  const { root } = fenceFixture();
  // Simulate an existing cleared epoch 5
  const toDelete = join(root, "src", "obsolete.ts");
  mkdirSync(join(root, "src"), { recursive: true });
  writeFileSync(toDelete, "export const x=1;\n", "utf8");
  // Delete the file before observation to simulate unreadable/deleted
  const { unlinkSync: rm } = await import("node:fs");
  rm(toDelete);
  const calls = [];
  let registeredEpoch = null;
  const ops = createWorkspaceMemoryOperations({
    rootFor: () => root,
    diagnosticsPost: async (path, options) => {
      calls.push([path, options?.body ?? null]);
      if (path.includes("/workspace/status")) return { ok: true, body: { latestSealedEpoch: 5 } };
      if (path.includes("/workspace/open")) return { ok: true, body: {} };
      if (path.includes("/registerObserved")) {
        registeredEpoch = options.body.epoch;
        return { ok: true, body: { observedEpoch: registeredEpoch.epoch } };
      }
      return { ok: false, status: 404, error: { code: "unexpected", detail: path } };
    },
  });
  const result = await dispatchMembraneHookEvent(
    { event: "PostToolUse", session_id: "s", tool_name: "apply_patch", tool_input: { patch: "*** Begin Patch\n*** Delete File: src/obsolete.ts\n*** End Patch" } },
    ops,
  );
  const out = result.results.find(({ id }) => id === "membrane.diagnostics-observe").output;
  // Deletion must still register a newer epoch (6) even though file now unreadable and hashes empty;
  // it must not be skipped and must not preserve old clean state.
  assert.equal(out.state, "available");
  assert.equal(out.reason, "mutation_observed");
  assert.ok(registeredEpoch, "registerObserved must be called");
  assert.equal(registeredEpoch.epoch, 6);
  assert.equal(registeredEpoch.parentEpoch, 5);
  assert.deepEqual(registeredEpoch.changedPaths, ["src/obsolete.ts"]);
  // hashes will be empty (file deleted) but epoch still carries changedPaths, so old clearance cannot be inherited
  assert.equal(registeredEpoch.changedFileHashes.length, 0);
  const paths = calls.map(([p]) => p.split("?")[0]);
  assert.ok(paths.includes("/diagnostics/mutation/registerObserved"));
});

test("observeMutation stops when workspace.open is non-OK and does not call registerObserved", async () => {
  const { root } = fenceFixture();
  const file = join(root, "src", "conflict.ts");
  mkdirSync(join(root, "src"), { recursive: true });
  writeFileSync(file, "export {};\n", "utf8");
  const calls = [];
  const ops = createWorkspaceMemoryOperations({
    rootFor: () => root,
    diagnosticsPost: async (path, options) => {
      calls.push([path, options?.body ?? null]);
      if (path.includes("/workspace/status")) return { ok: true, body: { latestSealedEpoch: 3 } };
      if (path.includes("/workspace/open")) return { ok: false, status: 409, error: { code: "workspace_project_root_conflict", detail: "conflict" } };
      if (path.includes("/registerObserved")) return { ok: true, body: {} };
      return { ok: false, status: 404, error: { code: "unexpected", detail: path } };
    },
  });
  const result = await dispatchMembraneHookEvent(
    { event: "PostToolUse", session_id: "s", tool_name: "Edit", tool_input: { file_path: file } },
    ops,
  );
  const out = result.results.find(({ id }) => id === "membrane.diagnostics-observe").output;
  assert.equal(out.state, "unavailable");
  assert.equal(out.reason, "workspace_project_root_conflict");
  const paths = calls.map(([p]) => p.split("?")[0]);
  assert.deepEqual(paths, ["/diagnostics/workspace/status", "/diagnostics/workspace/open"], "registerObserved must not be called when open is non-OK");
});
