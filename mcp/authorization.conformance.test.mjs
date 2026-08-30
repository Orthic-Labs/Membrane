import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { authorizeTarget } from "./authorization.mjs";

const CORPUS_URL = new URL("../schemas/conformance/authorization-conformance-v1.json", import.meta.url);
const UNGATED_OPERATIONS = new Set([
  "membrane_diagnostic_fence",
  "membrane_diagnostic_capabilities",
  "membrane_diagnostic_provider:list",
  "membrane_diagnostic_provider:status",
]);
const LEVELS = new Set(["read-only", "write-proposed", "write-trusted", "admin"]);

function stringAt(object, key, caseId) {
  assert.equal(typeof object?.[key], "string", `${caseId}: ${key} must be a string`);
  return object[key];
}

function bindingFor(source, caseData, { applyValidity = false } = {}) {
  assert.ok(source && typeof source === "object" && !Array.isArray(source), `${caseData.case_id}: binding must be an object`);
  const level = stringAt(source, "grant_level", caseData.case_id);
  assert.ok(LEVELS.has(level), `${caseData.case_id}: unknown grant level ${level}`);
  const binding = {
    root: stringAt(source, "root", caseData.case_id),
    repository_id: stringAt(source, "repository_id", caseData.case_id),
    scope_id: stringAt(source, "scope_id", caseData.case_id),
    grant_policy: {
      level,
      child_repository_ids: source.child_repository_ids,
    },
  };
  assert.ok(Array.isArray(source.child_repository_ids), `${caseData.case_id}: child_repository_ids must be an array`);
  if (source.token_generation !== null) {
    assert.equal(Number.isInteger(source.token_generation), true, `${caseData.case_id}: token_generation must be integer or null`);
    binding.token_grant = {
      generation: source.token_generation,
      revoked_generations: source.revoked_token_generations,
    };
    assert.ok(Array.isArray(source.revoked_token_generations), `${caseData.case_id}: revoked generations must be an array`);
  }
  if (applyValidity) {
    switch (caseData.validity_interval) {
      case "absent":
        break;
      case "valid":
        binding.not_before = 0;
        binding.not_after = 9_000_000_000_000;
        break;
      case "not-yet-valid":
        binding.not_before = Date.now() + 60 * 60 * 1000;
        break;
      case "expired":
        binding.not_after = 0;
        break;
      default:
        throw new Error(`${caseData.case_id}: runner does not know validity interval ${caseData.validity_interval}`);
    }
  }
  return binding;
}

function buildCase(caseData) {
  const caseId = stringAt(caseData, "case_id", "<unknown>");
  assert.equal(caseData.mode, "gated", `${caseId}: buildCase only constructs gated cases`);
  const request = caseData.request;
  assert.ok(request && typeof request === "object", `${caseId}: request is required`);
  assert.equal(caseData.declared_repository, request.target_repository, `${caseId}: declared repository must be the request target claim`);
  assert.equal(caseData.requested.action, request.action, `${caseId}: requested action differs from request`);
  assert.equal(caseData.requested.authority_level, request.task_grant_level, `${caseId}: requested authority differs from request task grant`);
  const installationState = caseData.installation?.state;
  if (!new Set(["enrolled", "unavailable"]).has(installationState)) throw new Error(`${caseId}: runner does not know installation state ${installationState}`);
  const callerEnrolled = caseData.scope_chain?.caller;
  const targetEnrolled = caseData.scope_chain?.target;
  if (!new Set(["enrolled", "missing"]).has(callerEnrolled)) throw new Error(`${caseId}: runner does not know caller scope state ${callerEnrolled}`);
  if (!new Set(["enrolled", "missing"]).has(targetEnrolled)) throw new Error(`${caseId}: runner does not know target scope state ${targetEnrolled}`);
  if (!["same_root", "explicit_child_grant", "neither"].includes(caseData.cross_root_reach)) throw new Error(`${caseId}: runner does not know cross-root state ${caseData.cross_root_reach}`);
  if (!["absent", "not-yet-valid", "valid", "expired"].includes(caseData.validity_interval)) throw new Error(`${caseId}: runner does not know validity state ${caseData.validity_interval}`);
  if (!["live", "revoked token generation"].includes(caseData.revocation_state)) throw new Error(`${caseId}: runner does not know revocation state ${caseData.revocation_state}`);

  let callerBinding = bindingFor(caseData.caller_binding, caseData);
  let targetBinding = bindingFor(caseData.target_binding, caseData, { applyValidity: true });
  if (callerEnrolled === "missing") callerBinding = { repository_id: null };
  if (targetEnrolled === "missing") targetBinding = { repository_id: null };
  if (installationState === "unavailable") callerBinding = null;
  const callerIdentity = {
    repositoryId: stringAt(request, "caller_repository_id", caseId),
    scopeId: stringAt(request, "caller_scope_id", caseId),
  };
  return {
    callerBinding,
    targetBinding,
    callerIdentity,
    action: stringAt(request, "action", caseId),
    taskGrantLevel: request.task_grant_level ?? undefined,
    hasExplicitChildGrant: caseData.cross_root_reach === "explicit_child_grant",
  };
}

async function runCase(caseData) {
  const caseId = stringAt(caseData, "case_id", "<unknown>");
  const expected = caseData.expected;
  assert.ok(expected && typeof expected === "object", `${caseId}: expected is required`);
  assert.equal(typeof expected.allowed, "boolean", `${caseId}: expected.allowed must be boolean`);
  if (caseData.mode === "ungated") {
    assert.equal(expected.allowed, true, `${caseId}: ungated case must be allowed`);
    assert.ok(UNGATED_OPERATIONS.has(caseData.diagnostic_operation), `${caseId}: runner does not know ungated operation ${caseData.diagnostic_operation}`);
    return;
  }
  if (caseData.mode !== "gated") throw new Error(`${caseId}: runner does not know mode ${caseData.mode}`);
  if (caseData.diagnostic_operation !== null && caseData.diagnostic_operation !== undefined) throw new Error(`${caseId}: gated case unexpectedly declares a diagnostic operation`);
  const constructed = buildCase(caseData);
  let result;
  try {
    result = await authorizeTarget(constructed);
  } catch (error) {
    result = error;
  }
  if (expected.allowed) {
    assert.equal(result instanceof Error, false, `${caseId}: expected allow, got ${result?.message ?? result}`);
  } else {
    assert.equal(result instanceof Error, true, `${caseId}: expected denial, got allow`);
    assert.equal(result.gate, expected.failed_gate, `${caseId}: JS failed gate differs from corpus`);
  }
}

test("Rust and JS authorization surfaces consume the same corpus in file order", async () => {
  const corpus = JSON.parse(await readFile(CORPUS_URL, "utf8"));
  assert.equal(corpus.schema_version, "membrane.authorization-conformance.v1");
  assert.ok(Array.isArray(corpus.cases), "authorization corpus cases must be an array");
  assert.equal(corpus.cases.length, 18, "authorization corpus case count");
  let executed = 0;
  for (const caseData of corpus.cases) {
    await runCase(caseData);
    executed += 1;
  }
  assert.equal(executed, corpus.cases.length, "every corpus case must execute");
});
