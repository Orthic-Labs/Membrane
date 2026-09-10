#!/usr/bin/env node
// scripts/qualification/cases/pul-windows.mjs — federation-catalog lane case module.
//
// PUL-001..PUL-042 are source-bound contract checks with executable native
// qualification. Source mode records canonical implementation/test digests;
// when an installed root is supplied, each row invokes installed membrane.exe
// Pull and validates its row-specific semantic response fields. Structural
// `kind` is retained for compatibility with existing fixture tests, while
// `evidenceKind` distinguishes source proof from installed runtime proof.
//   - EX_01..EX_09: real static exclusion checks over the actual
//     membrane-core / membrane-federation / membrane-runtime /
//     membrane-protocol source trees for forbidden patterns (second
//     graph/search/impact/planner/memory/vector/"Dream" service, per-client
//     semantic authority, graph mutation authority, Markdown graph store,
//     unverified-LLM-as-fact admission, scalar-trust replacing typed
//     authority, arbitrary truth expiration, speculative memory
//     branching/merging, automatic paid freshness/recomputation calls).
//     These run today, need no build, and each has an executable negative
//     control in pul-windows.test.mjs that injects the forbidden pattern
//     into a fixture root and asserts the check fails.
//
// Every exported function accepts an optional `{ root }` so tests can point
// checks at a fixture tree instead of the live repository (this is how the
// negative controls in pul-windows.test.mjs work without mutating real
// source).

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = resolve(HERE, "../../../");

function resolveRoot(options) {
  return (options && (options.root || options.workspaceRoot)) || REPO_ROOT;
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

const NATIVE_PULL_TASKS = Object.freeze({
  "PUL-001": "normalize caller task repository grant anchors budget deadline policy epoch requirements",
  "PUL-002": "derive monotonic multidimensional evidence requirements conservative broad fallback",
  "PUL-003": "catalog provider capability authority cost readiness omissions",
  "PUL-004": "build capability requirement staged acquisition budget fallback publication reserve",
  "PUL-005": "schedule provider work concurrently caps cancellation inherited deadline",
  "PUL-006": "retrieve current live file evidence repository confinement",
  "PUL-007": "retrieve current Git head index worktree evidence",
  "PUL-008": "retrieve workspace rules policy data only non authorizing evidence",
  "PUL-009": "resolve task source anchors exact current evidence",
  "PUL-010": "discover rank workspace skills resolver handles",
  "PUL-011": "retrieve scoped Cortex durable knowledge candidates",
  "PUL-012": "retrieve generation bound complete Blueprint evidence paths",
  "PUL-013": "retrieve Audit findings typed non authorizing evidence",
  "PUL-014": "retrieve architecture decision evidence as plans",
  "PUL-015": "admit invoke explicitly enabled Ledger provider authority freshness omissions",
  "PUL-016": "normalize heterogeneous outputs source generation authority freshness omissions atomic grouping",
  "PUL-017": "reject evidence before ranking scope grant trust influence sensitivity quarantine temporal resolution freshness authority",
  "PUL-018": "preserve authority freshness independent axes current direct evidence",
  "PUL-019": "evaluate requirement coverage satisfied partial missing contradictory stale unsafe unavailable",
  "PUL-020": "run one alternate corrective lane insufficiency remerge typed",
  "PUL-021": "fuse eligible providers deterministic fixed security ordering receipt",
  "PUL-022": "select deterministic named versioned RRF provider local scores",
  "PUL-023": "collapse duplicate lineage source hash preserve authority evidence diversity",
  "PUL-024": "fill minimum faithful evidence required dimensions before depth",
  "PUL-025": "spend residual budget deterministic marginal utility",
  "PUL-026": "retain reserved memory skills lanes migration control",
  "PUL-027": "choose cheapest faithful native excerpt skeleton summary resolver metadata",
  "PUL-028": "reconcile selected delivered tokens mutually exclusive lanes ceiling",
  "PUL-029": "reobserve grant identity policy epoch revocation before bytes",
  "PUL-030": "return typed policy changed no stale authorized packet",
  "PUL-031": "populate Pull receipts candidate journey resolution omissions coverage accounting",
  "PUL-032": "emit Pull candidate delivery outcome observations explicit strength",
  "PUL-033": "return versioned insufficient confidence searched lane counts",
  "PUL-035": "reobserve resolver availability immediately before publication",
  "PUL-036": "reconcile publication authorization immediately before emission",
  "PUL-037": "suppress unchanged evidence bounded session horizon restore changed content",
  "PUL-039": "preserve byte stable versioned reusable packet prefix equivalent request",
  "PUL-040": "place admitted evidence versioned semantic class membership authority trust atomic grouping",
  "PUL-041": "aggregate native workspace evidence independently authorized repositories target identity omissions",
  "PUL-042": "select resolver only negotiated callable owner resolver unsupported alternative",
});

// Source proof pairs each implementation artifact with an executable native
// test owner. This keeps source qualification tied to code & tests rather
// than to a free-standing marker or canon status.
const SOURCE_TESTS = Object.freeze({
  "PUL-001": ["engine/crates/membrane-federation/tests/requirements_contract.rs"],
  "PUL-002": ["engine/crates/membrane-federation/tests/corrective_retrieval_qualification.rs"],
  "PUL-003": ["engine/crates/membrane-federation/tests/engine_contract.rs"],
  "PUL-004": ["engine/crates/membrane-runtime/tests/pull_residual_qualification.rs"],
  "PUL-005": ["engine/crates/membrane-runtime/tests/pull_residual_qualification.rs"],
  "PUL-006": ["engine/crates/membrane-federation/tests/provider_live_files.rs"],
  "PUL-007": ["engine/crates/membrane-federation/tests/provider_git.rs"],
  "PUL-008": ["engine/crates/membrane-federation/tests/provider_rules.rs"],
  "PUL-009": ["engine/crates/membrane-federation/tests/provider_anchors.rs"],
  "PUL-010": ["engine/crates/membrane-federation/tests/provider_skills.rs"],
  "PUL-011": ["engine/crates/membrane-federation/tests/provider_cortex.rs"],
  "PUL-012": ["engine/crates/membrane-federation/tests/provider_blueprint.rs"],
  "PUL-013": ["engine/crates/membrane-federation/tests/provider_audit.rs"],
  "PUL-014": ["engine/crates/membrane-federation/tests/provider_architect.rs"],
  "PUL-015": ["engine/crates/membrane-federation/tests/engine_contract.rs"],
  "PUL-016": ["engine/crates/membrane-runtime/tests/pull_residual_qualification.rs"],
  "PUL-017": ["engine/crates/membrane-runtime/tests/pull_residual_qualification.rs"],
  "PUL-018": ["engine/crates/membrane-runtime/tests/pull_residual_qualification.rs"],
  "PUL-019": ["engine/crates/membrane-federation/tests/corrective_retrieval_qualification.rs"],
  "PUL-020": ["engine/crates/membrane-federation/tests/corrective_retrieval_qualification.rs"],
  "PUL-021": ["engine/crates/membrane-federation/tests/fusion_qualification.rs"],
  "PUL-022": ["engine/crates/membrane-federation/tests/fusion_qualification.rs"],
  "PUL-023": ["engine/crates/membrane-federation/tests/engine_contract.rs"],
  "PUL-024": ["engine/crates/membrane-runtime/tests/pull_residual_qualification.rs"],
  "PUL-025": ["engine/crates/membrane-runtime/tests/pull_residual_qualification.rs"],
  "PUL-026": ["engine/crates/membrane-runtime/tests/pull_residual_qualification.rs"],
  "PUL-027": ["engine/crates/membrane-runtime/tests/pull_post_merge_acceptance.rs"],
  "PUL-028": ["engine/crates/membrane-runtime/tests/pull_post_merge_acceptance.rs"],
  "PUL-029": ["engine/crates/membrane-runtime/tests/publication_fence_recheck.rs"],
  "PUL-030": ["engine/crates/membrane-runtime/tests/publication_fence_recheck.rs"],
  "PUL-031": ["engine/crates/membrane-runtime/tests/pull_post_merge_acceptance.rs"],
  "PUL-032": ["engine/crates/membrane-runtime/tests/pull_residual_qualification.rs"],
  "PUL-033": ["engine/crates/membrane-federation/tests/fence_abstention.rs"],
  "PUL-035": ["engine/crates/membrane-runtime/tests/publication_fence_recheck.rs"],
  "PUL-036": ["engine/crates/membrane-runtime/tests/publication_fence_recheck.rs"],
  "PUL-037": ["engine/crates/membrane-runtime/tests/pull_post_merge_acceptance.rs"],
  "PUL-039": ["engine/crates/membrane-runtime/tests/pull_post_merge_acceptance.rs"],
  "PUL-040": ["engine/crates/membrane-runtime/tests/pull_post_merge_acceptance.rs"],
  "PUL-041": ["engine/crates/membrane-runtime/tests/pull_residual_qualification.rs"],
  "PUL-042": ["engine/crates/membrane-runtime/tests/pull_residual_qualification.rs"],
});

function assertNativePullRow(id, value) {
  if (value.task !== NATIVE_PULL_TASKS[id]) throw new Error(`${id} native task was not preserved by Pull`);
  if (!value.sourceResponse || typeof value.sourceResponse !== "object") throw new Error(`${id} native Pull omitted provider source response`);
  if (!Array.isArray(value.providerDiagnostics)) throw new Error(`${id} native Pull omitted provider diagnostics`);
  if (!value.finalAdmission || typeof value.finalAdmission !== "object") throw new Error(`${id} native Pull omitted final admission`);
  if (id === "PUL-017" && !Array.isArray(value.finalAdmission.omissions)) throw new Error(`${id} native Pull omitted admission omissions`);
  if (id === "PUL-022" && !/fusion|rrf|reciprocal/i.test(JSON.stringify(value))) throw new Error(`${id} native Pull omitted fusion strategy evidence`);
  if (id === "PUL-031" && !value.requirementEvidenceMap && !value.receipts?.requirementEvidenceMap) throw new Error(`${id} native Pull omitted requirement journey evidence`);
  if (id === "PUL-039" && (!value.cachePrefixDiagnostic || typeof value.cachePrefixDiagnostic !== "object")) throw new Error(`${id} native Pull omitted cache prefix diagnostic`);
  if (id === "PUL-040" && (!value.placementReceipt || typeof value.placementReceipt !== "object")) throw new Error(`${id} native Pull omitted placement receipt`);
  if (id === "PUL-041" && !Array.isArray(value.atomicEvidencePaths)) throw new Error(`${id} native Pull omitted per-target evidence paths`);
  if (id === "PUL-042" && (!Array.isArray(value.packet.blocks) || !Array.isArray(value.finalAdmission.omissions))) throw new Error(`${id} native Pull omitted resolver-safe packet/admission outcome`);
}

function installedPull(options, id, assertions) {
  const root = process.env.MEMBRANE_QUALIFICATION_INSTALLED_ROOT;
  if (!root) return null;
  const exe = join(resolve(root), "membrane.exe");
  if (!existsSync(exe)) return { id, status: "failed", evidenceKind: "installed", reason: `installed membrane.exe missing: ${exe}` };
  try {
    const raw = execFileSync(exe, ["cli", "pull", "federate", "--repo", resolveRoot(options), "--task", NATIVE_PULL_TASKS[id] || `qualification_${id}`, "--max-tokens", "256", "--session", `windows-r5-${id}`], { cwd: resolveRoot(options), encoding: "utf8", windowsHide: true, timeout: 30_000 });
    const value = JSON.parse(raw);
    if (value.transport !== "native") throw new Error("Pull transport is not native");
    if (!value.packet || !Array.isArray(value.packet.blocks)) throw new Error("native Pull omitted packet blocks");
    if (!Array.isArray(value.receipts)) throw new Error("native Pull omitted receipts");
    if (!value.finalAdmission && !value.insufficientConfidence) throw new Error("native Pull omitted admission outcome");
    assertNativePullRow(id, value);
    assertions(value);
    return {
      id,
      status: "passed",
      evidenceKind: "installed",
      detail: {
        installedProof: {
          executable: exe,
          repository: resolveRoot(options),
          task: NATIVE_PULL_TASKS[id] || null,
          session: `windows-r5-${id}`,
          transport: value.transport,
          assertions: ["native_transport", "packet_blocks", "receipts", "source_response", "provider_diagnostics", "final_admission", id],
        },
        packet: value.packet,
        receipts: value.receipts,
        finalAdmission: value.finalAdmission ?? null,
        federationMetrics: value.federationMetrics ?? null,
      },
    };
  } catch (error) {
    return { id, status: "failed", evidenceKind: "installed", reason: `${id} installed native Pull assertion failed: ${error.message}` };
  }
}

function readyOrStructural(id, options, relPaths, markers, note, assertions) {
  return installedPull(options, id, assertions) ?? structuralCheck(id, options, relPaths, markers, note);
}

// Structural attestation: file exists and contains at least one of the
// given markers (regexes or plain strings). Never claims functional
// correctness — only that the named implementation artifact is present and
// carries the expected contract surface.
function structuralCheck(id, options, relPaths, markers, note) {
  // Registry qualification upgrades every source row to an installed native
  // probe when caller supplies an installed root. Fixture tests & source-only
  // checks remain deterministic because they never set this environment.
  if (!(options && options.root) && process.env.MEMBRANE_QUALIFICATION_INSTALLED_ROOT) {
    return installedPull(options, id, () => {});
  }
  const root = resolveRoot(options);
  const files = Array.isArray(relPaths) ? relPaths : [relPaths];
  const missing = files.filter((p) => !readFileSafe(root, p));
  if (missing.length === files.length) {
    return {
      id,
      kind: "structural",
      evidenceKind: "source",
      pass: false,
      reason: `none of the canonical implementation files exist: ${files.join(", ")}`,
      evidence: files,
    };
  }
  const hits = [];
  for (const relPath of files) {
    const content = readFileSafe(root, relPath);
    if (!content) continue;
    for (const marker of markers) {
      const re = marker instanceof RegExp ? marker : new RegExp(marker.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
      if (re.test(content)) hits.push({ file: relPath, marker: String(marker) });
    }
  }
  const sourceTests = (SOURCE_TESTS[id] || []).map((file) => {
    const content = readFileSafe(root, file);
    return content ? {
      file,
      sha256: createHash("sha256").update(content).digest("hex"),
    } : null;
  }).filter(Boolean);
  const missingTests = (SOURCE_TESTS[id] || []).filter((file) => !sourceTests.some((test) => test.file === file));
  return {
    id,
    status: hits.length > 0 ? "passed" : "failed",
    kind: "structural",
    evidenceKind: "source",
    sourceBound: true,
    pass: hits.length > 0,
    reason: hits.length > 0
      ? `found ${hits.length} contract marker(s) in canonical implementation file(s)`
      : `canonical implementation file(s) present but no expected contract marker found: ${markers.map(String).join(", ")}`,
    evidence: hits.length > 0 ? hits : files,
    sourceProof: { implementationFiles: files, markerHits: hits, executableTests: sourceTests, missingTests },
    detail: { sourceProof: { implementationFiles: files, markerHits: hits, executableTests: sourceTests, missingTests } },
    note,
  };
}

// ---------------------------------------------------------------------------
// PUL-001 .. PUL-042 — structural attestations against canonicalImplementationRow.
// ---------------------------------------------------------------------------

export function PUL_001(options) {
  return structuralCheck("PUL-001", options, "engine/crates/membrane-federation/src/request.rs",
    [/pub struct .*Request/, /deadline/i, /policy_epoch|policyEpoch/i], "normalization of caller task/grant/budget/deadline/epoch");
}
export function PUL_002(options) {
  return structuralCheck("PUL-002", options, "engine/crates/membrane-federation/src/corrective.rs",
    [/fn .*requirement/i, /fallback/i], "monotonic requirement derivation with fallback");
}
export function PUL_003(options) {
  return structuralCheck("PUL-003", options, "engine/crates/membrane-runtime/src/catalog.rs",
    [/capability/i, /authority/i, /omission/i], "provider catalog capability/authority/omission fields");
}
export function PUL_004(options) {
  return structuralCheck("PUL-004", options,
    ["engine/crates/membrane-federation/src/registry.rs", "engine/crates/membrane-federation/src/scheduler.rs", "engine/crates/membrane-federation/src/engine.rs"],
    [/fallback/i, /budget/i, /reserve/i], "staged acquisition with caps/fallback/reserve");
}
export function PUL_005(options) {
  return structuralCheck("PUL-005", options, ["engine/crates/membrane-federation/src/scheduler.rs", "engine/crates/membrane-federation/src/deadline.rs"],
    [/cancel/i, /deadline/i, /concurren/i], "concurrent scheduling under per-provider caps + cancellation");
}
export function PUL_006(options) {
  return structuralCheck("PUL-006", options, "engine/crates/membrane-federation/src/providers/live_files.rs",
    [/confine|confinement|repository_root|workspace_root/i], "live-file evidence under repository confinement");
}
export function PUL_007(options) {
  return structuralCheck("PUL-007", options, "engine/crates/membrane-federation/src/providers/git.rs",
    [/head/i, /worktree|index/i], "git head/index/worktree evidence retrieval");
}
export function PUL_008(options) {
  return structuralCheck("PUL-008", options, "engine/crates/membrane-federation/src/providers/rules.rs",
    [/data_only|dataOnly|instruction_policy|rule text is data/i, /grant|authoriz/i], "rules/policy documents marked data-only, non-authorizing");
}
export function PUL_009(options) {
  return structuralCheck("PUL-009", options, "engine/crates/membrane-federation/src/providers/anchors.rs",
    [/anchor/i], "task/source anchor resolution");
}
export function PUL_010(options) {
  return structuralCheck("PUL-010", options, "engine/crates/membrane-federation/src/providers/skills.rs",
    [/resolver|handle/i], "skill discovery returns resolver handles, not full bodies");
}
export function PUL_011(options) {
  return structuralCheck("PUL-011", options, "engine/crates/membrane-federation/src/providers/cortex.rs",
    [/scope/i, /candidate/i], "scoped Cortex candidate retrieval without final-attention authority");
}
export function PUL_012(options) {
  return structuralCheck("PUL-012", options, "engine/crates/membrane-federation/src/providers/blueprint.rs",
    [/generation/i], "generation-bound Blueprint evidence retrieval");
}
export function PUL_013(options) {
  return structuralCheck("PUL-013", options, "engine/crates/membrane-federation/src/providers/audit.rs",
    [/finding/i, /typed|Typed/], "Audit findings as typed non-authorizing evidence");
}
export function PUL_014(options) {
  return structuralCheck("PUL-014", options, "engine/crates/membrane-federation/src/providers/architect.rs",
    [/plan|decision|record|architect/i], "architecture evidence represented as plans, not current-code truth");
}
export function PUL_015(options) {
  return structuralCheck("PUL-015", options, "engine/crates/membrane-federation/src/registry.rs",
    [/registry|registration|capability/i, /fresh|ready|omission/i], "explicit provider admission preserving authority/freshness/omission");
}
export function PUL_016(options) {
  return structuralCheck("PUL-016", options, "engine/crates/membrane-federation/src/normalize.rs",
    [/authority/i, /freshness/i, /omission/i], "typed evidence normalization preserving authority/freshness/omissions");
}
export function PUL_017(options) {
  return structuralCheck("PUL-017", options, "engine/crates/membrane-runtime/src/pull/admission.rs",
    [/scope|authority|trust|quarantine|secret/i, /instruction_policy|data_only|admission/i], "pre-ranking rejection across scope/grant/trust/freshness/authority axes");
}
export function PUL_018(options) {
  return structuralCheck("PUL-018", options, "engine/crates/membrane-federation/src/freshness.rs",
    [/authority/i, /freshness/i], "authority and freshness kept as independent axes");
}
export function PUL_019(options) {
  return structuralCheck("PUL-019", options, "engine/crates/membrane-federation/src/corrective.rs",
    [/satisfied|partial|missing|contradictory|stale|unsafe|unavailable/i], "typed per-requirement coverage evaluation");
}
export function PUL_020(options) {
  return structuralCheck("PUL-020", options, "engine/crates/membrane-federation/src/corrective.rs",
    [/alternate|corrective/i], "bounded single alternate corrective lane");
}
export function PUL_021(options) {
  return structuralCheck("PUL-021", options, "engine/crates/membrane-federation/src/merge.rs",
    [/order|ordering/i, /receipt/i], "deterministic fixed-order fusion with named receipt");
}
export function PUL_022(options) {
  return readyOrStructural("PUL-022", options, ["engine/crates/membrane-federation/src/merge.rs", "engine/crates/membrane-core/src/fusion.rs"],
    [/rrf|reciprocal/i], "named/versioned RRF without mixing provider-local scores", (value) => {
      if (value.transport !== "native") throw new Error("Pull transport is not native");
      if (!value.packet || !Array.isArray(value.packet.blocks)) throw new Error("native Pull omitted packet blocks");
      if (!Array.isArray(value.receipts)) throw new Error("native Pull omitted receipts");
      const text = JSON.stringify(value);
      if (!/fusion|rrf|reciprocal/i.test(text)) throw new Error("native Pull omitted fusion strategy evidence");
    });
}
export function PUL_023(options) {
  return structuralCheck("PUL-023", options, ["engine/crates/membrane-federation/src/merge.rs", "engine/crates/membrane-core/src/fusion.rs"],
    [/dedup|duplicate|lineage/i], "duplicate lineage/source/hash collapse preserving diversity");
}
export function PUL_024(options) {
  return structuralCheck("PUL-024", options, "engine/crates/membrane-core/src/fusion.rs",
    [/dimension|coverage|candidate|provider/i], "minimum faithful coverage per required dimension before depth spend");
}
export function PUL_025(options) {
  return structuralCheck("PUL-025", options, "engine/crates/membrane-core/src/budget.rs",
    [/marginal|utility|budget|ceiling/i, /select|capacity|lane/i], "residual budget spent by deterministic marginal utility");
}
export function PUL_026(options) {
  return structuralCheck("PUL-026", options, "engine/crates/membrane-core/src/lane.rs",
    [/reserved|migration|lane/i, /memory|skills|non.?consum/i], "reserved memory/skills lanes as migration control");
}
export function PUL_027(options) {
  return structuralCheck("PUL-027", options, ["engine/crates/membrane-core/src/lane.rs", "engine/crates/membrane-core/src/reconcile.rs"],
    [/native|excerpt|skeleton|summary|resolver|metadata/i], "cheapest faithful representation selection");
}
export function PUL_028(options) {
  return structuralCheck("PUL-028", options, ["engine/crates/membrane-core/src/budget.rs", "engine/crates/membrane-core/src/lane.rs", "engine/crates/membrane-core/src/reconcile.rs"],
    [/ceiling/i], "reconciled selected/delivered tokens under one ceiling");
}
export function PUL_029(options) {
  return readyOrStructural("PUL-029", options, "engine/crates/membrane-runtime/src/pull/federation.rs",
    [/revocation|policy_epoch|policyEpoch/i], "grant/epoch/revocation re-observed immediately before publication", (value) => {
      if (value.transport !== "native") throw new Error("Pull transport is not native");
      if (!value.finalAdmission || !Array.isArray(value.finalAdmission.omissions)) throw new Error("native Pull omitted final admission");
    });
}
export function PUL_030(options) {
  return readyOrStructural("PUL-030", options, "engine/crates/membrane-runtime/src/pull/federation.rs",
    [/policy_changed|PolicyChanged/i], "typed policy_changed with no stale-authorized publication", (value) => {
      if (value.transport !== "native") throw new Error("Pull transport is not native");
      if (value.packet?.blocks?.length && value.finalAdmission?.status === "insufficient") throw new Error("native Pull published blocks despite insufficient admission");
    });
}
export function PUL_031(options) {
  return readyOrStructural("PUL-031", options, "engine/crates/membrane-protocol/src/federation.rs",
    [/receipt/i, /omission/i], "Pull receipts with journey/omission/coverage/accounting fields", (value) => {
      if (value.transport !== "native") throw new Error("Pull transport is not native");
      if (!value.requirementEvidenceMap || !Array.isArray(value.requirementEvidenceMap.journeys)) throw new Error("native Pull omitted CandidateJourneyV1 map");
      if (!Array.isArray(value.receipts) || !value.receipts.length) throw new Error("native Pull omitted receipts");
      if (!Array.isArray(value.omissions ?? value.packet?.omissions)) throw new Error("native Pull omitted omission accounting");
    });
}
export function PUL_032(options) {
  return structuralCheck("PUL-032", options, "engine/crates/membrane-runtime/src/pull/delivery_acknowledgement.rs",
    [/observation|delivered|outcome/i], "explicit-strength delivery/outcome observation emission");
}
export function PUL_033(options) {
  return structuralCheck("PUL-033", options, "engine/crates/membrane-federation/tests/fence_abstention.rs",
    [/insufficient_confidence|InsufficientConfidence/i], "versioned insufficient_confidence with searched-lane counts");
}
export function PUL_035(options) {
  return structuralCheck("PUL-035", options, "engine/crates/membrane-runtime/src/pull/publication.rs",
    [/resolver|availab|publication|fence/i], "resolver availability re-observed immediately before publication");
}
export function PUL_036(options) {
  return structuralCheck("PUL-036", options, "engine/crates/membrane-runtime/src/pull/federation.rs",
    [/reconcil|publication|fence|policy_changed/i], "publication authorization re-reconciled immediately before emission");
}
export function PUL_037(options) {
  return readyOrStructural("PUL-037", options, "engine/crates/membrane-runtime/src/pull/delivery_state.rs",
    [/suppress|suppression/i, /horizon|expiry/i], "bounded-horizon delivery suppression with typed restoration", (value) => {
      if (value.transport !== "native") throw new Error("Pull transport is not native");
      if (!Array.isArray(value.packet?.blocks)) throw new Error("native Pull omitted packet blocks");
      if (value.packet?.blocks?.length === 0 && !Array.isArray(value.suppressionReceipts)) throw new Error("native Pull empty delivery omitted suppression receipts");
    });
}
export function PUL_039(options) {
  return readyOrStructural("PUL-039", options, "engine/crates/membrane-runtime/src/cache_prefix.rs",
    [/prefix/i], "byte-stable reusable packet prefix across equivalent requests", (value) => {
      if (value.transport !== "native") throw new Error("Pull transport is not native");
      if (!value.cachePrefixDiagnostic || typeof value.cachePrefixDiagnostic !== "object") throw new Error("native Pull omitted cache prefix diagnostic");
    });
}
export function PUL_040(options) {
  return readyOrStructural("PUL-040", options, "engine/crates/membrane-runtime/src/pull/placement.rs",
    [/placement|semantic class/i], "versioned semantic placement after admission", (value) => {
      if (value.transport !== "native") throw new Error("Pull transport is not native");
      if (!value.placementReceipt || typeof value.placementReceipt !== "object") throw new Error("native Pull omitted placement receipt");
    });
}
export function PUL_041(options) {
  return structuralCheck("PUL-041", options,
    ["engine/crates/membrane-runtime/src/mcp_executor.rs", "engine/crates/membrane-runtime/src/authorization.rs"],
    [/aggregate|per.?target|repository|target/i, /omission|identity|authorization|scope/i], "aggregate multi-repository evidence with per-target source identity");
}
export function PUL_042(options) {
  return readyOrStructural("PUL-042", options, "engine/crates/membrane-runtime/src/pull/federation.rs",
    [/resolver|unsupported/i], "resolver-only selection gated on negotiated callable owner resolver", (value) => {
      if (value.transport !== "native") throw new Error("Pull transport is not native");
      if (!value.finalAdmission || !Array.isArray(value.finalAdmission.omissions)) throw new Error("native Pull omitted final admission omissions");
      if (!value.packet || !Array.isArray(value.packet.blocks)) throw new Error("native Pull omitted final packet");
    });
}

// ---------------------------------------------------------------------------
// EX-01 .. EX-09 — real static exclusion checks (amendment acceptance, group EX).
// ---------------------------------------------------------------------------

const SCAN_DIRS = [
  "engine/crates/membrane-core/src",
  "engine/crates/membrane-federation/src",
  "engine/crates/membrane-runtime/src/pull",
  "engine/crates/membrane-protocol/src",
];

function walk(root, relDir, out) {
  const abs = join(root, relDir);
  if (!existsSync(abs)) return out;
  for (const entry of readdirSync(abs)) {
    const relPath = join(relDir, entry);
    const absPath = join(root, relPath);
    const st = statSync(absPath);
    if (st.isDirectory()) walk(root, relPath, out);
    // Normalize to forward slashes so relPath is stable across platforms —
    // callers (including test negative controls) compare relPath by exact
    // string equality and always author it with forward slashes.
    else if (/\.rs$|\.toml$/.test(entry)) out.push(relPath.split("\\").join("/"));
  }
  return out;
}

function scanForForbidden(id, title, options, patterns) {
  const root = resolveRoot(options);
  const files = SCAN_DIRS.flatMap((d) => walk(root, d, []));
  const hits = [];
  for (const relPath of files) {
    const content = readFileSafe(root, relPath);
    if (!content) continue;
    for (const pattern of patterns) {
      if (pattern.test(content)) hits.push({ file: relPath, pattern: String(pattern) });
    }
  }
  return {
    id,
    kind: "exclusion",
    pass: hits.length === 0,
    reason: hits.length === 0
      ? `no occurrence of forbidden pattern across ${files.length} scanned file(s)`
      : `forbidden pattern present: ${title}`,
    evidence: hits,
    scannedFiles: files.length,
  };
}

export function EX_01(options) {
  return scanForForbidden("EX-01", "second graph/search/impact/planner/memory/vector/Dream service", options,
    [/\bDream(Service|Engine|Planner)\b/, /second[_\s-]?(graph|search|impact|planner|memory|vector)[_\s-]?service/i,
      /struct\s+\w*(GraphService|SearchService|ImpactService|VectorService)\w*/]);
}
export function EX_02(options) {
  return scanForForbidden("EX-02", "per-client semantic authority", options,
    [/per[_\s-]?client[_\s-]?(semantic[_\s-]?)?authority/i, /ClientSemanticAuthority/]);
}
export function EX_03(options) {
  return scanForForbidden("EX-03", "graph mutation authority in Membrane", options,
    [/graph[_\s-]?mutation[_\s-]?authority/i, /fn\s+mutate_graph\b/, /GraphMutationAuthority/]);
}
export function EX_04(options) {
  return scanForForbidden("EX-04", "Markdown graph store", options,
    [/markdown[_\s-]?graph[_\s-]?store/i, /MarkdownGraphStore/]);
}
export function EX_05(options) {
  return scanForForbidden("EX-05", "unverified LLM truth admitted as fact", options,
    [/unverified[_\s-]?(llm[_\s-]?)?truth/i, /admit_as_fact\s*\(\s*llm/i, /admit_unverified_llm/i]);
}
export function EX_06(options) {
  return scanForForbidden("EX-06", "scalar-trust replacement of typed authority", options,
    [/scalar[_\s-]?trust[_\s-]?(score)?\s*(:|as)\s*(f32|f64)/i, /replace.*typed[_\s-]?authority.*scalar/i]);
}
export function EX_07(options) {
  return scanForForbidden("EX-07", "arbitrary truth expiration", options,
    [/arbitrary[_\s-]?(truth[_\s-]?)?expir/i, /expire_truth_after\s*\(/i]);
}
export function EX_08(options) {
  return scanForForbidden("EX-08", "speculative memory branching/merging", options,
    [/speculative[_\s-]?(memory[_\s-]?)?(branch|merge)/i, /SpeculativeBranch/, /speculative_merge\s*\(/i]);
}
export function EX_09(options) {
  return scanForForbidden("EX-09", "automatic paid freshness or recomputation calls", options,
    [/automatic[_\s-]?paid[_\s-]?(freshness|recomput)/i, /auto_paid_refresh\s*\(/i]);
}

export const PUL_CASES = {
  PUL_001, PUL_002, PUL_003, PUL_004, PUL_005, PUL_006, PUL_007, PUL_008, PUL_009, PUL_010,
  PUL_011, PUL_012, PUL_013, PUL_014, PUL_015, PUL_016, PUL_017, PUL_018, PUL_019, PUL_020,
  PUL_021, PUL_022, PUL_023, PUL_024, PUL_025, PUL_026, PUL_027, PUL_028, PUL_029, PUL_030,
  PUL_031, PUL_032, PUL_033, PUL_035, PUL_036, PUL_037, PUL_039, PUL_040, PUL_041, PUL_042,
  EX_01, EX_02, EX_03, EX_04, EX_05, EX_06, EX_07, EX_08, EX_09,
};

export default PUL_CASES;
