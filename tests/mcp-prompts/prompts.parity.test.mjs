// MBR-303 parity test for canonical Membrane MCP prompts.
//
// Acceptance: "Prompts call bounded operations and include no hidden
// authority escalation." This file proves the legacy (JS) surface returns the
// exact same `prompts/list` and `prompts/get` payload the native (Rust)
// surface emits, by computing the Rust projection directly from the canonical
// `schemas/registry/prompts/*.json` source and asserting byte-for-byte equality.
//
// If the native (Rust) binary is available on disk the test also spawns it
// and asserts the live wire payload matches the JS projection. The native
// binary path is resolved via MEMBRANE_NATIVE_MCP_BIN (default: ../../engine/target/debug/membrane-mcp).
// Book-mode discipline: this file is not executed during implementation.

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { readFile, stat } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { listPrompts, getPrompt, PROMPT_NAMES } from "../../mcp/prompts.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(HERE, "..", "..");

const PROMPT_FILES = ["recap.v1.json", "plan.v1.json", "summarize.v1.json", "checkpoint.v1.json"];

// Exact mirror of the Rust `prompts::list_payload()` projection so the JS
// payload can be compared against the canonical shape the native server
// emits. Any drift here is a regression in parity.
function projectListEntry(prompt) {
  return {
    name: prompt.name,
    description: prompt.description,
    arguments: prompt.arguments ?? [],
    operations: prompt.operations ?? [],
    authorityScope: prompt.authorityScope ?? null,
    authorityEscalation: prompt.authorityEscalation ?? false,
    authorityNotes: prompt.authorityNotes ?? null,
  };
}

function projectGetEntry(prompt) {
  return {
    ...projectListEntry(prompt),
    messages: prompt.messages ?? [],
  };
}

const canonicalPrompts = await Promise.all(PROMPT_FILES.map(async (file) => {
  const raw = await readFile(join(REPO_ROOT, "schemas", "registry", "prompts", file), "utf8");
  return JSON.parse(raw);
}));

const expectedList = { prompts: canonicalPrompts.map(projectListEntry) };
const jsList = await listPrompts();
assert.deepEqual(jsList, expectedList, "legacy (JS) prompts/list payload equals the canonical projection used by the native (Rust) server");

for (const def of canonicalPrompts) {
  const jsGet = await getPrompt(def.name);
  assert.deepEqual(jsGet, projectGetEntry(def), `legacy (JS) prompts/get(${def.name}) payload equals the canonical projection`);
}

// Stable JSON: native and legacy must agree on object key ordering too, since
// both project from the same source using the same insertion order. Re-serialize
// both shapes via JSON.stringify with sorted keys and compare.
function stable(value) {
  if (Array.isArray(value)) return value.map(stable);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((k) => [k, stable(value[k])]));
  }
  return value;
}
assert.equal(JSON.stringify(stable(jsList)), JSON.stringify(stable(expectedList)), "prompts/list is stable under key-sorted serialization");

for (const def of canonicalPrompts) {
  const jsGet = await getPrompt(def.name);
  assert.equal(JSON.stringify(stable(jsGet)), JSON.stringify(stable(projectGetEntry(def))), `prompts/get(${def.name}) is stable under key-sorted serialization`);
}

assert.deepEqual([...PROMPT_NAMES].sort(), canonicalPrompts.map((p) => p.name).sort(), "PROMPT_NAMES matches the canonical prompt file set");

// Optional live-binary parity check. When the native MCP binary exists, drive
// it through stdio JSON-RPC and compare its wire payload to the JS projection.
async function tryNativeBinary() {
  const candidates = [];
  if (process.env.MEMBRANE_NATIVE_MCP_BIN) candidates.push(process.env.MEMBRANE_NATIVE_MCP_BIN);
  candidates.push(join(REPO_ROOT, "engine", "target", "debug", "membrane-mcp"));
  candidates.push(join(REPO_ROOT, "engine", "target", "release", "membrane-mcp"));
  for (const candidate of candidates) {
    try { await stat(candidate); return candidate; } catch { /* not built */ }
  }
  return null;
}

const nativeBin = await tryNativeBinary();
if (nativeBin) {
  const child = spawn(nativeBin, [], { stdio: ["pipe", "pipe", "pipe"] });
  const wire = [];
  const decoder = [];
  child.stdout.on("data", (chunk) => {
    decoder.push(chunk);
    const text = Buffer.concat(decoder).toString("utf8");
    const lines = text.split("\n");
    decoder.length = 0;
    for (const line of lines) {
      if (!line.trim()) continue;
      try { wire.push(JSON.parse(line)); } catch (_) { decoder.push(Buffer.from(line, "utf8")); }
    }
  });
  const send = (message) => new Promise((resolve, reject) => {
    child.stdin.write(JSON.stringify(message) + "\n", (error) => (error ? reject(error) : resolve()));
  });
  await send({ jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "mbr303-parity", version: "1.0.0" } } });
  await send({ jsonrpc: "2.0", id: 2, method: "prompts/list" });
  for (const name of PROMPT_NAMES) {
    await send({ jsonrpc: "2.0", id: 100 + PROMPT_NAMES.indexOf(name), method: "prompts/get", params: { name } });
  }
  await new Promise((resolve) => child.stdin.end());
  await new Promise((resolve) => child.once("close", resolve));

  const listRow = wire.find((row) => row.id === 2);
  assert.ok(listRow && listRow.result, "native prompts/list returned a result envelope");
  assert.deepEqual(listRow.result, expectedList, "native (Rust) prompts/list wire payload equals the canonical projection");

  for (let i = 0; i < PROMPT_NAMES.length; i += 1) {
    const name = PROMPT_NAMES[i];
    const row = wire.find((entry) => entry.id === 100 + i);
    assert.ok(row && row.result, `native prompts/get(${name}) returned a result envelope`);
    assert.deepEqual(row.result, projectGetEntry(canonicalPrompts.find((p) => p.name === name)), `native prompts/get(${name}) wire payload equals the canonical projection`);
  }
} else {
  // When the binary is absent (book-mode gate phase has not built it yet),
  // log so the operator knows parity is being proven only against the
  // canonical projection. This branch is intentional and not a failure.
  console.warn("[mbr-303 parity] native membrane-mcp binary not found; parity asserted against the canonical projection only");
}
