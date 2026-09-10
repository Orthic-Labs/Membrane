import test from "node:test";
import assert from "node:assert/strict";
import { BM01, validateBM01Observations, validateBM02Scenario, validateBM08Admission, validateBM10Journey, validateCandidate, validateSourceDigest, extractTypedCancellationCode } from "./bm-pul-windows.mjs";

const HASH = `sha256:${"0".repeat(64)}`;
const clone = (value) => structuredClone(value);
function candidate(id) { return { id, layer: 3, provider: "blueprint", sourceKind: "graph", sourceRef: `src/${id}.rs`, sourceHash: HASH, trustClass: "workspace_tracked", instructionPolicy: "data_only", estimatedTokens: 2, exact: true, recoverable: true, resolver: "blueprint_graph_generation", text: id }; }
function set(state, candidates = []) { return { schemaVersion: 1, state, candidateCount: candidates.length, candidates, totalKnownCount: candidates.length, truncated: false, coverage: state === "resolved" ? "complete" : "partial", freshness: "current", omissions: state === "resolved" ? [] : [{ reason: state === "ambiguous" ? "same_tier_ambiguity" : "no_match" }] }; }
function resolution(state, candidates = []) { const winner = state === "resolved" ? candidates[0] : null; return { schemaVersion: 1, generationId: "gen-1", state, requestedTarget: state === "ambiguous" ? "same_name" : state === "unresolved" ? "missing" : "exact_probe", resolution: { state, resolutionTier: state === "unresolved" ? "unresolved" : "exact", requested: state === "ambiguous" ? "same_name" : state === "unresolved" ? "missing" : "exact_probe", candidates, candidateCount: candidates.length, ambiguous: state === "ambiguous", resolved: winner, omissions: state === "resolved" ? [] : [{ reason: state === "ambiguous" ? "same_tier_ambiguity" : "no_match" }] }, candidateSet: set(state, candidates) }; }

test("BM01 refuses absent installed current instead of falling back to source", () => {
  const prior = process.env.MEMBRANE_QUALIFICATION_INSTALLED_ROOT; delete process.env.MEMBRANE_QUALIFICATION_INSTALLED_ROOT;
  try { const result = BM01(); assert.equal(result.status, "failed"); assert.equal(result.evidenceKind, "installed"); assert.match(result.reason, /installed|checkout/i); }
  finally { if (prior === undefined) delete process.env.MEMBRANE_QUALIFICATION_INSTALLED_ROOT; else process.env.MEMBRANE_QUALIFICATION_INSTALLED_ROOT = prior; }
});

test("BM01 accepts typed exact, ambiguity, unknown & cancellation fixture", () => {
  const one = candidate("exact"); const two = candidate("same-a"); const three = candidate("same-b");
  assert.deepEqual(validateBM01Observations({ exact: resolution("resolved", [one]), ambiguous: resolution("ambiguous", [two, three]), unknown: resolution("unresolved"), cancellationCode: "request_cancelled" }).states, ["resolved", "ambiguous", "unresolved"]);
});
test("BM01 hostile omission/mutation controls fail", () => {
  const one = candidate("exact"); const two = candidate("same-a"); const three = candidate("same-b"); const good = { exact: resolution("resolved", [one]), ambiguous: resolution("ambiguous", [two, three]), unknown: resolution("unresolved"), cancellationCode: "deadline_exceeded" };
  for (const mutate of [
    (v) => delete v.ambiguous.resolution.candidates[1],
    (v) => { v.ambiguous.resolution.resolved = two; },
    (v) => { v.unknown.resolution.omissions = []; },
    (v) => { v.cancellationCode = "unknown"; },
    (v) => { delete v.exact.candidateSet.candidates[0].sourceRef; },
  ]) { const bad = clone(good); mutate(bad); assert.throws(() => validateBM01Observations(bad)); }
});

function bm02Control(id = "partial") { const request = { partial: { packetCharBudget: 1 }, stale: { generation: "stale-generation" }, unsupported: { consumerCapabilities: { resolvers: ["unsupported-resolver"] } }, timeout: { maxWaitMs: 1 }, unresolved_dynamic: { anchors: ["dynamic://unresolved"] } }[id] ?? {}; return { id, task: "exact_probe", request }; }
function bm02Fixture(control = bm02Control()) { const reason = { partial: "provider_partial", stale: "blueprint_stale", unsupported: "provider_capability_missing", timeout: "provider_timeout", unresolved_dynamic: "unresolved_dynamic" }[control.id]; const output = { sourceResponse: { complete: false, warnings: [{ code: reason }] }, admission: { status: control.id === "stale" ? "degraded" : control.id === "unsupported" ? "blocked" : control.id === "timeout" || control.id === "unresolved_dynamic" ? "insufficient" : "partial", omissions: [{ reason }] }, packet: { omissions: [{ reason }] }, federationMetrics: { providerTimings: [{ provider: "blueprint", status: control.id, generation: "gen-1", freshness: control.id === "stale" ? "stale" : "current", elapsedMs: 3, cancellation: control.id === "timeout", errors: [], fallback: control.id === "unsupported" ? "non_graph_sources_only" : "portable_text", candidateCount: 0 }] } }; return { invocation: { operation: "federate", request: { task: control.task, ...control.request } }, response: { schemaVersion: 1, status: 200, data: output }, output }; }
test("BM02 accepts typed partial/stale/unsupported/timeout/unresolved-dynamic fixtures", () => {
  for (const scenario of ["partial", "stale", "unsupported", "timeout", "unresolved_dynamic"]) assert.equal(validateBM02Scenario(bm02Fixture(bm02Control(scenario)), scenario, bm02Control(scenario)).controlId, scenario);
});
test("BM02 hostile outer completeness/warnings/admission/rendering controls fail", () => {
  for (const mutate of [
    (v) => { v.output.sourceResponse.complete = true; },
    (v) => { v.output.sourceResponse.warnings = []; },
    (v) => { v.output.admission.omissions = []; },
    (v) => { v.output.packet.omissions = []; },
    (v) => { v.invocation.request.task = "partial exact_probe"; },
    (v) => { v.response.schemaVersion = 2; },
  ]) { const bad = bm02Fixture(); mutate(bad); assert.throws(() => validateBM02Scenario(bad, "partial", bm02Control())); }
});
test("BM02 hostile provider diagnostic omissions/mutations fail", () => {
  for (const field of ["provider", "status", "generation", "freshness", "elapsedMs", "cancellation", "errors", "fallback", "candidateCount"]) { const bad = bm02Fixture(); delete bad.output.federationMetrics.providerTimings[0][field]; assert.throws(() => validateBM02Scenario(bad, "partial", bm02Control()), field); }
});

function bm08Fixture() { const left = candidate("left"); const right = candidate("right"); return { admissionComparison: { independent: true, queryDriven: { applicable: true, scope: "task", authority: "blueprint", budget: 1, omissions: [] }, baseline: { applicable: true, scope: "governed-baseline", authority: "cortex", budget: 1, omissions: [] } }, packet: { blocks: [left, right], omissions: [] }, receipts: [{ id: "left", sourceRef: left.sourceRef, contentSha256: HASH, decision: "admitted" }, { id: "right", sourceRef: right.sourceRef, contentSha256: HASH, decision: "admitted" }], atomicEvidencePaths: [{ candidateId: "left", sourceRef: left.sourceRef, sourceHash: HASH, complete: true, provenance: { generation: "gen-1" } }, { candidateId: "right", sourceRef: right.sourceRef, sourceHash: HASH, complete: true, provenance: { generation: "gen-1" } }], contradictionPairs: [{ leftId: "left", rightId: "right", required: true, present: true }], budgetReduction: { misleadingFragmentRetained: false, omissionReasons: ["budget_exhausted"] }, ambiguityDisposition: { action: "block", candidates: ["left", "right"], selectedId: null } }; }
test("BM08 accepts typed atomic admission fixture", () => { const result = validateBM08Admission(bm08Fixture()); assert.equal(result.disposition, "block"); });
test("BM08 hostile omission/mutation controls fail", () => {
  for (const mutate of [
    (v) => { v.admissionComparison.independent = false; },
    (v) => { delete v.admissionComparison.baseline; },
    (v) => { delete v.atomicEvidencePaths[0].provenance; },
    (v) => { v.atomicEvidencePaths[0].complete = false; },
    (v) => { v.contradictionPairs[0].present = false; },
    (v) => { v.budgetReduction.misleadingFragmentRetained = true; },
    (v) => { v.ambiguityDisposition.action = "winner"; },
    (v) => { v.ambiguityDisposition.candidates = ["left"]; },
    (v) => { v.receipts[0].sourceRef = "wrong.rs"; },
  ]) { const bad = clone(bm08Fixture()); mutate(bad); assert.throws(() => validateBM08Admission(bad)); }
});

function bm10Fixture() { const states = ["NOT_DISCOVERED", "DISCOVERED_REJECTED", "DISCOVERED_BUDGET_DROPPED", "STALE", "ADAPTER_DROPPED", "EXECUTION_FAILURE"]; const journeys = states.map((state, index) => ({ evidenceId: `evidence:${index}`, requirementBindingDigest: `binding-${index}`, dimension: "CurrentState", provider: "blueprint", sourceHash: HASH, representationDigest: HASH, acquired: true, eligible: true, admitted: state === "DISCOVERED_REJECTED" ? false : true, represented: false, fenced: false, emitted: false, retained: false, dropped: true, state })); return { requirementEvidenceMap: { schemaVersion: 1, taskId: "bm10", journeys, unsatisfied: ["CurrentState"] }, expectedEvidence: journeys.map((journey) => ({ evidenceId: journey.evidenceId, required: true })), conversionReceipts: journeys.map((journey, index) => ({ receiptId: `convert-${index}`, evidenceId: journey.evidenceId, reason: "converted" })), omissionReceipts: journeys.map((journey, index) => ({ receiptId: `omit-${index}`, evidenceId: journey.evidenceId, reason: "omitted" })), deliveryAttribution: journeys.map((journey) => ({ evidenceId: journey.evidenceId, emitted: false, hostIncluded: false, modelUsed: false, helped: false })) }; }
test("BM10 accepts all typed CandidateJourneyV1 outcomes & accounting", () => { const result = validateBM10Journey(bm10Fixture()); assert.equal(result.states.length, 6); assert.equal(result.expectedCount, 6); });
test("BM10 hostile state/ID/receipt/attribution controls fail", () => {
  for (const mutate of [
    (v) => { v.requirementEvidenceMap.journeys.pop(); },
    (v) => { v.requirementEvidenceMap.journeys[0].evidenceId = ""; },
    (v) => { v.expectedEvidence[0].evidenceId = "missing"; },
    (v) => { v.conversionReceipts = []; },
    (v) => { v.omissionReceipts[0].evidenceId = "missing"; },
    (v) => { v.deliveryAttribution[0].hostIncluded = true; },
    (v) => { v.deliveryAttribution[0].outcome = "DELIVERED_IGNORED"; },
  ]) { const bad = clone(bm10Fixture()); mutate(bad); assert.throws(() => validateBM10Journey(bad)); }
});

test("source digest validator accepts supported sha256 & xxh128 forms", () => {
  assert.equal(validateSourceDigest(HASH), HASH);
  const xxh = `xxh128:${"a".repeat(32)}`;
  assert.equal(validateSourceDigest(xxh), xxh);
  const xxhCandidate = candidate("xxh"); xxhCandidate.sourceHash = xxh;
  assert.equal(validateCandidate(xxhCandidate).sourceHash, xxh);
});
test("source digest validator rejects unsupported algorithms & lengths", () => {
  for (const value of ["md5:" + "0".repeat(32), "xxh64:" + "0".repeat(16), "xxh128:" + "0".repeat(31), "xxh128:" + "0".repeat(33), "sha256:" + "0".repeat(63), "sha256:" + "0".repeat(65)]) assert.throws(() => validateSourceDigest(value));
  const badSource = candidate("bad-source"); badSource.sourceHash = "xxh128:" + "0".repeat(31); assert.throws(() => validateCandidate(badSource));
});
test("extractTypedCancellationCode accepts the installed CLI's 'membrane: <code>: <message>' form", () => {
  assert.equal(extractTypedCancellationCode("membrane: request_cancelled: request cancelled", undefined), "request_cancelled");
  assert.equal(extractTypedCancellationCode("membrane: deadline_exceeded: deadline exceeded", undefined), "deadline_exceeded");
});
test("extractTypedCancellationCode accepts structured JSON error/code/reason fields", () => {
  assert.equal(extractTypedCancellationCode("", { code: "request_cancelled", message: "request cancelled" }), "request_cancelled");
  assert.equal(extractTypedCancellationCode("", { error: { code: "cancelled" } }), "cancelled");
  assert.equal(extractTypedCancellationCode("", { reason: "deadline_exceeded" }), "deadline_exceeded");
});
test("extractTypedCancellationCode still accepts the legacy prefixed form", () => {
  assert.equal(extractTypedCancellationCode("error: request_cancelled", undefined), "request_cancelled");
});
test("extractTypedCancellationCode refuses an untyped message (negative control)", () => {
  assert.equal(extractTypedCancellationCode("membrane: request cancelled", undefined), undefined);
  assert.equal(extractTypedCancellationCode("something went wrong, operation aborted", undefined), undefined);
  assert.equal(extractTypedCancellationCode("", { message: "the request was aborted" }), undefined);
});

test("representation & content digests remain strict sha256", () => {
  const badJourney = bm10Fixture(); badJourney.requirementEvidenceMap.journeys[0].representationDigest = `xxh128:${"a".repeat(32)}`; assert.throws(() => validateBM10Journey(badJourney));
  const badReceipt = bm08Fixture(); badReceipt.receipts[0].contentSha256 = `xxh128:${"a".repeat(32)}`; assert.throws(() => validateBM08Admission(badReceipt));
});
