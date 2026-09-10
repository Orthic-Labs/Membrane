#!/usr/bin/env node
// BM01/BM02/BM08/BM10 installed Windows qualification.
// Model: gpt-5.6-luna.

import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const JOURNEY_STATES = new Set(["NOT_DISCOVERED", "DISCOVERED_ACCEPTED", "DISCOVERED_REJECTED", "DISCOVERED_BUDGET_DROPPED", "STALE", "ADAPTER_DROPPED", "EXECUTION_FAILURE"]);
const REQUIRED_JOURNEY_STATES = ["NOT_DISCOVERED", "DISCOVERED_REJECTED", "DISCOVERED_BUDGET_DROPPED", "STALE", "ADAPTER_DROPPED", "EXECUTION_FAILURE"];
const DIMENSIONS = new Set(["repository_truth", "current_state", "policy", "history", "diagnostics", "durable_knowledge"]);
const DISPOSITIONS = new Set(["allow", "continue", "block", "noop"]);
const nonEmpty = (value) => typeof value === "string" && value.trim().length > 0;
const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
const isInteger = (value) => Number.isInteger(value) && value >= 0;
const asArray = (value, label) => { if (!Array.isArray(value)) throw new Error(`${label} must be an array`); return value; };
const asObject = (value, label) => { if (!isObject(value)) throw new Error(`${label} must be an object`); return value; };
const string = (value, label) => { if (!nonEmpty(value)) throw new Error(`${label} must be a non-empty string`); return value; };
const integer = (value, label) => { if (!isInteger(value)) throw new Error(`${label} must be a non-negative integer`); return value; };
const bool = (value, label) => { if (typeof value !== "boolean") throw new Error(`${label} must be boolean`); return value; };
const sha256Digest = (value, label) => { string(value, label); if (!/^sha256:[0-9a-f]{64}$/i.test(value)) throw new Error(`${label} must be sha256:<64 hex>`); return value; };
export const validateSourceDigest = (value, label = "sourceDigest") => { string(value, label); if (!/^(?:sha256:[0-9a-f]{64}|xxh128:[0-9a-f]{32})$/i.test(value)) throw new Error(`${label} must be sha256:<64 hex> or xxh128:<32 hex>`); return value; };
const sameSet = (left, right) => left.length === right.length && left.every((value) => right.includes(value));
const parse = (text, label) => { try { return JSON.parse(text); } catch { throw new Error(`${label} emitted invalid JSON`); } };

export function validateCandidate(candidate, label = "candidate") {
  const value = asObject(candidate, label);
  string(value.id, `${label}.id`); string(value.sourceRef, `${label}.sourceRef`); validateSourceDigest(value.sourceHash, `${label}.sourceHash`);
  string(value.sourceKind, `${label}.sourceKind`); string(value.trustClass, `${label}.trustClass`); string(value.instructionPolicy, `${label}.instructionPolicy`); string(value.resolver, `${label}.resolver`); string(value.text, `${label}.text`);
  integer(value.layer, `${label}.layer`); integer(value.estimatedTokens, `${label}.estimatedTokens`); bool(value.exact, `${label}.exact`); bool(value.recoverable, `${label}.recoverable`);
  return value;
}
export function validateCandidateSet(set, label = "candidateSet") {
  const value = asObject(set, label);
  if (value.schemaVersion !== 1) throw new Error(`${label}.schemaVersion must equal 1`);
  string(value.state, `${label}.state`); string(value.freshness, `${label}.freshness`); const candidates = asArray(value.candidates, `${label}.candidates`);
  if (Object.keys(candidates).length !== candidates.length) throw new Error(`${label}.candidates must be dense`);
  if (value.candidateCount !== candidates.length) throw new Error(`${label}.candidateCount disagrees with candidates`);
  bool(value.truncated, `${label}.truncated`); string(value.coverage, `${label}.coverage`); asArray(value.omissions, `${label}.omissions`);
  const ids = candidates.map((candidate, index) => validateCandidate(candidate, `${label}.candidates[${index}]`).id);
  if (new Set(ids).size !== ids.length) throw new Error(`${label}.candidates contains duplicate IDs`);
  return value;
}
function validateResolution(value, expectedState, label) {
  const response = asObject(value, label); if (response.schemaVersion !== 1) throw new Error(`${label}.schemaVersion must equal 1`); string(response.generationId, `${label}.generationId`); string(response.requestedTarget, `${label}.requestedTarget`);
  const resolution = asObject(response.resolution, `${label}.resolution`); if (resolution.state !== expectedState) throw new Error(`${label}.resolution.state must be ${expectedState}`);
  const candidates = asArray(resolution.candidates, `${label}.resolution.candidates`); integer(resolution.candidateCount, `${label}.resolution.candidateCount`); if (resolution.candidateCount !== candidates.length) throw new Error(`${label}.resolution candidate count disagrees`);
  const set = validateCandidateSet(response.candidateSet, `${label}.candidateSet`); if (set.state !== expectedState) throw new Error(`${label}.candidateSet.state must agree with resolution`);
  if (expectedState === "ambiguous") {
    if (resolution.ambiguous !== true || resolution.resolved !== null) throw new Error(`${label} ambiguity collapsed to a resolved winner`);
    if (candidates.length < 2 || set.candidates.length < 2) throw new Error(`${label} ambiguity omitted alternatives`);
    if (!asArray(resolution.omissions, `${label}.resolution.omissions`).some((item) => item?.reason === "same_tier_ambiguity")) throw new Error(`${label} omitted same_tier_ambiguity`);
  } else if (expectedState === "unresolved") {
    if (resolution.ambiguous !== false || resolution.resolved !== null || candidates.length !== 0 || set.candidates.length !== 0) throw new Error(`${label} unknown target was not an explicit empty outcome`);
    if (!asArray(resolution.omissions, `${label}.resolution.omissions`).length) throw new Error(`${label} omitted unknown-target reason`);
  } else if (expectedState === "resolved") {
    if (resolution.ambiguous !== false || !isObject(resolution.resolved) || !candidates.length || !set.candidates.length) throw new Error(`${label} resolved target omitted typed winner`);
    for (const key of ["id", "sourceRef", "sourceHash"]) if (resolution.resolved[key] !== candidates[0][key] || candidates[0][key] !== set.candidates[0][key]) throw new Error(`${label} Resolve/Recall ${key} disagreement`);
  }
  return response;
}
function invokeAttempt(exe, args, cwd, input) {
  try { const options = { cwd, encoding: "utf8", windowsHide: true }; if (input !== undefined) options.input = input; const stdout = execFileSync(exe, args, options); return { ok: true, value: parse(stdout, args.join(" ")), args }; }
  catch (error) { return { ok: false, error, stdout: String(error.stdout ?? ""), stderr: String(error.stderr ?? ""), args }; }
}
function invoke(exe, args, cwd, input) { const result = invokeAttempt(exe, args, cwd, input); if (!result.ok) throw new Error(`${args.join(" ")} failed: ${result.stderr || result.error.message}`); return result.value; }
function installed() {
  const root = process.env.MEMBRANE_QUALIFICATION_INSTALLED_ROOT; if (!nonEmpty(root)) throw new Error("MEMBRANE_QUALIFICATION_INSTALLED_ROOT is required; checkout execution is forbidden");
  const exe = join(root, "membrane.exe"); if (!existsSync(exe)) throw new Error(`installed membrane.exe missing: ${exe}`); return { root: resolve(root), exe };
}
function makeFixture() {
  const root = mkdtempSync(join(tmpdir(), "membrane-bm-"));
  writeFileSync(join(root, "lib.rs"), "pub fn exact_probe() -> &'static str { \"exact\" }\npub fn call_probe() -> &'static str { exact_probe() }\npub fn same_name_a() -> &'static str { \"a\" }\npub fn same_name_b() -> &'static str { \"b\" }\n");
  execFileSync("git", ["init", "--quiet"], { cwd: root, windowsHide: true }); execFileSync("git", ["add", "lib.rs"], { cwd: root, windowsHide: true });
  execFileSync("git", ["-c", "user.name=Membrane BM", "-c", "user.email=bm@membrane.invalid", "commit", "--quiet", "-m", "fixture"], { cwd: root, windowsHide: true }); return root;
}
function basePath() {
  const { root, exe } = installed(); const identity = invoke(exe, ["cli", "build-info"], root); const generation = identity.releaseGeneration ?? identity.release_generation ?? identity.generation;
  if (!nonEmpty(generation) || [identity.runtimeOrigin, identity.runtime_origin].some((value) => value && value !== "installed")) throw new Error("installed identity is absent or not installed");
  const repo = makeFixture();
  try {
    const build = invoke(exe, ["cli", "blueprint", "build", "--repo-root", repo], repo); const blueprintGeneration = build.generationId ?? build.generation_id; string(blueprintGeneration, "Blueprint build.generationId");
    const recall = invoke(exe, ["cli", "blueprint", "recall", "--repo-root", repo, "--task", "exact_probe"], repo); validateCandidateSet(recall.candidateSet, "Recall.candidateSet"); const candidates = recall.candidateSet.candidates; if (!candidates.length) throw new Error("native Recall omitted source-bound candidates");
    const packet = invoke(exe, ["cli", "pull", "federate", "--repo", repo, "--task", "exact_probe", "--max-tokens", "1024"], repo); asObject(packet.packet, "final packet"); asArray(packet.receipts, "final receipts"); if (packet.transport !== "native") throw new Error("installed Pull did not report native transport");
    return { root, exe, repo, generation, blueprintGeneration, recall, candidates, packet };
  } catch (error) { rmSync(repo, { recursive: true, force: true }); throw error; }
}
function actual(id, body) { let path; try { path = basePath(); return { status: "passed", evidenceKind: "installed", detail: { id, ...body(path) } }; } catch (error) { return { status: error.insufficient ? "insufficient" : "failed", evidenceKind: "installed", detail: { id }, reason: error.message }; } finally { if (path?.repo) rmSync(path.repo, { recursive: true, force: true }); } }
function insufficient(message) { const error = new Error(message); error.insufficient = true; throw error; }
function explicitRequest(path, control) {
  const taskId = `bm02-${control.id}`;
  const sessionId = taskId;
  const observedAtUnixMs = Date.now();
  return {
    task: control.task, repo: path.repo, taskId, maxTokens: 1024,
    client: "membrane-bm02", sessionId,
    // Resident/native federation enforces request-time H8. Keep this probe
    // bound to same explicit task/session identities instead of treating a
    // missing host observation as an implicit infinite budget.
    remainingContextCeiling: {
      schemaVersion: 1,
      ceilingId: `bm02-h8:${sessionId}:${observedAtUnixMs}`,
      sessionId,
      taskId: { coverage: "complete", value: taskId },
      requestedAtUnixMs: observedAtUnixMs,
      remainingTokens: { basis: { id: "o200k_base", version: "1" }, estimate: { coverage: "complete", value: 100000 } },
      provenanceReceipt: {
        schemaVersion: 1,
        receiptId: `bm02-h8:${sessionId}:${observedAtUnixMs}`,
        source: "qualification-host",
        observedAtUnixMs,
        receiptDigest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      },
    },
    ...control.request,
  };
}
function explicitFederate(path, control) {
  const request = explicitRequest(path, control);
  const bindingProbe = invoke(path.exe, ["cli", "explicit-call"], path.root, JSON.stringify({ schemaVersion: 1, operation: "binding", expectedBinding: null, request: {} }));
  const binding = asObject(bindingProbe.binding, "Explicit binding");
  const response = invoke(path.exe, ["cli", "explicit-call"], path.root, JSON.stringify({ schemaVersion: 1, operation: "federate", expectedBinding: binding, request }));
  const data = response.data ?? response.output ?? response.body;
  return { invocation: { operation: "federate", request }, response, output: asObject(data, `BM02 ${control.id}.output`) };
}
const CANCEL_CODE_PATTERN = "(?:request_cancelled|deadline_exceeded|cancelled|[a-z_]*cancel[a-z_]*)";
export function extractTypedCancellationCode(text, structured) {
  const serialized = structured ? JSON.stringify(structured) : text;
  // 1) structured JSON: an explicit error/code/reason field carrying the typed code.
  if (structured) {
    for (const value of [structured.code, structured.error, structured.reason, structured?.error?.code, structured?.error?.reason]) {
      if (typeof value === "string") {
        const match = value.match(new RegExp(`^${CANCEL_CODE_PATTERN}$`, "i"));
        if (match) return match[0].toLowerCase();
      }
    }
  }
  // 2) CLI's typed-outcome form: "membrane: <code>: <message>".
  const cliMatch = serialized.match(new RegExp(`membrane:\\s*(${CANCEL_CODE_PATTERN})\\s*:`, "i"));
  if (cliMatch) return cliMatch[1].toLowerCase();
  // 3) generic "error/code/reason: <code>" prefixed form.
  const prefixed = serialized.match(new RegExp(`(?:error|code|reason)[^a-z]*(${CANCEL_CODE_PATTERN})`, "i"));
  if (prefixed) return prefixed[1].toLowerCase();
  return undefined;
}
function cancellation(path) {
  const result = invokeAttempt(path.exe, ["cli", "blueprint", "recall", "--repo-root", path.repo, "--task", "exact_probe", "--cancel-before-dispatch"], path.repo);
  const text = `${result.stdout}\n${result.stderr}`;
  let structured;
  try { structured = result.stdout ? JSON.parse(result.stdout) : undefined; } catch {}
  const code = extractTypedCancellationCode(text, structured);
  if (result.ok) throw new Error("BM01 cancellation unexpectedly produced successful Blueprint output");
  if (!code) throw new Error(`BM01 cancellation omitted typed native outcome: ${text.trim()}`);
  return code;
}
export function validateBM01Observations({ exact, ambiguous, unknown, cancellationCode }) { validateResolution(exact, "resolved", "BM01 exact"); validateResolution(ambiguous, "ambiguous", "BM01 ambiguous"); validateResolution(unknown, "unresolved", "BM01 unknown"); if (!["request_cancelled", "deadline_exceeded", "cancelled"].includes(cancellationCode)) throw new Error("BM01 cancellation outcome is not typed"); return { states: [exact.state, ambiguous.state, unknown.state], cancellation: cancellationCode }; }

function scenarioRecord(output, expected, control) {
  const record = asObject(output, `BM02 ${expected}`); const invocation = asObject(record.invocation, `BM02 ${expected}.invocation`); if (invocation.operation !== "federate") throw new Error(`BM02 ${expected} was not invoked through explicit federate control`);
  const request = asObject(invocation.request, `BM02 ${expected}.invocation.request`); const binding = asObject(control, `BM02 ${expected}.control`); string(binding.id, `BM02 ${expected}.control.id`); string(binding.task, `BM02 ${expected}.control.task`); if (binding.id !== expected) throw new Error(`BM02 ${expected} output/control identity disagrees`); if (binding.task !== "exact_probe" || request.task !== binding.task) throw new Error(`BM02 ${expected} control task was keyword-derived or unbound`); for (const [key, expectedValue] of Object.entries(binding.request ?? {})) if (JSON.stringify(request[key]) !== JSON.stringify(expectedValue)) throw new Error(`BM02 ${expected} control ${key} was not bound to invocation`);
  const response = asObject(record.response, `BM02 ${expected}.response`); if (response.schemaVersion !== 1) throw new Error(`BM02 ${expected}.response.schemaVersion must equal 1`); if (!isInteger(response.status)) throw new Error(`BM02 ${expected}.response.status missing`);
  const value = asObject(record.output, `BM02 ${expected}.output`);
  if (Object.hasOwn(value, "scenario") || Object.hasOwn(value, "scenarioId")) throw new Error(`BM02 ${expected} used invented scenario marker`);
  const source = asObject(value.sourceResponse ?? value, `BM02 ${expected} outer response`); const sourceMustDegrade = new Set(["partial", "stale", "unsupported", "timeout", "unresolved_dynamic"]).has(expected); if (sourceMustDegrade && source.complete !== false) throw new Error(`BM02 ${expected} outer response remained complete`); const sourceWarnings = asArray(source.warnings, `BM02 ${expected}.warnings`); if (source.complete === false && !sourceWarnings.length) throw new Error(`BM02 ${expected} omitted outer warnings`);
  const admission = asObject(value.admission ?? value.finalAdmission, `BM02 ${expected}.admission`); if (!["partial", "insufficient", "blocked", "degraded"].includes(admission.status)) throw new Error(`BM02 ${expected} admission did not degrade`); if (!asArray(admission.omissions, `BM02 ${expected}.admission.omissions`).length) throw new Error(`BM02 ${expected} admission omitted reasons`);
  const packet = asObject(value.packet, `BM02 ${expected}.packet`); if (!asArray(packet.omissions, `BM02 ${expected}.packet.omissions`).length) throw new Error(`BM02 ${expected} rendered packet omitted reasons`);
  const requiredReason = { partial: "provider_partial", stale: "blueprint_stale", unsupported: "provider_capability_missing", timeout: "provider_timeout", unresolved_dynamic: "unresolved_dynamic" }[expected]; if (!requiredReason) throw new Error(`BM02 unknown degradation expectation ${expected}`); const reasonText = JSON.stringify({ source, admission, packet, output: value }); if (!reasonText.includes(requiredReason)) throw new Error(`BM02 ${expected} omitted typed reason ${requiredReason}`); return value;
}
function diagnostics(output, expected) {
  const metrics = asObject(output.federationMetrics ?? {}, `BM02 ${expected}.federationMetrics`); const rows = output.providerDiagnostics ?? metrics.providerTimings; const list = asArray(rows, `BM02 ${expected}.providerDiagnostics`); if (!list.length) throw new Error(`BM02 ${expected} omitted provider diagnostics`);
  for (const [index, row] of list.entries()) { const value = asObject(row, `BM02 ${expected}.providerDiagnostics[${index}]`); string(value.provider, `BM02 ${expected}.providerDiagnostics[${index}].provider`); const attrs = isObject(value.attributes) ? value.attributes : value; string(value.status ?? attrs.status, `BM02 ${expected}.diagnostics.status`); string(value.generation ?? attrs.generation, `BM02 ${expected}.diagnostics.generation`); string(value.freshness ?? attrs.freshness, `BM02 ${expected}.diagnostics.freshness`); if (!isInteger(value.latencyMs) && !isInteger(value.elapsedMs) && !isInteger(attrs.latencyMs)) throw new Error(`BM02 ${expected}.diagnostics latency missing`); bool(value.cancellation ?? attrs.cancellation, `BM02 ${expected}.diagnostics.cancellation`); if (!(Array.isArray(value.errors) || Array.isArray(attrs.errors))) throw new Error(`BM02 ${expected}.diagnostics.errors missing`); string(value.fallback ?? attrs.fallback, `BM02 ${expected}.diagnostics.fallback`); integer(value.candidateCount ?? attrs.candidateCount, `BM02 ${expected}.diagnostics.candidateCount`); }
  return list.length;
}
export function validateBM02Scenario(output, expected, control) { const value = scenarioRecord(output, expected, control); return { controlId: control.id, diagnostics: diagnostics(value, expected) }; }

function atomicPath(value, label) { const item = asObject(value, label); string(item.candidateId ?? item.evidenceId, `${label}.candidateId`); string(item.sourceRef ?? item.path, `${label}.sourceRef`); validateSourceDigest(item.sourceHash, `${label}.sourceHash`); bool(item.complete, `${label}.complete`); if (item.complete !== true) throw new Error(`${label}.complete must be true`); asObject(item.provenance, `${label}.provenance`); return item; }
export function validateBM08Admission(output) {
  const value = asObject(output, "BM08 output"); const comparison = asObject(value.admissionComparison, "BM08 admissionComparison"); if (comparison.independent !== true) throw new Error("BM08 admission comparison is not independently specified");
  for (const lane of ["queryDriven", "baseline"]) { const projection = asObject(comparison[lane], `BM08 ${lane}`); bool(projection.applicable, `${lane}.applicable`); string(projection.scope, `${lane}.scope`); string(projection.authority, `${lane}.authority`); integer(projection.budget, `${lane}.budget`); asArray(projection.omissions, `${lane}.omissions`); }
  const packet = asObject(value.packet, "BM08.packet"); const blocks = asArray(packet.blocks, "BM08.packet.blocks"); const receipts = asArray(value.receipts, "BM08.receipts"); const blockById = new Map();
  for (const [index, block] of blocks.entries()) { const candidate = asObject(block, `BM08.packet.blocks[${index}]`); string(candidate.id, `BM08.packet.blocks[${index}].id`); string(candidate.sourceRef, `BM08.packet.blocks[${index}].sourceRef`); validateSourceDigest(candidate.sourceHash, `BM08.packet.blocks[${index}].sourceHash`); string(candidate.provider, `BM08.packet.blocks[${index}].provider`); blockById.set(candidate.id, candidate); }
  for (const [index, receipt] of receipts.entries()) { const item = asObject(receipt, `BM08.receipts[${index}]`); string(item.id, `BM08.receipts[${index}].id`); string(item.sourceRef, `${item.id}.sourceRef`); sha256Digest(item.contentSha256, `${item.id}.contentSha256`); const block = blockById.get(item.id); if (item.decision === "admitted" && !block) throw new Error(`BM08 admitted receipt ${item.id} has no complete block`); if (block && item.sourceRef !== block.sourceRef) throw new Error(`BM08 receipt ${item.id} provenance disagrees with packet block`); }
  const paths = asArray(value.atomicEvidencePaths, "BM08.atomicEvidencePaths"); const pathById = new Map(); paths.forEach((item, index) => { const path = atomicPath(item, `BM08.atomicEvidencePaths[${index}]`); const id = path.candidateId ?? path.evidenceId; if (pathById.has(id)) throw new Error(`BM08 duplicate atomic evidence path ${id}`); pathById.set(id, path); const block = blockById.get(id); if (!block || block.sourceRef !== (path.sourceRef ?? path.path)) throw new Error(`BM08 atomic evidence path ${id} is not bound to complete packet block`); }); const pairs = asArray(value.contradictionPairs, "BM08.contradictionPairs");
  for (const [index, pair] of pairs.entries()) { const item = asObject(pair, `BM08.contradictionPairs[${index}]`); bool(item.required, `BM08.contradictionPairs[${index}].required`); bool(item.present, `BM08.contradictionPairs[${index}].present`); string(item.leftId, `BM08.contradictionPairs[${index}].leftId`); string(item.rightId, `BM08.contradictionPairs[${index}].rightId`); if (item.required && !item.present) throw new Error(`BM08 required contradiction pair ${index} was omitted`); if (!blockById.has(item.leftId) || !blockById.has(item.rightId)) throw new Error(`BM08 contradiction pair ${index} is not atomic`); }
  const reduction = asObject(value.budgetReduction, "BM08.budgetReduction"); bool(reduction.misleadingFragmentRetained, "BM08.misleadingFragmentRetained"); if (reduction.misleadingFragmentRetained) throw new Error("BM08 retained misleading fragment after budget reduction"); asArray(reduction.omissionReasons, "BM08.omissionReasons");
  const ambiguity = asObject(value.ambiguityDisposition, "BM08.ambiguityDisposition"); if (!DISPOSITIONS.has(ambiguity.action)) throw new Error("BM08 ambiguity disposition is not allow/continue/block/noop"); asArray(ambiguity.candidates, "BM08.ambiguityDisposition.candidates"); if (ambiguity.candidates.length < 2) throw new Error("BM08 ambiguity omitted alternatives"); if (ambiguity.action !== "allow" && ambiguity.selectedId != null) throw new Error("BM08 ambiguity silently selected top-1"); return { blockCount: blocks.length, receiptCount: receipts.length, disposition: ambiguity.action };
}

function validateJourney(journey, label) { const item = asObject(journey, label); string(item.evidenceId, `${label}.evidenceId`); string(item.requirementBindingDigest, `${label}.requirementBindingDigest`); string(item.provider, `${label}.provider`); if (!DIMENSIONS.has(item.dimension)) throw new Error(`${label}.dimension invalid`); if (!JOURNEY_STATES.has(item.state)) throw new Error(`${label}.state invalid`); if (item.sourceHash === "unknown") { if (item.state === "DISCOVERED_ACCEPTED" || item.acquired) throw new Error(`${label}.sourceHash unknown for acquired evidence`); } else validateSourceDigest(item.sourceHash, `${label}.sourceHash`); if (item.representationDigest === "unknown") { if (item.represented) throw new Error(`${label}.representationDigest unknown for represented evidence`); } else sha256Digest(item.representationDigest, `${label}.representationDigest`); for (const field of ["acquired", "eligible", "admitted", "represented", "fenced", "emitted", "retained", "dropped"]) bool(item[field], `${label}.${field}`); return item; }
export function validateBM10Journey(output) {
  const value = asObject(output, "BM10 output"); const map = asObject(value.requirementEvidenceMap, "BM10.requirementEvidenceMap"); if (map.schemaVersion !== 1) throw new Error("BM10 requirementEvidenceMap.schemaVersion must equal 1"); string(map.taskId, "BM10.taskId"); const journeys = asArray(map.journeys, "BM10.journeys"); asArray(map.unsatisfied, "BM10.unsatisfied"); const ids = new Set(); const states = new Set();
  for (const [index, journey] of journeys.entries()) { const item = validateJourney(journey, `BM10.journeys[${index}]`); if (ids.has(item.evidenceId)) throw new Error(`BM10 duplicate evidence ID ${item.evidenceId}`); ids.add(item.evidenceId); states.add(item.state); }
  for (const state of REQUIRED_JOURNEY_STATES) if (!states.has(state)) throw new Error(`BM10 missing CandidateJourneyV1 state ${state}`);
  const expected = asArray(value.expectedEvidence, "BM10.expectedEvidence"); const expectedIds = expected.map((item, index) => { const row = asObject(item, `BM10.expectedEvidence[${index}]`); string(row.evidenceId, `BM10.expectedEvidence[${index}].evidenceId`); bool(row.required, `BM10.expectedEvidence[${index}].required`); return row.evidenceId; }); if (!sameSet([...ids], expectedIds)) throw new Error("BM10 journey IDs do not equal independently specified expected evidence IDs");
  for (const field of ["conversionReceipts", "omissionReceipts"]) { const receipts = asArray(value[field], `BM10.${field}`); if (!receipts.length) throw new Error(`BM10 ${field} absent`); for (const [index, receipt] of receipts.entries()) { const row = asObject(receipt, `BM10.${field}[${index}]`); string(row.receiptId, `BM10.${field}[${index}].receiptId`); string(row.evidenceId, `BM10.${field}[${index}].evidenceId`); string(row.reason, `BM10.${field}[${index}].reason`); if (!ids.has(row.evidenceId)) throw new Error(`BM10 ${field} references unknown evidence ID`); } }
  const attribution = asArray(value.deliveryAttribution, "BM10.deliveryAttribution"); const attributionIds = new Set(); for (const [index, row] of attribution.entries()) { const item = asObject(row, `BM10.deliveryAttribution[${index}]`); string(item.evidenceId, `BM10.deliveryAttribution[${index}].evidenceId`); if (attributionIds.has(item.evidenceId)) throw new Error(`BM10 duplicate delivery attribution ${item.evidenceId}`); attributionIds.add(item.evidenceId); bool(item.emitted, `BM10.deliveryAttribution[${index}].emitted`); bool(item.hostIncluded, `BM10.deliveryAttribution[${index}].hostIncluded`); bool(item.modelUsed, `BM10.deliveryAttribution[${index}].modelUsed`); bool(item.helped, `BM10.deliveryAttribution[${index}].helped`); if ((item.hostIncluded || item.modelUsed || item.helped) && (!isObject(item.observation) || item.observation.supported !== true)) throw new Error(`BM10 unsupported host/model/help attribution for ${item.evidenceId}`); if (item.outcome === "DELIVERED_IGNORED" && (!isObject(item.observation) || item.observation.supported !== true)) throw new Error("BM10 DELIVERED_IGNORED lacks supported observation"); }
  if (!sameSet([...ids], [...attributionIds])) throw new Error("BM10 delivery attribution IDs do not equal journey IDs");
  return { journeyCount: journeys.length, states: [...states].sort(), expectedCount: expected.length };
}

// Each explicit control is an independent Pull request.  Canonical evidence
// IDs may therefore recur across maps; only duplicate IDs within one map are
// invalid.  Keep state coverage across requests separate from per-request
// accounting validation.
export function collectBM10JourneyStates(outputs) {
  const states = new Set();
  for (const output of outputs) {
    const map = output?.requirementEvidenceMap;
    if (!isObject(map) || !Array.isArray(map.journeys)) continue;
    const ids = new Set();
    for (const [index, journey] of map.journeys.entries()) {
      const item = validateJourney(journey, `BM10.journeys[${index}]`);
      if (ids.has(item.evidenceId)) throw new Error(`BM10 duplicate evidence ID ${item.evidenceId} within one Pull map`);
      ids.add(item.evidenceId);
      states.add(item.state);
    }
  }
  return [...states].sort();
}

export function validateBM10ControlOutput(output, control) {
  const value = asObject(output, `BM10 ${control.id}.output`);
  const map = asObject(value.requirementEvidenceMap, `BM10 ${control.id}.requirementEvidenceMap`);
  const journeys = asArray(map.journeys, `BM10 ${control.id}.journeys`);
  const states = new Set(journeys.map((journey, index) => validateJourney(journey, `BM10 ${control.id}.journeys[${index}]`).state));
  if (!states.has(control.expectedState)) {
    throw new Error(`BM10 installed control ${control.id} expected journey state ${control.expectedState}, observed ${[...states].sort().join(",") || "none"}`);
  }
  return [...states].sort();
}

export function BM01() { return actual("BM01", (path) => { const exactId = path.candidates[0]?.id; if (!nonEmpty(exactId)) throw new Error("BM01 Recall omitted stable candidate ID"); const exact = invoke(path.exe, ["cli", "blueprint", "resolve", "--repo-root", path.repo, "--node", exactId], path.repo); const ambiguous = invoke(path.exe, ["cli", "blueprint", "resolve", "--repo-root", path.repo, "--node", "same_name"], path.repo); const unknown = invoke(path.exe, ["cli", "blueprint", "resolve", "--repo-root", path.repo, "--node", "does_not_exist"], path.repo); const cancellationCode = cancellation(path); return { releaseGeneration: path.generation, blueprintGeneration: path.blueprintGeneration, ...validateBM01Observations({ exact, ambiguous, unknown, cancellationCode }) }; }); }
export function BM02() { return actual("BM02", (path) => {
  const controls = [
    { id: "partial", task: "exact_probe", request: { packetCharBudget: 1 } },
    { id: "stale", task: "exact_probe", request: { generation: "bm02-stale-generation" } },
    { id: "unsupported", task: "exact_probe", request: { consumerCapabilities: { resolvers: ["bm02-unsupported-resolver"] } } },
    { id: "timeout", task: "exact_probe", request: { maxWaitMs: 1 } },
    { id: "unresolved_dynamic", task: "exact_probe", request: { anchors: ["dynamic://bm02/unresolved"] } },
  ];
  const results = {};
  for (const control of controls) {
    let record;
    try { record = explicitFederate(path, control); }
    catch (error) { insufficient(`BM02 ${control.id} installed control unavailable or unbound: membrane cli explicit-call federate; ${error.message}`); }
    if (!record.output.sourceResponse && !record.output.finalAdmission) insufficient(`BM02 ${control.id} installed output omits typed SourceResponse/admission required for degradation accounting; engine/crates/membrane-runtime/src/pull/federation.rs#native_response_to_ccs`);
    results[control.id] = validateBM02Scenario(record, control.id, control);
  }
  return { releaseGeneration: path.generation, scenarios: results };
}); }
export function BM08() { return actual("BM08", (path) => { const output = invoke(path.exe, ["cli", "pull", "federate", "--repo", path.repo, "--task", "same_name", "--max-tokens", "1"], path.repo); return { releaseGeneration: path.generation, ...validateBM08Admission(output) }; }); }
export function BM10() { return actual("BM10", (path) => {
  const controls = [
    { id: "partial", expectedState: "DISCOVERED_BUDGET_DROPPED", task: "exact_probe", request: { packetCharBudget: 1 } },
    { id: "stale", expectedState: "STALE", task: "exact_probe", request: { generation: "bm10-stale-generation" } },
    { id: "unsupported", expectedState: "ADAPTER_DROPPED", task: "exact_probe", request: { consumerCapabilities: { resolvers: ["bm10-unsupported-resolver"] } } },
    { id: "timeout", expectedState: "EXECUTION_FAILURE", task: "exact_probe", request: { maxWaitMs: 1 } },
    { id: "unresolved_dynamic", expectedState: "NOT_DISCOVERED", task: "exact_probe", request: { anchors: ["dynamic://bm10/unresolved"] } },
    { id: "budget_dropped", expectedState: "DISCOVERED_BUDGET_DROPPED", task: "exact_probe", request: { packetCharBudget: 1, requirementFacts: [{ dimension: "repository_truth", required: true, ruleId: "bm10_budgeted_source", exactTarget: "lib.rs" }] } },
    { id: "rejected", expectedState: "DISCOVERED_REJECTED", task: "exact_probe", request: { anchors: ["symbol:exact_probe"], requirementFacts: [{ dimension: "repository_truth", required: true, ruleId: "bm10_rejected_source" }] } },
    { id: "not_discovered", expectedState: "NOT_DISCOVERED", task: "exact_probe", request: { requirementFacts: [{ dimension: "current_state", required: true, ruleId: "bm10_missing_target", exactTarget: "does_not_exist.rs" }] } },
  ];
  const outputs = [path.packet];
  for (const control of controls) {
    let output;
    try {
      output = explicitFederate(path, control).output;
    }
    catch (error) { insufficient(`BM10 installed transition control unavailable: ${control.id}; ${error.message}`); }
    validateBM10ControlOutput(output, control);
    outputs.push(output);
  }
  const states = collectBM10JourneyStates(outputs);
  if (!states.length) insufficient("BM10 installed Pull omitted CandidateJourneyV1 observations");
  for (const state of REQUIRED_JOURNEY_STATES) if (!states.includes(state)) insufficient(`BM10 installed Pull did not expose real transition state ${state}`);
  const native = outputs.find((output) => isObject(output?.requirementEvidenceMap) && Array.isArray(output.expectedEvidence) && Array.isArray(output.conversionReceipts) && Array.isArray(output.omissionReceipts) && Array.isArray(output.deliveryAttribution));
  if (!native) insufficient("BM10 installed Pull omitted native expected evidence, conversion/omission receipts or delivery attribution");
  const accounting = { requirementEvidenceMap: native.requirementEvidenceMap, expectedEvidence: native.expectedEvidence, conversionReceipts: native.conversionReceipts, omissionReceipts: native.omissionReceipts, deliveryAttribution: native.deliveryAttribution };
  return { releaseGeneration: path.generation, ...accounting, ...validateBM10Journey(accounting), states };
}); }
export const BM_CASES = { BM01, BM02, BM08, BM10 };
export default BM_CASES;
