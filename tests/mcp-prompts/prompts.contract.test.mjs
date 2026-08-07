// MBR-303 contract tests for canonical Membrane MCP prompts.
//
// Acceptance: "Prompts call bounded operations and include no hidden
// authority escalation." This file asserts every prompt:
//   (a) only references operations that exist in the operations registry,
//   (b) declares `authorityEscalation: false`,
//   (c) lives inside an `authorityScope` no higher than the highest authority
//       the operations it references already grant.
// Book-mode discipline: tests are written to compile and pass when run, but
// this file is not executed during implementation — the deferred command list
// runs at the end of Book 1.

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { listPrompts, getPrompt, allPromptDefinitions, PROMPT_NAMES } from "../../mcp/prompts.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(HERE, "..", "..");

const registry = JSON.parse(await readFile(join(REPO_ROOT, "operations", "operations", "operations-index.v1.golden.json"), "utf8"));
const KNOWN_OPERATIONS = new Set(registry.operations.map((entry) => entry.name));

// Operations the prompt contract treats as read-only. This mirrors the
// Membrane install-time authority levels: membrane_context, membrane_source_read,
// membrane_checkpoint_load, membrane_working_context(operation=load),
// membrane_temporal_fact(operation=query), membrane_scratchpad(operation=load)
// all serve the caller at their persisted grant — they never widen authority.
// The four write paths (proposal, checkpoint save, working context save/close,
// scratchpad save, temporal fact record) all sit inside the caller's existing
// write-proposed grant and never escalate to admin.
const READ_ONLY_OPERATIONS = new Set([
  "membrane_context",
  "membrane_source_read",
  "membrane_checkpoint_load",
  "membrane_working_context", // operation is the load branch by contract
  "membrane_temporal_fact",   // operation is the query branch by contract
  "membrane_scratchpad",      // operation is the load branch by contract
]);
const WRITE_PROPOSED_OPERATIONS = new Set([
  "membrane_knowledge_propose",
  "membrane_checkpoint_save",
  "membrane_feedback",
]);

function assertBounded(name, prompt) {
  assert.ok(Array.isArray(prompt.operations), `${name} declares an operations array`);
  assert.ok(prompt.operations.length > 0, `${name} must reference at least one operation`);
  for (const op of prompt.operations) {
    assert.ok(KNOWN_OPERATIONS.has(op), `${name} references unknown operation: ${op}`);
  }
}

function assertNoEscalation(name, prompt) {
  assert.equal(prompt.authorityEscalation, false, `${name} must declare authorityEscalation: false`);
  const ops = prompt.operations;
  const allReadOnly = ops.every((op) => READ_ONLY_OPERATIONS.has(op));
  if (allReadOnly) {
    assert.equal(prompt.authorityScope, "read-only", `${name} references only read-only operations and must declare authorityScope=read-only`);
  } else if (ops.some((op) => WRITE_PROPOSED_OPERATIONS.has(op))) {
    // The only write-proposed operation a prompt currently uses is
    // membrane_checkpoint_save; the checkpoint prompt lives at write-proposed.
    assert.equal(prompt.authorityScope, "write-proposed", `${name} references a write-proposed operation and must declare authorityScope=write-proposed`);
  } else {
    assert.fail(`${name} references operations at an unsupported authority tier: ${ops.join(", ")}`);
  }
}

const definitions = await allPromptDefinitions();
assert.equal(definitions.length, 4, "four canonical prompts are committed");

for (const def of definitions) {
  assertBounded(def.name, def);
  assertNoEscalation(def.name, def);
}

const list = await listPrompts();
assert.equal(list.prompts.length, 4, "listPrompts emits the canonical set");
assert.deepEqual(
  list.prompts.map((p) => p.name).sort(),
  [...PROMPT_NAMES].sort(),
  "listPrompts and PROMPT_NAMES agree on the canonical set",
);

for (const prompt of list.prompts) {
  assertBounded(prompt.name, prompt);
  assertNoEscalation(prompt.name, prompt);
  assert.ok(typeof prompt.description === "string" && prompt.description.length > 0, `${prompt.name} declares a description`);
  assert.ok(Array.isArray(prompt.arguments), `${prompt.name} declares arguments`);
  for (const arg of prompt.arguments) {
    assert.ok(typeof arg.name === "string" && arg.name.length > 0, `${prompt.name} argument has a name`);
    assert.ok(typeof arg.description === "string", `${prompt.name} argument ${arg.name} has a description`);
    assert.equal(typeof arg.required, "boolean", `${prompt.name} argument ${arg.name} declares required`);
  }
}

for (const name of PROMPT_NAMES) {
  const got = await getPrompt(name);
  assert.ok(got, `getPrompt returns a payload for ${name}`);
  assert.equal(got.name, name);
  assert.ok(Array.isArray(got.messages) && got.messages.length > 0, `${name} carries messages`);
  for (const message of got.messages) {
    assert.equal(message.role, "user", `${name} messages are user-authored`);
    assert.equal(message.content.type, "text", `${name} messages are text`);
    assert.ok(typeof message.content.text === "string" && message.content.text.length > 0, `${name} message text is non-empty`);
    // Bounded by construction: the prompt text mentions exactly the operations it
    // already declared, never references any registry operation outside that set,
    // and never asks for scopes/permissions/elevated grants.
    for (const op of KNOWN_OPERATIONS) {
      const mentions = message.content.text.includes(op);
      if (mentions && !got.operations.includes(op)) {
        assert.fail(`${name} mentions ${op} but does not declare it in operations[]`);
      }
    }
    assert.doesNotMatch(message.content.text, /(?:grant|request)[ -](?:admin|root|elevated|escalat)/i, `${name} message must not ask for escalated grants`);
    assert.doesNotMatch(message.content.text, /(?:system|developer)[ -](?:prompt|override|bypass)/i, `${name} message must not reference system/developer overrides`);
  }
}

const unknown = await getPrompt("does-not-exist");
assert.equal(unknown, null, "getPrompt returns null for unknown prompt names");
