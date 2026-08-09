#!/usr/bin/env node
import assert from "node:assert/strict";
import { createRequire } from "node:module";

const seedText = process.env.MEMBRANE_RANDOM_SEED ?? "1597463007";
const seed = Number(seedText);
if (!Number.isSafeInteger(seed) || seed < 0 || seed > 0xffffffff) {
  throw new Error("MEMBRANE_RANDOM_SEED must be an unsigned 32-bit integer");
}

let state = seed >>> 0;
function randomUint32() {
  state = (state + 0x6d2b79f5) >>> 0;
  let value = state;
  value = Math.imul(value ^ (value >>> 15), value | 1);
  value ^= value + Math.imul(value ^ (value >>> 7), value | 61);
  return (value ^ (value >>> 14)) >>> 0;
}

function identifier(prefix) {
  return `${prefix}-${randomUint32().toString(16).padStart(8, "0")}`;
}

const require = createRequire(import.meta.url);
const { buildObservableEvent, validateObservableEvent } = require("../../mcp/host/observable-event.cjs");
const trials = 256;

for (let index = 0; index < trials; index += 1) {
  const event = buildObservableEvent({
    installationId: identifier("installation"),
    clientId: identifier("client"),
    sessionId: identifier("session"),
    taskId: identifier("task"),
    turnId: identifier("turn"),
    traceId: identifier("trace"),
    eventType: index % 2 === 0 ? "tool_receipt" : "file",
    origin: index % 2 === 0 ? "tool" : "repository",
    content: identifier("private-content"),
    completeness: { randomized: true, trial: index % 3 === 0 },
    timestamp: new Date(1_700_000_000_000 + index * 1_000).toISOString(),
  });
  assert.equal(validateObservableEvent(event), event);
  assert.throws(() => validateObservableEvent({ ...event, content: "must-not-leak" }), /forbidden field/);
}

console.log(`randomized observable-event suite OK: seed=${seed} trials=${trials}`);
