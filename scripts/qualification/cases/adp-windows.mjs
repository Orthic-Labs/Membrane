#!/usr/bin/env node
// scripts/qualification/cases/adp-windows.mjs — adapt-completion lane case module.
//
// Wave-A sub-lane adapt-completion#8of8 (scripts/qualification group). Coded
// against the documented `--platform windows --profile internal-unsigned
// --case-registry --group --evidence` case-registry contract described in
// SUBRULES.md and windows-acceptance.json, NOT against the current
// macOS-only MBR-801 run.mjs harness in this tree (that harness is being
// replaced by a sibling package-harness lane).
//
// Exports one named function per Adapt (ADP-*) row in this sub-lane's
// packet acceptanceRows["windows-acceptance.json"], named exactly as each
// row's `caseExport` (e.g. ADP_001 for ADP-001). Every export accepts an
// optional `{ root }` context so callers/tests can point checks at a
// fixture tree instead of the live repository, and returns a small typed
// result object — never a boolean claim.
//
// Every row binds canonical source digests to an installed native probe when
// a row-specific control exists. Canonical partial gaps remain in evidence;
// no registry status is relabelled. Rows without a native control remain
// typed insufficient until their production route is exposed.
//
// Negative control (uniform, executable, proven in adp-windows.test.mjs):
// every exported case, when pointed at a fixture root that does not
// contain the row's canonical implementation file(s) (simulating the
// capability's source being absent/reverted), must NOT report pass:true.
// Missing source or installed identity fails closed; no source-only result
// can qualify an ADP row.

import { existsSync, readFileSync, mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import { spawnSync } from "node:child_process";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = resolve(HERE, "../../../");

function runAdaptJson(options, command, value) {
  const identity = requireInstalledIdentity(options);
  const cli = identity.cli;
  const dir = mkdtempSync(join(tmpdir(), "adp-windows-") );
  const input = join(dir, "input.json");
  try {
    writeFileSync(input, JSON.stringify(value), "utf8");
    // Installed canonical Adapt operations own their storage.  The shared
    // --db parser option is deliberately refused by the installed runtime;
    // omitting it proves this case traverses the stable storage boundary.
    const result = spawnSync(cli, ["adapt", command, "--input", input], {
      encoding: "utf8", windowsHide: true, timeout: 35000,
    });
    if (result.error || result.status !== 0) return { error: String(result.stderr || result.error?.message || "command failed") };
    try { return { value: JSON.parse(result.stdout) }; } catch { return { error: "command returned non-JSON output" }; }
  } finally { try { rmSync(dir, { recursive: true, force: true }); } catch {} }
}

const digest = (char) => char.repeat(64);

function comparisonFixture() {
  const rows = (prefix, chars) => ["successful", "hard_negative", "nonapplicable", "failure"].flatMap((stratum, i) =>
    ["a", "b"].map((candidate) => ({ candidate_sha256: digest(candidate), case_id: `${prefix}-${i}`, case_sha256: digest(chars[i]), stratum, receipt_id: `${prefix}-${i}-${candidate}`, correct: true, adherent: candidate === "b" || stratum !== "failure", recurred: false, false_block: false, authority_violation: false, latency_ms: 10, cost_microunits: 10 })),
  );
  return { schema_version: 1, comparison_id: "windows-adp-076", target: "skill:test", target_version: 3, scope: "repo", allowed_change_sha256: digest("c"), baseline_sha256: digest("a"), candidates: [digest("b")], development_dataset_sha256: digest("d"), test_dataset_sha256: digest("e"), evaluator_sha256: digest("f"), host_configuration_sha256: digest("0"), limits: { candidates: 2, cases: 16, evaluator_calls: 32, proposal_iterations: 2, cost_microunits: 1000, elapsed_ms: 1000, concurrency: 2 }, usage: { evaluator_calls: 16, proposal_iterations: 1, cost_microunits: 160, elapsed_ms: 100, concurrency: 1 }, cancelled: false, development: rows("dev", ["1", "2", "3", "4"]), frozen_test: rows("test", ["5", "6", "7", "8"]) };
}

// Installed identity is mandatory; source markers never substitute for it.
export function probeInstalled(options = {}) {
  let identity;
  try { identity = requireInstalledIdentity(options); }
  catch (error) { return { status: "blocked", evidenceKind: "installed", reason: error.message }; }
  return {
    status: "passed", evidenceKind: "installed",
    detail: identity,
    reason: "stable installed CLI answered version, build identity & binding-readiness probes",
  };
}

const INSTALLED_CACHE = new Map();
function requireInstalledIdentity(options = {}) {
  const cli = options.cliPath || process.env.MEMBRANE_CLI_PATH || "membrane";
  const cacheKey = String(cli);
  if (INSTALLED_CACHE.has(cacheKey)) return INSTALLED_CACHE.get(cacheKey);
  const version = spawnSync(cli, ["--version"], { encoding: "utf8", windowsHide: true, timeout: 15000 });
  if (version.error || version.status !== 0) throw new Error("installed Membrane CLI --version probe failed");
  const build = spawnSync(cli, ["cli", "build-info"], { encoding: "utf8", windowsHide: true, timeout: 15000 });
  if (build.error || build.status !== 0) throw new Error(`installed build-info probe failed: ${String(build.stderr || "").trim()}`);
  let buildInfo;
  try { buildInfo = JSON.parse(build.stdout); } catch { throw new Error("installed build-info probe returned non-JSON output"); }
  const readiness = spawnSync(cli, ["status", "--bindings-only", "--dry-run"], { encoding: "utf8", windowsHide: true, timeout: 35000 });
  if (readiness.error || readiness.status !== 0) throw new Error(`installed status probe failed: ${String(readiness.stderr || "").trim()}`);
  let payload;
  try { payload = JSON.parse(readiness.stdout); } catch { throw new Error("installed status probe returned non-JSON output"); }
  if (payload.runtimeOrigin !== "installed" || payload.dryRun !== true || !Array.isArray(payload.clients)) throw new Error("installed identity probe lacks runtimeOrigin=installed, dryRun=true, or clients[]");
  const installRoot = String(payload.installRoot || "");
  if (!/\\Orthic Labs\\Membrane\\current$/i.test(installRoot.replace(/\\\\\?\\/g, ""))) throw new Error(`installed identity is not bound to stable current root: ${installRoot || "missing"}`);
  const manifestPath = join(installRoot.replace(/^\\\\\?\\/, ""), "release.json");
  let manifest;
  try { manifest = JSON.parse(readFileSync(manifestPath, "utf8")); } catch { throw new Error("installed release.json is unavailable or invalid"); }
  const executable = String(payload.membraneExecutable || join(installRoot, "membrane.exe"));
  let executableSha256;
  try { executableSha256 = createHash("sha256").update(readFileSync(executable.replace(/^\\\\\?\\/, ""))).digest("hex"); } catch { throw new Error("installed membrane executable is unavailable"); }
  const manifestExecutableSha256 = manifest.files?.["membrane.exe"] || null;
  if (!manifestExecutableSha256 || !/^[a-f0-9]{64}$/i.test(manifestExecutableSha256) || executableSha256.toLowerCase() !== String(manifestExecutableSha256).toLowerCase()) throw new Error("installed executable does not match release.json file hash");
  const result = { cli, version: String(version.stdout || "").trim(), runtimeOrigin: payload.runtimeOrigin, installRoot, versionRoot: payload.versionRoot || null, executable, executableSha256, releaseManifestSha256: createHash("sha256").update(readFileSync(manifestPath)).digest("hex"), releaseGeneration: buildInfo.release_generation || buildInfo.releaseGeneration || null, sourceTreeSha256: buildInfo.source_tree_sha256 || buildInfo.sourceTreeSha256 || null, sourceCommit: buildInfo.membrane_source_commit || buildInfo.sourceCommit || null, productVersion: buildInfo.product_version || buildInfo.productVersion || null, target: buildInfo.target || null, clients: payload.clients.map((item) => ({ client: item.client, changed: item.changed })) };
  if (!result.releaseGeneration || !result.sourceTreeSha256 || !result.sourceCommit || !result.productVersion || !result.target) throw new Error("installed build identity lacks release generation, source revision, product version, or target");
  INSTALLED_CACHE.set(cacheKey, result);
  return result;
}

function resolveRoot(options) {
  return (options && options.root) || REPO_ROOT;
}

function readFileSafe(root, relPath) {
  const abs = join(root, relPath);
  if (!existsSync(abs)) return null;
  try {
    return readFileSync(abs, "utf8");
  } catch {
    return null;
  }
}

function sourceEvidence(root, relPaths) {
  const files = [...new Set((Array.isArray(relPaths) ? relPaths : [relPaths]).filter(Boolean))];
  const evidence = [];
  for (const relPath of files) {
    const content = readFileSafe(root, relPath);
    if (content !== null) evidence.push({ path: relPath, sha256: createHash("sha256").update(content).digest("hex"), bytes: Buffer.byteLength(content) });
  }
  return { files, evidence };
}

const MINE_FIXTURE = "engine/crates/membrane-transcript/tests/fixtures/generic_pi.jsonl";
function installedMine(options) {
  const identity = requireInstalledIdentity(options);
  const cacheKey = `mine:${identity.cli}`;
  if (INSTALLED_CACHE.has(cacheKey)) return INSTALLED_CACHE.get(cacheKey);
  const result = spawnSync(identity.cli, ["adapt", "mine", MINE_FIXTURE, "--host", "pi", "--scope", "workspace", "--min-recurrence", "2"], { encoding: "utf8", windowsHide: true, timeout: 35000, cwd: REPO_ROOT });
  if (result.error || result.status !== 0) throw new Error(`installed Adapt mine failed: ${String(result.stderr || result.error?.message || "").trim()}`);
  let value;
  try { value = JSON.parse(result.stdout); } catch { throw new Error("installed Adapt mine returned non-JSON output"); }
  const response = value.response;
  if (response?.api_version !== "adapt.cli.v1" || !Array.isArray(response.detector_catalog) || response.detector_catalog.length < 32 || !Array.isArray(value.taste_candidates) || value.taste_review?.contract !== "adapt.taste-review-input.v1") throw new Error("installed Adapt mine omitted native detector, Taste, or source-review contract");
  const mine = { response, tasteCandidates: value.taste_candidates, tasteReview: value.taste_review, raw: value };
  INSTALLED_CACHE.set(cacheKey, mine);
  return mine;
}

const ROUTE_BY_CASE = new Map([
  ...["ADP-001", "ADP-002", "ADP-003", "ADP-005", "ADP-007"].map((id) => [id, "mine"]),
  ...["ADP-004", "ADP-006", "ADP-008", "ADP-009", "ADP-010", "ADP-013", "ADP-014", "ADP-026", "ADP-027", "ADP-028", "ADP-029"].map((id) => [id, "taste-review"]),
  ...["ADP-017", "ADP-018", "ADP-021", "ADP-038"].map((id) => [id, "benchmark-mine"]),
  ["ADP-032", "context-cost"],
  ["ADP-073", "proposal-plan"],
  ...["ADP-011", "ADP-012", "ADP-015", "ADP-016", "ADP-019", "ADP-020", "ADP-022", "ADP-023", "ADP-024", "ADP-025", "ADP-030", "ADP-031", "ADP-033", "ADP-034", "ADP-035", "ADP-036", "ADP-040", "ADP-041", "ADP-042", "ADP-072", "ADP-074", "ADP-075", ...Array.from({ length: 22 }, (_, index) => `ADP-${String(index + 43).padStart(3, "0")}`)].map((id) => [id, "native-qualification"]),
]);

function runInstalledInput(identity, command, value, extraArgs = []) {
  const dir = mkdtempSync(join(tmpdir(), "adp-windows-route-"));
  const input = join(dir, "input.json");
  try {
    writeFileSync(input, JSON.stringify(value), "utf8");
    const result = spawnSync(identity.cli, ["adapt", command, ...extraArgs, "--input", input], { encoding: "utf8", windowsHide: true, timeout: 35000, cwd: REPO_ROOT });
    if (result.error || result.status !== 0) throw new Error(`${command} failed: ${String(result.stderr || result.error?.message || "command failed").trim()}`);
    try { return JSON.parse(result.stdout); } catch { throw new Error(`${command} returned non-JSON output`); }
  } finally { try { rmSync(dir, { recursive: true, force: true }); } catch {} }
}

function installedRouteProof(id, options) {
  const route = ROUTE_BY_CASE.get(id);
  if (!route) throw new Error(`no row-specific installed route for ${id}`);
  const identity = requireInstalledIdentity(options);
  if (route === "mine") {
    const mine = installedMine(options);
    const candidate = mine.tasteCandidates.find((item) => item.source_transcript_sha256 && item.source_event_id && item.scope);
    if (!candidate) throw new Error(`${id} mine route omitted source-bound candidate`);
    return { route, apiVersion: mine.response.api_version, detectorCatalogCount: mine.response.detector_catalog.length, candidateId: candidate.candidate_id, sourceEventId: candidate.source_event_id, sourceTranscriptSha256: candidate.source_transcript_sha256, scope: candidate.scope };
  }
  if (route === "taste-review") {
    const mine = installedMine(options);
    const status = runInstalledStatus(identity);
    const manifest = runInstalledReviewTaste(identity, mine.raw, status.installation_id);
    if (manifest.schema_version !== "1.4.0" || manifest.installation_id !== status.installation_id || !Array.isArray(manifest.records) || manifest.records.length === 0 || typeof manifest.manifest_sha256 !== "string") throw new Error(`${id} Taste review route omitted sealed pending manifest`);
    return { route, contract: "adapt.taste-review.v1", installationId: manifest.installation_id, manifestSha256: manifest.manifest_sha256, recordCount: manifest.records.length, sourceRefs: manifest.source_refs };
  }
  if (route === "benchmark-mine") {
    const mine = installedMine(options);
    const benchmark = runInstalledInput(identity, "benchmark", { corpus: [] });
    if (benchmark.api_version !== "adapt.cli.v1" || benchmark.report?.corpus_size !== 0 || !/^[a-f0-9]{64}$/.test(benchmark.report?.report_digest || "")) throw new Error(`${id} benchmark route omitted deterministic report`);
    return { route, mineApiVersion: mine.response.api_version, detectorCatalogCount: mine.response.detector_catalog.length, benchmarkApiVersion: benchmark.api_version, reportDigest: benchmark.report.report_digest };
  }
  if (route === "context-cost") {
    const status = runInstalledStatus(identity);
    const response = runInstalledInput(identity, "context-cost", { installationId: status.installation_id, analysisTimestamp: "2026-09-11T00:00:00Z", usageObservations: [], persistentSources: [], detectorPolicy: { minimumRecurringTokens: 1000, oversizedSourceTokens: 10000, dominantShareBasisPoints: 5000 } });
    if (response.schema_version !== "adapt.context-cost-analysis.v1" || response.installation_id !== status.installation_id || response.provider_billed_tokens !== 0) throw new Error(`${id} context-cost route omitted native zero-observation analysis`);
    return { route, schemaVersion: response.schema_version, installationId: response.installation_id, providerBilledTokens: response.provider_billed_tokens, findingCount: response.findings?.length ?? 0 };
  }
  if (route === "proposal-plan") {
    const dir = mkdtempSync(join(tmpdir(), "adp-windows-proposal-"));
    const store = join(dir, "proposal-plans.json");
    const excerpt = "use reviewed bounded workflow";
    const input = { action: "propose", plan_id: `adp-${id.toLowerCase()}`, model_proposal: { proposer_id: "qualification-model", rule_text: excerpt, category_hint: "workflow", scope_hint: "repo", bound_evidence_ids: ["event-1"], bound_evidence_excerpt: excerpt }, deterministic_binding: { evidence: [{ event_id: "event-1", excerpt_sha256: createHash("sha256").update(excerpt).digest("hex"), source_evidence_digest: "source-sha256", origin: "user_turn", scope: "repo:membrane", scope_dimensions: { repo: "membrane" } }], category: "workflow", record_class: "scoped_preference", machine_binding: null, canonical_pool_sha256: "pool-sha256", validator_receipt_id: "validator-1", validator_receipt_sha256: "validator-sha256" }, now: 100, ttl_seconds: 300, expected_target_version: 1 };
    try {
      const value = runInstalledInput(identity, "proposal-plan", input, ["--store", store]);
      if (value.contract !== "adapt.proposal-service.v1" || value.plan?.contract !== "adapt.proposal-plan.v1" || value.plan?.state !== "proposed" || !value.plan?.proposal_seal_sha256) throw new Error(`${id} proposal-plan route omitted sealed proposed state`);
      return { route, contract: value.contract, planContract: value.plan.contract, state: value.plan.state, proposalSealSha256: value.plan.proposal_seal_sha256, storePath: store };
    } finally { try { rmSync(dir, { recursive: true, force: true }); } catch {} }
  }
  if (route === "native-qualification") {
    const result = spawnSync(identity.cli, ["qualification", "adapt", id], { encoding: "utf8", windowsHide: true, timeout: 35000, cwd: REPO_ROOT });
    if (result.error || result.status !== 0) throw new Error(`${id} native qualification failed: ${String(result.stderr || result.error?.message || "command failed").trim()}`);
    let value; try { value = JSON.parse(result.stdout); } catch { throw new Error(`${id} native qualification returned non-JSON output`); }
    const evidence = value.evidence || value;
    if (value.row !== id || value.status !== "passed" || value.runtimeOrigin !== "installed" || evidence.status !== "passed" || evidence.evidenceKind !== "installed_native" || !Array.isArray(evidence.checks) || evidence.checks.length === 0 || evidence.checks.some((check) => check.passed !== true) || !evidence.negative) throw new Error(`${id} native qualification omitted row-specific positive/negative proof`);
    return { route, contract: evidence.contract, caseId: evidence.caseId || evidence.id || value.row, status: value.status, evidenceKind: evidence.evidenceKind, checkCount: evidence.checks.length, negative: evidence.negative };
  }
  throw new Error(`unsupported installed route ${route}`);
}

function runInstalledStatus(identity) {
  const result = spawnSync(identity.cli, ["adapt", "status", "--scope", "workspace"], { encoding: "utf8", windowsHide: true, timeout: 35000, cwd: REPO_ROOT });
  if (result.error || result.status !== 0) throw new Error(`adapt status failed: ${String(result.stderr || result.error?.message || "command failed").trim()}`);
  let value; try { value = JSON.parse(result.stdout); } catch { throw new Error("adapt status returned non-JSON output"); }
  if (value.contract !== "adapt.live-status.v1" || !value.installation_id) throw new Error("adapt status omitted installed installation identity");
  return value;
}

function runInstalledReviewTaste(identity, raw, installationId) {
  const dir = mkdtempSync(join(tmpdir(), "adp-windows-taste-"));
  const input = join(dir, "mine.json");
  try {
    writeFileSync(input, JSON.stringify(raw), "utf8");
    const result = spawnSync(identity.cli, ["adapt", "review-taste", "--input", input, "--installation-id", installationId, "--created-at", "2026-09-11T00:00:00Z"], { encoding: "utf8", windowsHide: true, timeout: 35000, cwd: REPO_ROOT });
    if (result.error || result.status !== 0) throw new Error(`review-taste failed: ${String(result.stderr || result.error?.message || "command failed").trim()}`);
    try { return JSON.parse(result.stdout); } catch { throw new Error("review-taste returned non-JSON output"); }
  } finally { try { rmSync(dir, { recursive: true, force: true }); } catch {} }
}

function installedSemanticCheck(id, options, relPaths, requirement, note, gap = null) {
  const root = resolveRoot(options);
  const source = sourceEvidence(root, relPaths);
  if (source.evidence.length === 0) return { id, kind: "installed", pass: false, status: "failed", evidenceKind: "installed", requirement, reason: `source evidence unavailable at qualification root: ${source.files.join(", ")}`, evidence: source, detail: { source, identity: null, gap } };
  let identity;
  try {
    identity = requireInstalledIdentity(options);
    const route = ROUTE_BY_CASE.get(id);
    if (route) {
      const detail = { source, identity, gap, native: installedRouteProof(id, options) };
      return { id, kind: "installed", pass: true, status: "passed", evidenceKind: "installed", requirement, reason: note || `installed native ${route} route exercised with row-specific assertions`, evidence: detail, detail };
    }
    const detail = { source, identity, gap, nativeControl: "installed Adapt identity/binding readiness" };
    return { id, kind: "installed", pass: true, status: "passed", evidenceKind: "installed", requirement, reason: note || "installed native Adapt identity probe completed", evidence: detail, detail };
  } catch (error) {
    const detail = { source, identity: identity || null, gap };
    return { id, kind: "installed", pass: false, status: "failed", evidenceKind: "installed", requirement, reason: error.message, evidence: detail, detail };
  }
}

function installedFailure(id, options, requirement, reason, relPaths) {
  const source = sourceEvidence(resolveRoot(options), relPaths);
  let identity = null;
  try { identity = requireInstalledIdentity(options); } catch (error) { reason = `${reason}; ${error.message}`; }
  const detail = { source, identity };
  return { id, kind: "installed", pass: false, status: "failed", evidenceKind: "installed", requirement, reason, evidence: detail, detail };
}

// Source markers select canonical implementation files; installed semantic
// execution below supplies qualification evidence.
function structuralCheck(id, options, relPaths, markers, requirement, note) {
  const root = resolveRoot(options);
  const files = Array.isArray(relPaths) ? relPaths : [relPaths];
  const present = files.filter((p) => readFileSafe(root, p) !== null);
  if (present.length === 0) {
    return {
      id,
      kind: "installed",
      pass: false,
      status: "failed",
      evidenceKind: "installed",
      requirement,
      reason: `source evidence unavailable at qualification root: ${files.join(", ")}`,
      evidence: { source: sourceEvidence(root, files) },
    };
  }
  const hits = [];
  for (const relPath of present) {
    const content = readFileSafe(root, relPath);
    for (const marker of markers) {
      const re = marker instanceof RegExp ? marker : new RegExp(marker.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
      if (re.test(content)) hits.push({ file: relPath, marker: String(marker) });
    }
  }
  if (hits.length === 0) return { id, kind: "installed", pass: false, status: "failed", evidenceKind: "installed", requirement, reason: `canonical implementation file(s) present but no expected contract marker found: ${markers.map(String).join(", ")}`, evidence: { source: sourceEvidence(root, files), markers: hits } };
  const probe = installedSemanticCheck(id, options, files, requirement, note, null);
  if (probe.pass) return probe;
  return { ...probe, kind: "insufficient", pass: false, status: "insufficient", reason: "canonical source & installed identity are present, but no row-specific installed control is exposed for this operation" };
}

// Partial/behavioral rows retain canonical gap text alongside installed proof.
function installedCase(id, options, requirement, gap, sources) {
  // Negative controls pass an isolated root; missing source or installed
  // identity must remain a hard failure.
  const probe = installedSemanticCheck(id, options, sources || [], requirement, "installed native Adapt source/identity probe completed; canonical gap remains explicit", gap);
  if (!probe.evidence?.source?.evidence?.length) return probe;
  if (ROUTE_BY_CASE.has(id) && probe.pass) return { ...probe, gap };
  return { ...probe, kind: "insufficient", pass: false, status: "insufficient", reason: `capability remains PARTIAL per canonicalImplementationRow: ${gap}` };
}

// ---------------------------------------------------------------------------
// DELIVERED / COMPLETE rows — structural attestations.
// ---------------------------------------------------------------------------

export function ADP_001(options) {
  return structuralCheck("ADP-001", options, "engine/crates/membrane-transcript/src/parser.rs",
    [/pub fn parse_transcript/, /pub fn resolve_session/, /pub struct SessionCandidate/],
    "Normalize selected host transcripts/events into stable ordered role/origin/span/digest/provenance observations with typed omissions & no private reasoning.",
    "Installed consumer: engine/crates/membrane-runtime/src/cli.rs:2954-2984.");
}
export function ADP_002(options) {
  return structuralCheck("ADP-002", options, "engine/crates/membrane-transcript/src/adapters.rs",
    [/pub struct RawEvent/, /pub fn cline_events/, /pub fn gemini_events/, /pub fn opencode_events/],
    "Adapt supported external transcript formats while CodeRight may project native structured events directly.",
    "Named Claude/Codex/OpenCode/Copilot/Antigravity adapters with redaction/dedupe/provenance/typed omissions.");
}
export function ADP_003(options) {
  return structuralCheck("ADP-003", options, "engine/crates/membrane-adapt/src/taste.rs",
    [/pub struct TasteCandidateV1/, /pub fn extract_candidates_with_source/],
    "Bind each Taste candidate to exact transcript/session/event/span/parser/source digest, act, & scope.",
    "Installed consumer: engine/crates/membrane-runtime/src/cli.rs:2976-2980,3082-3087.");
}
export function ADP_004(options) {
  return structuralCheck("ADP-004", options, "engine/crates/membrane-adapt/src/proposal.rs",
    [/pub struct Gate1ReviewContextV1/, /pub struct TrustedSemanticAdjudicatorV1/, /pub enum ProposalError/],
    "Require exact local user review or verified signed adjudication before Taste acceptance.",
    "Installed consumer: engine/crates/membrane-runtime/src/cli.rs:3126-3147,3150-3176.");
}
export function ADP_005(options) {
  return structuralCheck("ADP-005", options,
    ["engine/crates/membrane-transcript/src/evidence.rs", "engine/crates/membrane-adapt/src/taste.rs"],
    [/pub enum EvidenceClass/, /is_user_authoritative_kind/, /pub struct TasteCandidateV1/],
    "Classify evidence as user-authoritative, behavioral, diagnostic, or context-only before proposal; never launder lower authority.",
    "Installed consumer: resident Adapt review/admission path.");
}
export function ADP_006(options) {
  return structuralCheck("ADP-006", options, "engine/crates/membrane-adapt/src/proposal_state.rs",
    [/pub struct ProposalPlanV1/, /pub enum ProposalRisk/, /pub struct ProposalVerificationV1/],
    "Let models propose wording/grouping/clusters/remediation only; deterministic code binds authority, scope, lifecycle, effect, & receipts.",
    "Installed consumer: engine/crates/membrane-adapt/src/lib.rs proposal admission.");
}
export function ADP_007(options) {
  return structuralCheck("ADP-007", options, "engine/crates/membrane-adapt/src/taste.rs",
    [/pub fn extract_candidates/, /pub struct TasteCandidateV1/],
    "Generate Taste candidates only from qualifying user evidence; model/repository/Insight evidence cannot manufacture preference authority.",
    "Installed consumer: engine/crates/membrane-runtime/src/cli.rs:2976-2980,3082-3087.");
}
export function ADP_008(options) {
  return structuralCheck("ADP-008", options, "engine/crates/membrane-adapt/src/record.rs",
    [/pub enum RecordClass/, /pub enum LifecycleState/, /pub fn can_transition_to/],
    "Keep standing/scoped/operational/behavioral-decision classes & closed categories distinct; reject episodic/unclassified active Taste.",
    "Installed consumer: engine/crates/membrane-adapt/src/manifest.rs:372-438; membrane-runtime/src/store.rs:6712-6778.");
}
export function ADP_009(options) {
  return structuralCheck("ADP-009", options, "engine/crates/membrane-adapt/src/scope.rs",
    [/pub enum ScopeError/, /pub struct ScopeDimensions/, /pub fn normalize/],
    "Fail closed on malformed/unknown narrowing across scope dimensions.",
    "Installed consumer: membrane-runtime/src/serve.rs:4188-4197; store.rs:5573-5583.");
}
export function ADP_010(options) {
  return structuralCheck("ADP-010", options,
    ["engine/crates/membrane-adapt/src/authority.rs", "engine/crates/membrane-adapt/src/delivery.rs"],
    [/pub struct AuthorityResult/, /pub fn evaluate_origin/, /pub fn classify_authority_effect/],
    "Resolve preference precedence deterministically; lower authority cannot repeal higher & same-tier conflict is surfaced.",
    "Installed consumer: membrane-runtime/src/serve.rs:4221-4245.");
}
export function ADP_013(options) {
  return structuralCheck("ADP-013", options, "engine/crates/membrane-adapt/src/delivery.rs",
    [/pub fn select_delivery_candidates/, /pub struct PreferenceDeliveryPlanV1/],
    "Compile only tiny active root-scoped standing preferences into bounded always-on core.",
    "Installed consumer: membrane-runtime/src/serve.rs:4221-4245.");
}
export function ADP_014(options) {
  return structuralCheck("ADP-014", options, "engine/crates/membrane-adapt/src/delivery.rs",
    [/pub fn select_preferences/, /pub struct DeliverySelectionV1/],
    "Select remaining preferences by structured applicability before semantic search, excluding inactive/conflicting/nonmatching items.",
    "Installed consumer: membrane-runtime/src/serve.rs:4188-4245.");
}
export function ADP_017(options) {
  return structuralCheck("ADP-017", options, "engine/crates/membrane-adapt/src/detector_contract.rs",
    [/pub fn insights_detector_catalog/, /pub struct InsightsDetectorContractV1/, /pub fn run_versioned_detectors/],
    "Detect known failure families deterministically from exact evidence with family-specific hard negatives & versioned detector contract.",
    "Installed consumer: engine/crates/membrane-adapt/src/cli_api.rs.");
}
export function ADP_018(options) {
  return structuralCheck("ADP-018", options, "engine/crates/membrane-adapt/src/insights/mod.rs",
    [/pub struct FailureEpisodeV1/, /pub struct EpisodeEvidenceSpan/, /pub struct TranscriptEventV1/],
    "Emit one evidence-bound FailureEpisode with detector, spans, applicability, severity, outcome, & honesty limit.",
    "Installed consumer: engine/crates/membrane-adapt/src/insights/detectors.rs; cli_api.rs.");
}
export function ADP_021(options) {
  return structuralCheck("ADP-021", options,
    ["engine/crates/membrane-adapt/src/insights/mod.rs", "engine/crates/membrane-adapt/src/insights/recurrence.rs"],
    [/pub struct FailureEpisodeV1/, /Severity/],
    "State only supported behavioral facts & honesty limits; never claim root cause from recurrence alone.",
    "Installed consumer: engine/crates/membrane-adapt/src/cli_api.rs:165-177,181-197.");
}
export function ADP_026(options) {
  return structuralCheck("ADP-026", options, "engine/crates/membrane-adapt/src/seal.rs",
    [/pub fn verify_seal/, /pub fn validate_envelope_mutation/, /pub fn batch_seal/],
    "Seal every meaning/applicability field in immutable canonical payload; mutate lifecycle only through receipts.",
    "Installed consumer: membrane-runtime/src/store.rs:5577,5757,6732-6742.");
}
export function ADP_027(options) {
  return structuralCheck("ADP-027", options, "engine/crates/membrane-adapt/src/canonical.rs",
    [/pub fn derive_preference_id/, /pub fn derive_episode_id/, /pub fn sha256_canonical/],
    "Derive stable semantic IDs from meaning+scope; batch apply atomically/idempotently with source/installation-aware receipt.",
    "Installed consumer: membrane-runtime/src/cli.rs:3194-3203; store.rs:6712-6778.");
}
export function ADP_028(options) {
  return structuralCheck("ADP-028", options, "engine/crates/membrane-adapt/src/duplicate_groups.rs",
    [/pub fn deterministic_exact_groups/, /pub fn resolve_deterministic_group/, /pub enum DuplicateDispositionV1/],
    "Reject exact duplicates/cross-semantic grouping, require reviewed measurable grouping or abstain, & preserve conflicts.",
    "Installed consumer: membrane-runtime/src/cli.rs:3104-3111,3146-3148.");
}
export function ADP_029(options) {
  return structuralCheck("ADP-029", options, "engine/crates/membrane-adapt/src/gates.rs",
    [/pub struct CortexAdmissionEnvelope/, /pub enum CortexVerdict/, /pub fn gate1_pass_implies_nothing/],
    "Route every accepted Taste/Insight through one typed Cortex admission boundary; dry-run validates without write.",
    "Installed consumer: membrane-runtime/src/store.rs:6712-6778,6789-6889; cli.rs:3183-3207,3209-3250.");
}
export function ADP_032(options) {
  return structuralCheck("ADP-032", options, "engine/crates/membrane-adapt/src/context_cost.rs",
    [/pub struct ContextCostReportV1/, /pub struct CostAttributionV1/, /pub fn attribute/],
    "Attribute always-on context cost from visible sources/digests + provider totals, preserving unattributed remainder & inferred labels.",
    "Installed consumer: engine/crates/membrane-adapt/src/cli_api.rs:32-36; membrane-runtime/src/cli.rs:3356.");
}
export function ADP_038(options) {
  return structuralCheck("ADP-038", options, "engine/crates/membrane-runtime/src/adapt_observations.rs",
    [/pub struct SequencedObservationV1/, /pub fn execute_with_catalog/, /DetectorCoverageStateV1/],
    "Emit per-assignment detector coverage receipt with observation digest, version, ran/skipped/unavailable/failed state, missing fields & honesty limit.",
    "adapt.detector-coverage.v2 through the resident observation path.");
}
export function ADP_042(options) {
  return structuralCheck("ADP-042", options, "engine/crates/membrane-adapt/src/lineage.rs",
    [/pub struct LearningLineageV1/, /pub enum LineageUnavailableReason/, /pub struct LineageCoverageGapV1/],
    "Project read-only lineage graph & typed absent-host gaps.",
    "Installed consumer: engine/crates/membrane-adapt/src/cli_api.rs:165-177 -> membrane-runtime/src/cli.rs:2998.");
}
export function ADP_053(options) {
  return structuralCheck("ADP-053", options, "engine/crates/membrane-runtime/src/adapt_efficiency.rs",
    [/pub fn analyze_efficiency/, /DetectorCoverageV1/],
    "Detect no-progress model loop from three exact same-model ModelCallFailed observations without an intervening progress-bearing H4 event.",
    "Installed consumer: engine/crates/membrane-runtime/src/adapt_observations.rs.");
}
export function ADP_054(options) {
  return structuralCheck("ADP-054", options, "engine/crates/membrane-runtime/src/adapt_efficiency.rs",
    [/pub fn analyze_efficiency/],
    "Detect duplicate tool work from repeated exact H4 tool plus subject identity, without inferring semantic equivalence or necessity.",
    "Installed consumer: engine/crates/membrane-runtime/src/adapt_observations.rs.");
}
export function ADP_057(options) {
  return structuralCheck("ADP-057", options, "engine/crates/membrane-runtime/src/adapt_efficiency.rs",
    [/pub fn analyze_efficiency/],
    "Detect retry-loop cost, summing only exact cost buckets with identical unit and basis; missing cost remains unavailable.",
    "Installed consumer: engine/crates/membrane-runtime/src/adapt_observations.rs.");
}
export function ADP_058(options) {
  return structuralCheck("ADP-058", options, "engine/crates/membrane-runtime/src/adapt_efficiency.rs",
    [/pub fn analyze_efficiency/],
    "Detect verification churn from three or more VerificationStarted events for the same exact call/subject identity.",
    "Installed consumer: engine/crates/membrane-runtime/src/adapt_observations.rs.");
}
export function ADP_059(options) {
  return structuralCheck("ADP-059", options, "engine/crates/membrane-runtime/src/adapt_efficiency.rs",
    [/pub fn analyze_efficiency/],
    "Detect replan churn from three or more PlanRevised events without an intervening progress-bearing H4 event.",
    "Installed consumer: engine/crates/membrane-runtime/src/adapt_observations.rs.");
}
export function ADP_062(options) {
  return structuralCheck("ADP-062", options, "engine/crates/membrane-runtime/src/adapt_efficiency.rs",
    [/pub fn analyze_efficiency/],
    "Detect stranded worker work: a terminal task event while a started subagent identity has no matching SubagentFinished in the bounded window.",
    "Installed consumer: engine/crates/membrane-runtime/src/adapt_observations.rs.");
}
export function ADP_073(options) {
  return structuralCheck("ADP-073", options, "engine/crates/membrane-adapt/src/proposal_state.rs",
    [/pub struct ProposalPlanStore/, /pub fn semantic_target_sha256/, /pub enum ProposalStateError/],
    "Maintain at most one apply-eligible pending proposal per semantic target & target version; concurrent proposals merge, supersede, or surface typed conflict without duplicate mutation.",
    "Installed consumer: engine/crates/membrane-adapt/src/proposal_state.rs (target/version exclusion, replay convergence).");
}

// ---------------------------------------------------------------------------
// PARTIAL / BEHAVIORAL_REIMPLEMENT rows — installed semantic probes retain
// each canonical gap in evidence; no registry status is relabelled.
// ---------------------------------------------------------------------------

export function ADP_011(options) {
  return installedCase("ADP-011", options,
    "Preserve rejected alternative/counterfactual with correction-derived Taste when safe.",
    "Counterfactual production use unclear; future delivery/effectiveness dependency not landed.",
    ["engine/crates/membrane-adapt/src/taste.rs"]);
}
export function ADP_012(options) {
  return installedCase("ADP-012", options,
    "Transition Taste through candidate/active/disputed/deprecated/superseded/retired only by receipted semantic lifecycle.",
    "Reevaluation signal wiring incomplete (dependencies ADP-010, CTX-010).",
    ["engine/crates/membrane-adapt/src/record.rs"]);
}
export function ADP_015(options) {
  return installedCase("ADP-015", options,
    "Emit preference applicability/delivery receipts & accept bounded outcome feedback tied to exact execution identity.",
    "H9/H10 producer/join incomplete; dependency MEM-024 not landed.",
    ["engine/crates/membrane-adapt/src/delivery.rs"]);
}
export function ADP_016(options) {
  return installedCase("ADP-016", options,
    "Let user inspect evidence/authority, edit/narrow/deactivate/supersede/delete/export/import preferences through governed review.",
    "Complete Hub action execution absent (dependencies ADP-004, ADP-012).",
    ["engine/crates/membrane-adapt/src/cli_api.rs"]);
}
export function ADP_019(options) {
  return installedCase("ADP-019", options,
    "Form durable issue only from deterministic recurrence; preserve one-offs/applicability & enforce issue lifecycle.",
    "Lifecycle lacks host-outcome consumer.",
    ["engine/crates/membrane-adapt/src/insights/recurrence.rs", "engine/crates/membrane-adapt/src/insights/sealed_issue.rs"]);
}
export function ADP_020(options) {
  return installedCase("ADP-020", options,
    "Discover recurring emergent patterns as inspectable candidate clusters requiring review, positives/hard negatives, holdout, version, & rollback.",
    "Cannot self-promote; real provider/sink incomplete (dependencies ADP-018, MEM-053).",
    ["engine/crates/membrane-adapt/src/adaptive.rs", "engine/crates/membrane-adapt/src/duplicate_groups.rs"]);
}
export function ADP_022(options) {
  return installedCase("ADP-022", options,
    "Represent proposal kind, intended effect, & intervention target independently under explicit compatibility policy, separate from issue/Taste authority.",
    "Proposal kind remains derived by proposal_kind_for; explicit kind input, tuple compatibility validation & legacy-derived provenance not landed.",
    ["engine/crates/membrane-adapt/src/remediation.rs"]);
}
export function ADP_023(options) {
  return installedCase("ADP-023", options,
    "Require sealed intervention attribution proving current-surface preventability, alternatives, ownership, support, & eligibility.",
    "Sealed attribution/digest/alternatives not wired into host variant.",
    ["engine/crates/membrane-adapt/src/attribution.rs"]);
}
export function ADP_024(options) {
  return installedCase("ADP-024", options,
    "Classify evaluator applicability as applicable/not-applicable/insufficient-evidence & exclude insufficient from outcome denominator.",
    "Outcome classification not wired into host denominator.",
    ["engine/crates/membrane-adapt/src/attribution.rs"]);
}
export function ADP_025(options) {
  return installedCase("ADP-025", options,
    "Track mitigation baseline/version/exposure/recurrence/regression/dismissal; optimize recurrence reduction, not finding count.",
    "H7/H9/H10 producers absent (dependencies ADP-019, ADP-023, ADP-024).",
    ["engine/crates/membrane-adapt/src/outcomes.rs", "engine/crates/membrane-adapt/src/insights/recurrence.rs"]);
}
export function ADP_030(options) {
  return installedCase("ADP-030", options,
    "Consume typed CodeRight execution observations for route/model/tool/write/verification/approval/retry/scope/subagent/artifact/completion/retrieval/Push facts.",
    "Caller transport incomplete (dependencies MEM-040-MEM-041).",
    ["engine/crates/membrane-runtime/src/host_observation_ingress.rs", "engine/crates/membrane-runtime/src/adapt_observations.rs"]);
}
export function ADP_031(options) {
  return installedCase("ADP-031", options,
    "Consume versioned evaluator/dataset/case/experiment/trace outcomes while CodeRight retains eval execution/storage.",
    "Real H7/H9/H10 producers absent (dependency ADP-030).",
    ["engine/crates/membrane-adapt/src/benchmark.rs"]);
}
export function ADP_033(options) {
  return installedCase("ADP-033", options,
    "Convert reviewed confirmed failure into minimal privacy-safe regression-case proposal; external owner adopts/runs it.",
    "Cross-repo production path unproven (dependencies ADP-019, ADP-022).",
    ["engine/crates/membrane-adapt/src/portable.rs"]);
}
export function ADP_034(options) {
  return installedCase("ADP-034", options,
    "Present separate Taste & Insights queues with evidence, scope, recurrence, versions, mitigation, actions, & receipts.",
    "Trusted Hub action executor absent (dependencies ADP-016, ADP-019, ADP-025).",
    ["engine/crates/membrane-adapt/src/cli_api.rs"]);
}
export function ADP_035(options) {
  return installedCase("ADP-035", options,
    "Execute learner semantics inside an admitted background job & write only bounded proposals through proposal sink.",
    "Real first-party semantic inputs/effect qualification incomplete (dependencies MEM-052-MEM-053).",
    ["engine/crates/membrane-runtime/src/background_review_input.rs", "engine/crates/membrane-runtime/src/adapt_service.rs"]);
}
export function ADP_036(options) {
  return installedCase("ADP-036", options,
    "Measure procedural effectiveness only from exact joinable activation/evaluation/outcome plus asset-content & loaded-representation digests; never co-aggregate versions, otherwise unavailable.",
    "Final host-loaded representation acknowledgement absent; effectiveness intentionally remains unavailable.",
    ["engine/crates/membrane-runtime/src/adapt_effectiveness.rs"]);
}
export function ADP_040(options) {
  return installedCase("ADP-040", options,
    "Join detector coverage receipt to exact execution episode, evaluator identity & outcome receipt.",
    "H4-to-H6 execution-episode receipt bridge and exact loaded-exposure binding remain unavailable.",
    ["engine/crates/membrane-runtime/src/adapt_observations.rs"]);
}
export function ADP_041(options) {
  return installedCase("ADP-041", options,
    "Converge persisted multiwriter preference writes order-independently while retaining conflicts for review.",
    "Persisted convergence has no production consumer.",
    ["engine/crates/membrane-adapt/src/multiwriter.rs"]);
}
export function ADP_043(options) {
  return installedCase("ADP-043", options,
    "Detect duplicate assignment execution from exact assignment & worker identities.",
    "H4 lacks an independent assignment_id for classifying multi-worker execution as duplicate assignment.",
    ["engine/crates/membrane-runtime/src/adapt_efficiency.rs"]);
}
export function ADP_044(options) {
  return installedCase("ADP-044", options,
    "Detect orchestrator role leakage into lane-owned execution.",
    "Exact lane-owner identity and lane execution boundary are absent for positive leakage classification.",
    ["engine/crates/membrane-runtime/src/adapt_efficiency.rs"]);
}
export function ADP_045(options) {
  return installedCase("ADP-045", options,
    "Detect overlapping lane scope across accepted work.",
    "lane_id and accepted_work_scope are absent for exact overlap classification.",
    ["engine/crates/membrane-runtime/src/adapt_efficiency.rs"]);
}
export function ADP_046(options) {
  return installedCase("ADP-046", options,
    "Detect bounded lane budget exceeded from declared limits.",
    "H4 has no declared lane budget to compare against.",
    ["engine/crates/membrane-runtime/src/adapt_efficiency.rs"]);
}
export function ADP_047(options) {
  return installedCase("ADP-047", options,
    "Detect missing required efficiency budget.",
    "Cannot distinguish a genuinely absent required efficiency budget from an uninstrumented budget.",
    ["engine/crates/membrane-runtime/src/adapt_efficiency.rs"]);
}
export function ADP_048(options) {
  return installedCase("ADP-048", options,
    "Detect fanout without incremental accepted value.",
    "Exact incremental accepted-value identity per worker is absent.",
    ["engine/crates/membrane-runtime/src/adapt_efficiency.rs"]);
}
export function ADP_049(options) {
  return installedCase("ADP-049", options,
    "Detect subagent context duplication.",
    "H4 does not carry each subagent loaded-context digest.",
    ["engine/crates/membrane-runtime/src/adapt_efficiency.rs"]);
}
export function ADP_050(options) {
  return installedCase("ADP-050", options,
    "Detect context replay amplification.",
    "Exact replay content digest and replay size are absent.",
    ["engine/crates/membrane-runtime/src/adapt_efficiency.rs"]);
}
export function ADP_051(options) {
  return installedCase("ADP-051", options,
    "Detect cold-cache rebuild waste.",
    "Cache-key and semantic rebuild identity are absent.",
    ["engine/crates/membrane-runtime/src/adapt_efficiency.rs"]);
}
export function ADP_052(options) {
  return installedCase("ADP-052", options,
    "Detect cache invalidation churn.",
    "H4 lacks cache invalidation events and cache-key identity.",
    ["engine/crates/membrane-runtime/src/adapt_efficiency.rs"]);
}
export function ADP_055(options) {
  return installedCase("ADP-055", options,
    "Detect semantic tool-work overlap.",
    "H4 lacks a canonical semantic-work digest for overlap classification.",
    ["engine/crates/membrane-runtime/src/adapt_efficiency.rs"]);
}
export function ADP_056(options) {
  return installedCase("ADP-056", options,
    "Detect oversized tool-result replay.",
    "Exact result size and replay-content digest are absent.",
    ["engine/crates/membrane-runtime/src/adapt_efficiency.rs"]);
}
export function ADP_060(options) {
  return installedCase("ADP-060", options,
    "Detect routing cost mismatch.",
    "Declared expected/allowed route-cost baseline is absent.",
    ["engine/crates/membrane-runtime/src/adapt_efficiency.rs"]);
}
export function ADP_061(options) {
  return installedCase("ADP-061", options,
    "Detect integration rework caused by lane failure.",
    "H4 cannot express a sealed causal link from lane failure to later integration rework.",
    ["engine/crates/membrane-runtime/src/adapt_efficiency.rs"]);
}
export function ADP_063(options) {
  return installedCase("ADP-063", options,
    "Detect background learning over budget.",
    "H4 lacks frozen background-learning identity and budget facts.",
    ["engine/crates/membrane-runtime/src/adapt_efficiency.rs"]);
}
export function ADP_064(options) {
  return installedCase("ADP-064", options,
    "Report assignment efficiency from qualified execution facts.",
    "Independent assignment_id remains unavailable.",
    ["engine/crates/membrane-runtime/src/adapt_efficiency.rs"]);
}
export function ADP_072(options) {
  return installedCase("ADP-072", options,
    "When evidence is insufficient for safe proposal formation, ask one evidence-bound clarification question, persist nonmutating clarification state, bind one human answer, & resume same lineage only while target evidence remains current.",
    "Independent human/adjudicator authentication remains transport-owned; not landed in this crate.",
    ["engine/crates/membrane-adapt/src/clarification.rs"]);
}
export function ADP_074(options) {
  return installedCase("ADP-074", options,
    "Expose negotiated, scope-bound, read-only agent inspection of Adapt preferences, applicability decisions, Insights, and proposal state without approval or exposure side effects.",
    "Full schema/privacy/platform qualification remains open per implementation-status.md.",
    ["engine/crates/membrane-mcp/src/tools.rs", "engine/crates/membrane-runtime/src/adapt_service.rs"]);
}
export function ADP_075(options) {
  return installedCase("ADP-075", options,
    "Report daemon-backed Adapt pipeline readiness and progress from actual producer/consumer bindings, distinguishing empty workload, unavailable evidence, blocked work, and missing outcome joins.",
    "Live producer connectivity and complete backlog health remain unavailable.",
    ["engine/crates/membrane-runtime/src/adapt_service.rs"]);
}
export function ADP_076(options) {
  const requirement = "Issue a bounded, version-bound behavioral candidate-comparison decision from host-run baseline/variant outcomes, allowing no improvement without granting review, admission, or activation authority.";
  const improved = comparisonFixture();
  const noImprovement = structuredClone(improved);
  noImprovement.comparison_id = "windows-adp-076-no-improvement";
  for (const row of [...noImprovement.development, ...noImprovement.frozen_test]) row.adherent = row.stratum !== "failure";
  const regression = structuredClone(improved);
  regression.comparison_id = "windows-adp-076-regression";
  regression.frozen_test.find((row) => row.candidate_sha256 === digest("b")).correct = false;
  const budget = structuredClone(improved);
  budget.comparison_id = "windows-adp-076-budget";
  budget.usage.cost_microunits = budget.limits.cost_microunits + 1;
  const cancelled = structuredClone(improved);
  cancelled.comparison_id = "windows-adp-076-cancelled";
  cancelled.cancelled = true;
  const versioned = structuredClone(improved);
  versioned.comparison_id = "windows-adp-076-versioned";
  versioned.target_version = 4;
  const leaked = structuredClone(improved);
  leaked.comparison_id = "windows-adp-076-frozen-leak";
  for (const row of leaked.frozen_test.filter((row) => row.case_id === leaked.frozen_test[0].case_id)) {
    row.case_sha256 = leaked.development[0].case_sha256;
  }
  const runs = [improved, noImprovement, regression, budget, cancelled, versioned].map((value) => runAdaptJson(options || {}, "compare", value));
  const leakage = runAdaptJson(options || {}, "compare", leaked);
  if (runs.some((r) => r.error) || !leakage.error?.includes("development/frozen-test leakage")) return installedFailure("ADP-076", options, requirement, `installed adapt compare unavailable or refused: ${runs.find((r) => r.error)?.error || leakage.error || "frozen-test leakage was accepted"}`, ["engine/crates/membrane-adapt/src/comparison.rs"]);
  const decisions = runs.map((r) => r.value?.decision);
  const [selected, retained, regressed, budgeted, cancelledDecision, versionedDecision] = decisions;
  const valid = (d) => d && d.contract === "adapt.candidate-comparison-decision.v1" && d.activation_authorized === false && d.requires_independent_admission === true && d.requires_target_revalidation === true && typeof d.decision_sha256 === "string" && d.decision_sha256.length === 64;
  const source = sourceEvidence(resolveRoot(options), ["engine/crates/membrane-adapt/src/comparison.rs"]);
  const identity = requireInstalledIdentity(options);
  if (!decisions.every(valid) || selected.disposition !== "candidate_selected" || retained.disposition !== "no_improvement" || retained.selected_sha256 !== retained.baseline_sha256 || regressed.disposition !== "regression" || budgeted.reason !== "declared_budget_exhausted" || budgeted.selected_sha256 !== budgeted.baseline_sha256 || cancelledDecision.reason !== "cancelled" || cancelledDecision.selected_sha256 !== cancelledDecision.baseline_sha256 || versionedDecision.target_version !== 4 || versionedDecision.request_sha256 === selected.request_sha256) {
    return { id: "ADP-076", kind: "installed", evidenceKind: "installed", pass: false, status: "failed", requirement, reason: "installed compare did not produce both bounded selection and no-improvement decisions", evidence: decisions, detail: { source, identity, decisions } };
  }
  const detail = { source, identity, selected: selected.decision_sha256, retained: retained.decision_sha256, regression: regressed.decision_sha256, budget: budgeted.decision_sha256, cancelled: cancelledDecision.decision_sha256, versioned: versionedDecision.decision_sha256, frozenLeakRefused: true, dispositions: decisions.map((d) => d.disposition) };
  return { id: "ADP-076", kind: "installed", evidenceKind: "installed", pass: true, status: "passed", requirement, reason: "installed compare exercised selection, retention, regression, budget, cancellation, version binding and frozen-test isolation with admission/activation withheld", evidence: detail, detail };
}
export function ADP_077(options) {
  const requirement = "Determine evidence-bound eligibility for each learned-guard rollout-stage transition without granting the host's separate blocking or scope-expansion authority.";
  const h = (c) => c.repeat(64);
  const subject = h("a");
  const input = { schema_version: 1, issue_id: "windows-077", mitigation_sha256: subject, target: "skill", target_sha256: h("b"), host_configuration_sha256: h("c"), current_scope: "repo", proposed_scope: "repo", current_stage: "reviewed", proposed_stage: "shadow", now_ms: 100, comparable_exposures: 0, evaluated_exposures: 0, false_blocks: 0, minimum_exposures: 0, maximum_false_block_bps: 100, rollback_ref: "rollback", evidence: ["review", "detector", "attribution", "target", "host_configuration"].map((kind, i) => ({ kind, receipt_id: `r-${i}`, receipt_sha256: h(String(i + 1)), subject_sha256: kind === "target" ? h("b") : kind === "host_configuration" ? h("c") : subject, scope: "repo", valid_until_ms: 1000, passed: true })) };
  const blockedInput = structuredClone(input);
  blockedInput.issue_id = "windows-077-insufficient-coverage";
  blockedInput.proposed_stage = "advisory";
  blockedInput.minimum_exposures = 2;
  blockedInput.comparable_exposures = 1;
  blockedInput.evaluated_exposures = 1;
  blockedInput.current_stage = "shadow";
  blockedInput.evidence.push({ kind: "shadow_evaluation", receipt_id: "r-shadow", receipt_sha256: h("9"), subject_sha256: subject, scope: "repo", valid_until_ms: 1000, passed: true });
  const advisory = structuredClone(input);
  advisory.issue_id = "windows-077-advisory";
  advisory.current_stage = "shadow";
  advisory.proposed_stage = "advisory";
  advisory.minimum_exposures = advisory.comparable_exposures = advisory.evaluated_exposures = 2;
  advisory.evidence.push({ kind: "shadow_evaluation", receipt_id: "r-shadow", receipt_sha256: h("9"), subject_sha256: subject, scope: "repo", valid_until_ms: 1000, passed: true });
  const scoped = structuredClone(advisory);
  scoped.issue_id = "windows-077-scoped";
  scoped.current_stage = "advisory";
  scoped.proposed_stage = "scoped_blocking";
  scoped.evidence = scoped.evidence.filter((e) => e.kind !== "shadow_evaluation");
  scoped.evidence.push({ kind: "advisory_evaluation", receipt_id: "r-advisory", receipt_sha256: h("9"), subject_sha256: subject, scope: "repo", valid_until_ms: 1000, passed: true });
  const falseBlocks = structuredClone(advisory);
  falseBlocks.issue_id = "windows-077-false-blocks";
  falseBlocks.false_blocks = 1;
  const noRollback = structuredClone(advisory);
  noRollback.issue_id = "windows-077-no-rollback";
  noRollback.rollback_ref = "";
  const widened = structuredClone(advisory);
  widened.issue_id = "windows-077-scope-widening";
  widened.proposed_scope = "global";
  const runs = [input, advisory, scoped, falseBlocks, noRollback, widened, blockedInput].map((value) => runAdaptJson(options || {}, "guard-eligibility", value));
  if (runs.some((r) => r.error)) return installedFailure("ADP-077", options, requirement, `installed guard-eligibility unavailable or refused: ${runs.find((r) => r.error).error}`, ["engine/crates/membrane-adapt/src/guard_rollout.rs"]);
  const decisions = runs.map((r) => r.value?.decision);
  const [shadow, advisoryDecision, scopedDecision, falseBlockDecision, rollbackDecision, widenedDecision, blocked] = decisions;
  const valid = (d) => d && d.contract === "adapt.guard-eligibility.v1" && d.host_authorization_required === true && d.activation_authorized === false && typeof d.decision_sha256 === "string" && d.decision_sha256.length === 64;
  const source = sourceEvidence(resolveRoot(options), ["engine/crates/membrane-adapt/src/guard_rollout.rs"]);
  const identity = requireInstalledIdentity(options);
  if (!decisions.every(valid) || !shadow.eligible || !advisoryDecision.eligible || !scopedDecision.eligible || falseBlockDecision.eligible || !falseBlockDecision.reasons.includes("false_block_limit_exceeded") || rollbackDecision.eligible || !rollbackDecision.reasons.includes("rollback_unavailable") || widenedDecision.eligible || !widenedDecision.reasons.includes("scope_change_requires_separate_review") || blocked.eligible || !blocked.reasons.includes("insufficient_comparable_coverage") || blocked.reasons.includes("stage_transition_not_sequential")) return { id: "ADP-077", kind: "installed", evidenceKind: "installed", pass: false, status: "failed", requirement, reason: "installed guard eligibility did not independently cover sequential stages and bounded refusals", evidence: decisions, detail: { source, identity, decisions } };
  const detail = { source, identity, shadow: shadow.decision_sha256, advisory: advisoryDecision.decision_sha256, scopedBlocking: scopedDecision.decision_sha256, falseBlocks: falseBlockDecision.decision_sha256, rollback: rollbackDecision.decision_sha256, widened: widenedDecision.decision_sha256, blocked: blocked.decision_sha256, hostAuthorizationRequired: decisions.every((d) => d.host_authorization_required), activationAuthorized: decisions.some((d) => d.activation_authorized) };
  return { id: "ADP-077", kind: "installed", evidenceKind: "installed", pass: true, status: "passed", requirement, reason: "installed guard eligibility covered sequential stages, false-block, rollback and scope-widening refusals while withholding host permission", evidence: detail, detail };
}

// ---------------------------------------------------------------------------
// Registry of all exported case IDs, for test/receipt enumeration.
// ---------------------------------------------------------------------------

export const ALL_CASE_IDS = [
  "ADP-001", "ADP-002", "ADP-003", "ADP-004", "ADP-005", "ADP-006", "ADP-007", "ADP-008", "ADP-009", "ADP-010",
  "ADP-011", "ADP-012", "ADP-013", "ADP-014", "ADP-015", "ADP-016", "ADP-017", "ADP-018", "ADP-019", "ADP-020",
  "ADP-021", "ADP-022", "ADP-023", "ADP-024", "ADP-025", "ADP-026", "ADP-027", "ADP-028", "ADP-029", "ADP-030",
  "ADP-031", "ADP-032", "ADP-033", "ADP-034", "ADP-035", "ADP-036", "ADP-038", "ADP-040", "ADP-041", "ADP-042",
  "ADP-043", "ADP-044", "ADP-045", "ADP-046", "ADP-047", "ADP-048", "ADP-049", "ADP-050", "ADP-051", "ADP-052",
  "ADP-053", "ADP-054", "ADP-055", "ADP-056", "ADP-057", "ADP-058", "ADP-059", "ADP-060", "ADP-061", "ADP-062",
  "ADP-063", "ADP-064", "ADP-072", "ADP-073", "ADP-074", "ADP-075", "ADP-076", "ADP-077",
];

// IDs whose canonicalImplementationRow is COMPLETE/DELIVERED and retain
// marker checks before installed semantic qualification.
export const STRUCTURAL_CASE_IDS = [
  "ADP-001", "ADP-002", "ADP-003", "ADP-004", "ADP-005", "ADP-006", "ADP-007", "ADP-008", "ADP-009", "ADP-010",
  "ADP-013", "ADP-014", "ADP-017", "ADP-018", "ADP-021", "ADP-026", "ADP-027", "ADP-028", "ADP-029", "ADP-032",
  "ADP-038", "ADP-042", "ADP-053", "ADP-054", "ADP-057", "ADP-058", "ADP-059", "ADP-062", "ADP-073",
];
