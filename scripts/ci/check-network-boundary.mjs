#!/usr/bin/env node
import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync, readFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const repoRoot = fileURLToPath(new URL("../..", import.meta.url));
const require = createRequire(import.meta.url);
const ingress = require("../../mcp/host/observable-ingress.cjs");
const { buildObservableEvent } = require("../../mcp/host/observable-event.cjs");
const ingressSource = readFileSync(path.join(repoRoot, "mcp/host/observable-ingress.cjs"), "utf8");

assert.doesNotMatch(ingressSource, /node:(?:net|http|https)|\bfetch\s*\(|\bspawn(?:Sync)?\s*\(/);

const directory = mkdtempSync(path.join(os.tmpdir(), "membrane-network-boundary-"));
try {
  const remoteConfig = path.join(directory, "runtime.json");
  writeFileSync(remoteConfig, JSON.stringify({ schemaVersion: 1, serviceId: "membrane-local-v1", host: "0.0.0.0" }));
  assert.deepEqual(ingress._internal.resolveDefaultIngressTarget(remoteConfig), { ok: false });

  const target = path.join(directory, "telemetry.jsonl");
  const previous = process.env.MEMBRANE_TELEMETRY_INGRESS;
  process.env.MEMBRANE_TELEMETRY_INGRESS = target;
  try {
    const event = buildObservableEvent({
      installationId: "installation-ci", clientId: "codex", sessionId: "session-ci",
      taskId: "task-ci", turnId: "turn-ci", traceId: "trace-ci",
      eventType: "tool_receipt", origin: "tool", content: "private content", completeness: { receipt: true },
    });
    assert.deepEqual(ingress.appendObservableEvent(event), { status: "persisted", target: "membrane_prompt_ingress" });
    assert.deepEqual(JSON.parse(readFileSync(target, "utf8")).observable_events[0], event);
  } finally {
    if (previous === undefined) delete process.env.MEMBRANE_TELEMETRY_INGRESS;
    else process.env.MEMBRANE_TELEMETRY_INGRESS = previous;
  }
} finally {
  rmSync(directory, { recursive: true, force: true });
}

console.log("network boundary OK: ingress stays local, loopback-only, and file-backed");
