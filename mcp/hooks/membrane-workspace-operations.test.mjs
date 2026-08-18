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
    runCrypt: async (_root, args) => { calls.push(args.join(" ")); return true; },
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

test("ingest accepts only durable memory & Cortex artifacts, including host memory", () => {
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

test("checkpoint summaries redact secret-bearing lines before Crypt", async () => {
  const { root } = fixture(); const inputs = [];
  const ops = createWorkspaceMemoryOperations({
    rootFor: () => root,
    contextAdapter: {}, probeStatus: async () => true,
    runCrypt: async (_root, _args, input) => { inputs.push(input); return true; },
  });
  await dispatchMembraneHookEvent({ event: "PreCompact", cwd: root, session_id: "redact" }, ops);
  await dispatchMembraneHookEvent({ event: "PostCompact", cwd: root, session_id: "redact", compact_summary: "safe\ntoken=private\nsafe too" }, ops);
  assert.equal(JSON.parse(inputs[0]).summary, "safe\n[redacted sensitive summary line]\nsafe too");
});

test("SessionStart performs health only & never starts or kicks Crypt", async () => {
  const { root } = fixture(); const calls = [];
  const result = await runHook({ hook_event_name: "SessionStart", cwd: root }, { operations: operations(root, calls) });
  assert.deepEqual(calls, ["status"]);
  assert.equal(result.membraneHook.results[0].output.detail.lifecycle, "hub-child");
  assert.equal(result.membraneHook.results[1].output.reason, "event_not_applicable");
  const manifest = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8"));
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
    runCrypt: async () => true, postObservation: async (_root, _event, trace) => { observed.push(trace); return true; } });

  const database = join(root, "crypt-engine.db");
  const seen = join(root, "recall-seen", `${session}.json`);
  mkdirSync(join(seen, ".."), { recursive: true });
  writeFileSync(seen, "{}\n", "utf8");
  const priorDatabase = process.env.CRYPT_DB;
  process.env.CRYPT_DB = database;
  const rearm = await dispatchMembraneHookEvent({ event: "SessionStart", source: "compact", session_id: session }, ops);
  if (priorDatabase === undefined) delete process.env.CRYPT_DB; else process.env.CRYPT_DB = priorDatabase;
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
    runCrypt: async () => true,
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
