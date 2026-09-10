#!/usr/bin/env node
// scripts/qualification/cases/bpt-windows.mjs -- blueprint-repair lane case module.
//
// Contract: each export is called by scripts/qualification/run.mjs runOneRegistryCase
// as caseFunction({ row, workspaceRoot, profile, platform, evidencePath }) and must
// return { status: "passed"|<anything else>, evidenceKind, detail, reason } where
// evidenceKind is one of run.mjs EVIDENCE_KINDS. A missing/unrecognized evidenceKind
// is treated by the runner as a hard failure, never a soft pass.
//
// Honesty policy for this module (see docs/agent-rules.md and the wave-A sub-lane
// packet at <lanes>/sub/blueprint-repair__6.json): most windows-acceptance.json BPT
// rows cite a delete-after-parity LEGACY JavaScript implementation under
// blueprint/src/... (this repository still carries that checkout). Per REC-02
// (windows-amendment-acceptance.json) and each row own nativeRequalification note,
// a legacy-cited row DELIVERED status is VOID for the native
// (engine/crates/membrane-blueprint) path until the native owner module passes
// this same case. This module therefore never reports status "passed" for a
// legacyEvidenceVoid row from legacy-artifact presence alone: it records the
// legacy artifact real on-disk presence/content as evidence, but returns a
// non-passed, explicitly reasoned outcome citing the native-requalification gap.
// Rows without a legacy-void flag and with a concrete canonical implementation
// path are checked for real on-disk existence + non-empty content (a structural,
// not functional, proof -- consistent with the sibling pul-windows.mjs convention).
// Rows whose registry entry carries no concrete file path (prose-only
// canonicalImplementationRow) cannot be structurally verified by this module and
// are reported as a typed insufficient/blocked outcome citing the gap rather than
// a fabricated pass.

import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, statSync, symlinkSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = resolve(HERE, "../../../");

function resolveRoot(context) {
  return (context && context.workspaceRoot) || (context && context.root) || REPO_ROOT;
}

function installedExecutable() {
  const root = process.env.MEMBRANE_QUALIFICATION_INSTALLED_ROOT;
  if (!root) throw new Error("MEMBRANE_QUALIFICATION_INSTALLED_ROOT is required");
  const exe = join(resolve(root), "membrane.exe");
  if (!existsSync(exe)) throw new Error(`installed membrane.exe missing: ${exe}`);
  return exe;
}

function nativeCall(exe, args, cwd) {
  const stdout = execFileSync(exe, ["cli", "blueprint", ...args], { cwd, encoding: "utf8", windowsHide: true });
  return JSON.parse(stdout);
}

// Pure classifier for a fail-closed refusal outcome, isolated from process
// spawning so it can be unit-tested directly (see bpt-windows.test.mjs).
// A typed refusal requires BOTH a non-zero exit AND the expected typed
// marker string present in the combined stdout/stderr -- this is the
// negative control: a crash, a wrong exit code, or a marker-less failure
// is never classified as the proven fail-closed outcome.
export function classifyRefusal(exitCode, stdout, stderr, marker) {
  const combined = `${stdout || ""}\n${stderr || ""}`;
  const failed = typeof exitCode === "number" && exitCode !== 0;
  const typed = failed && combined.includes(marker);
  return { failed, typed, combined };
}

// Runs a CLI invocation that is EXPECTED to be refused with a typed fail-closed
// error (e.g. generation_mismatch). Unlike nativeCall, a non-zero exit here is
// the proof of correct behavior, not an unexpected failure -- but only when the
// refusal carries the expected typed marker; any other outcome (success, or a
// non-typed failure) is surfaced for the caller to reject.
function nativeCallExpectRefusal(exe, args, cwd, marker) {
  try {
    const stdout = execFileSync(exe, ["cli", "blueprint", ...args], { cwd, encoding: "utf8", windowsHide: true });
    return { refused: false, typed: false, exitCode: 0, combined: stdout };
  } catch (error) {
    const exitCode = typeof error.status === "number" ? error.status : -1;
    const stdout = error.stdout ? String(error.stdout) : "";
    const stderr = error.stderr ? String(error.stderr) : "";
    const { failed, typed, combined } = classifyRefusal(exitCode, stdout, stderr, marker);
    return { refused: failed, typed, exitCode, combined };
  }
}

function nativeFixture(files, probe) {
  const root = mkdtempSync(join(tmpdir(), "membrane-bpt-"));
  try {
    for (const [path, content] of Object.entries(files)) {
      const target = join(root, path);
      const parent = dirname(target);
      if (!existsSync(parent)) mkdirSync(parent, { recursive: true });
      writeFileSync(target, content);
    }
    return probe(installedExecutable(), root);
  } finally { rmSync(root, { recursive: true, force: true }); }
}

function installedResult(id, fn) {
  try { return { status: "passed", evidenceKind: "installed", detail: { id, ...fn() } }; }
  catch (error) { return { status: "insufficient", evidenceKind: "installed", detail: { id, error: error.message }, reason: `${id}: installed native probe unavailable or failed: ${error.message}` }; }
}

function fileEvidence(root, relPath) {
  const abs = join(root, relPath);
  if (!existsSync(abs)) return { path: relPath, exists: false, nonEmpty: false };
  try {
    const st = statSync(abs);
    if (!st.isFile()) return { path: relPath, exists: false, nonEmpty: false };
    const content = readFileSync(abs, "utf8");
    return { path: relPath, exists: true, nonEmpty: content.trim().length > 0 };
  } catch {
    return { path: relPath, exists: false, nonEmpty: false };
  }
}

function bptRow(id, requirement, files, legacyVoid, context) {
  const root = resolveRoot(context);
  if (!Array.isArray(files) || files.length === 0) {
    return {
      status: "insufficient",
      evidenceKind: "source",
      detail: { id, requirement, files: [], legacyVoid },
      reason: id + ": registry row carries no concrete canonicalImplementationRow file path; cannot structurally verify without fabricating evidence",
    };
  }
  const evidence = files.map((f) => fileEvidence(root, f));
  const allPresent = evidence.every((e) => e.exists && e.nonEmpty);
  if (!allPresent) {
    return {
      status: "failed",
      evidenceKind: "source",
      detail: { id, requirement, evidence, legacyVoid },
      reason: id + ": one or more canonical implementation artifact(s) missing/empty: " + evidence.filter((e) => !e.exists || !e.nonEmpty).map((e) => e.path).join(", "),
    };
  }
  if (legacyVoid) {
    return {
      status: "insufficient",
      evidenceKind: "source",
      detail: { id, requirement, evidence, legacyVoid },
      reason: id + ": legacy artifact present and non-empty, but DELIVERED status is void for the native path per REC-02/nativeRequalification until the engine/crates/membrane-blueprint owner module passes this case",
    };
  }
  return {
    status: "passed",
    evidenceKind: "source",
    detail: { id, requirement, evidence, legacyVoid },
    reason: id + ": canonical implementation artifact(s) present and non-empty (structural presence proof, not a functional proof)",
  };
}

// Real, executable fixture proof for BPT-020/021's selective-invalidation negative control.
// Exported so bpt-windows.test.mjs can inject a fault (a mutated DAG builder that drops the
// `config` parent-edge or that always reports every projection as invalidated -- i.e. a
// full-rebuild instead of a selective one) and assert this check then fails.
export function evaluateSelectiveInvalidation(dagModule) {
  const { buildProjectionDependencyDag, invalidatedProjections, PROJECTION_DEPENDENCIES } = dagModule;
  if (typeof buildProjectionDependencyDag !== "function" || typeof invalidatedProjections !== "function" || !PROJECTION_DEPENDENCIES) {
    return { pass: false, reason: "module does not export buildProjectionDependencyDag/invalidatedProjections/PROJECTION_DEPENDENCIES" };
  }
  const dag = buildProjectionDependencyDag({
    sourceHash: "sha256:source-v1", providerDigest: "sha256:provider-v1",
    configDigest: "sha256:config-v1", schemaVersion: "v1", generationId: "gen-1",
  });

  const parentCases = [
    { parent: "source", label: "source change" },
    { parent: "provider", label: "provider change" },
    { parent: "config", label: "config change" },
    { parent: "schema", label: "schema change" },
    { parent: "generation", label: "generation-parent change" },
  ];
  const findings = [];
  for (const { parent, label } of parentCases) {
    const expected = Object.entries(PROJECTION_DEPENDENCIES)
      .filter(([, deps]) => deps.includes(parent))
      .map(([name]) => name)
      .sort();
    const actual = [...invalidatedProjections(dag, [parent])].sort();
    const expectedSet = new Set(expected);
    const actualSet = new Set(actual);
    const matches = expected.length === actual.length && expected.every((p) => actualSet.has(p));
    // Full-rebuild fault detector: a real SELECTIVE invalidation must never invalidate a
    // projection that does not declare this parent as a dependency (unless every projection
    // happens to declare it, which none of the five parent axes below do).
    const overInvalidated = actual.filter((p) => !expectedSet.has(p));
    findings.push({ parent, label, expected, actual, matches, overInvalidated });
  }
  const allMatch = findings.every((f) => f.matches && f.overInvalidated.length === 0);
  const configFinding = findings.find((f) => f.parent === "config");
  const configSelective = configFinding && configFinding.expected.length > 0 && configFinding.expected.length < Object.keys(PROJECTION_DEPENDENCIES).length;
  return {
    pass: allMatch && Boolean(configSelective),
    reason: allMatch && configSelective
      ? `each of source/provider/config/schema/generation-parent changes selectively invalidates exactly its declared dependent projections (config: ${configFinding.expected.join(",")}), never a full rebuild`
      : `selective invalidation fixture mismatch: ${JSON.stringify(findings.filter((f) => !f.matches || f.overInvalidated.length > 0))}`,
    findings,
  };
}

// BPT-026 real, executable ranking negative control (r5 requirement). Imports the real legacy
// non-compensatory comparator (blueprint/src/graph/recall-circuit.mjs `comparePaths`, exported
// specifically so this contract can be asserted at the point it is decided) and proves that
// fixtures differing only in confidence(-adjacent authority rank) and hop-count reorder under it.
// See bpt-windows.test.mjs for the negative control that substitutes a compensatory/summed
// comparator and asserts this check then fails.
export async function bpt026RankingCheck(context) {
  const root = resolveRoot(context);
  const modulePath = join(root, "blueprint/src/graph/recall-circuit.mjs");
  if (!existsSync(modulePath)) {
    return {
      status: "insufficient", evidenceKind: "source", detail: { id: "BPT-026" },
      reason: "BPT-026: legacy recall-circuit.mjs module not found; cannot exercise ranking fixture",
    };
  }
  let mod;
  try {
    mod = await import(pathToFileURL(modulePath).href);
  } catch (error) {
    return {
      status: "failed", evidenceKind: "source", detail: { id: "BPT-026", error: error.message },
      reason: `BPT-026: failed to import recall-circuit.mjs: ${error.message}`,
    };
  }
  const check = evaluateRankingComparator(mod.comparePaths);
  return {
    status: check.pass ? "insufficient" : "failed",
    evidenceKind: "source",
    detail: { id: "BPT-026", ...check },
    reason: check.pass
      ? "BPT-026: real legacy comparePaths reorders under both a confidence/authority-rank change and an independent hop-count change (fixture-level proof, case writer independence per row note not applicable to this automated fixture), but DELIVERED status is void for the native path per REC-02 until engine/crates/membrane-blueprint lands equivalent ranking and this case is requalified against it"
      : `BPT-026: ranking fixture failed against the real legacy comparePaths: ${check.reason}`,
  };
}
export function BPT_026(context) {
  return bpt026RankingCheck(context);
}

// Exported for the negative-control test to call with a substitute (faulty) comparator.
export function evaluateRankingComparator(comparePaths) {
  if (typeof comparePaths !== "function") return { pass: false, reason: "comparePaths export is not a function" };
  const base = { state: "complete", semanticAuthorityRank: 0, minimumEdgeTier: "EXACT_RESOLUTION", seedExactness: 1, evidenceCoverage: 1, hopCount: 1, id: "path:a" };
  // Named ranking factor 1: semantic authority / confidence-tier rank differs.
  const strongerAuthority = { ...base, id: "path:b" };
  const weakerAuthority = { ...base, id: "path:c", semanticAuthorityRank: 5, minimumEdgeTier: "CROSS_FILE_HEURISTIC" };
  const authorityOrder = [weakerAuthority, strongerAuthority].sort(comparePaths).map((p) => p.id);
  const authorityChanged = authorityOrder[0] === strongerAuthority.id;

  // Named ranking factor 2: hop-count differs, with everything else held equal.
  const fewerHops = { ...base, id: "path:d", hopCount: 1 };
  const moreHops = { ...base, id: "path:e", hopCount: 9 };
  const hopOrder = [moreHops, fewerHops].sort(comparePaths).map((p) => p.id);
  const hopChanged = hopOrder[0] === fewerHops.id;

  return {
    pass: authorityChanged && hopChanged,
    reason: authorityChanged && hopChanged
      ? "both the semantic-authority/confidence-tier factor and the hop-count factor independently changed result order"
      : `expected authority and hop-count factors to both reorder results; authorityChanged=${authorityChanged} hopChanged=${hopChanged}`,
    authorityOrder, hopOrder,
  };
}

// ---------------------------------------------------------------------------
// BPT-001 .. BPT-071 -- one export per installed acceptance row.
// ---------------------------------------------------------------------------

export function BPT_001(context) {
  return installedResult("BPT-001", () => nativeFixture({
    "src/root-probe.mjs": "export function rootProbe() { return 'bpt-root-probe'; }\n",
  }, (exe, root) => {
    const canonical = realpathSync.native(root);
    const initial = nativeCall(exe, ["refresh", "--repo-root", root], root);
    const initialStatus = nativeCall(exe, ["status", "--repo-root", root], root);
    const generationId = initial.generationId;
    if (!generationId || initialStatus.generationId !== generationId) throw new Error("canonical root build/status generation mismatch");

    const traversalStatus = nativeCall(exe, ["status", "--repo-root", `${root}\\src\\..`], root);
    if (traversalStatus.generationId !== generationId) throw new Error("traversal spelling selected a different repository");

    const caseStatus = nativeCall(exe, ["status", "--repo-root", root.toUpperCase()], root);
    if (caseStatus.generationId !== generationId) throw new Error("case variant selected a different repository");

    const junction = join(dirname(root), `${root.split(/[\\\\/]/).pop()}-junction`);
    try {
      symlinkSync(root, junction, "junction");
      const junctionStatus = nativeCall(exe, ["status", "--repo-root", junction], root);
      if (junctionStatus.generationId !== generationId) throw new Error("junction spelling selected a different repository");
    } finally {
      if (existsSync(junction)) rmSync(junction, { recursive: true, force: true });
    }

    const sibling = join(dirname(root), `${root.split(/[\\\\/]/).pop()}-sibling`);
    mkdirSync(sibling, { recursive: true });
    try {
      writeFileSync(join(sibling, "src.mjs"), "export const sibling = true;\n");
      const siblingStatus = nativeCall(exe, ["status", "--repo-root", sibling], sibling);
      if (siblingStatus.state !== "missing" || (siblingStatus.generationId ?? null) !== null) {
        throw new Error(`shared-prefix sibling did not return rawcanon absence: state=${siblingStatus.state ?? "null"} generationId=${siblingStatus.generationId ?? "null"}`);
      }
    } finally {
      rmSync(sibling, { recursive: true, force: true });
    }

    const ambiguous = nativeCall(exe, ["status", "--repo-root", canonical + "\\."], root);
    if (ambiguous.generationId !== generationId) throw new Error("equivalent root selector was not canonicalized");
    return { canonicalRoot: canonical, generationId, checks: ["traversal", "case", "junction", "prefix-sibling", "ambiguous-equivalent"] };
  }));
}
export function BPT_002(context) {
  return bptRow("BPT-002", "Observe HEAD/index/worktree, dirty/untracked overlay, merge-base/treeish, source hashes, & deterministic discovery accounting.", ["blueprint/scripts/blueprint.mjs", "blueprint/src/graph/git-source-observation.mjs", "blueprint/src/sources/live-overlay.mjs"], true, context);
}
export function BPT_003(context) {
  return bptRow("BPT-003", "Give every considered source one typed terminal ingestion disposition with no silent disappearance.", [], false, context);
}
export function BPT_004(context) {
  return bptRow("BPT-004", "Extract deterministic lexical symbols/occurrences/edges as baseline/fallback.", ["blueprint/scripts/blueprint.mjs", "blueprint/src/graph/language-extractors.mjs", "blueprint/src/graph/static-provider.mjs"], true, context);
}
export function BPT_005(context) {
  return bptRow("BPT-005", "Load pinned verified Tree-sitter grammars & emit structural facts with explicit language capability.", ["blueprint/scripts/blueprint.mjs", "blueprint/src/graph/static-provider.mjs", "blueprint/src/graph/treesitter-provider.mjs"], true, context);
}
export function BPT_006(context) {
  return bptRow("BPT-006", "Ingest SCIP definitions/references/relations/roles/ranges/diagnostics with integrity & position-encoding validation.", ["blueprint/src/graph/scip-provider.mjs", "blueprint/src/graph/static-provider.mjs", "blueprint/src/providers/build.mjs"], true, context);
}
export function BPT_007(context) {
  return bptRow("BPT-007", "Resolve JavaScript & Python module/import bindings deterministically.", ["blueprint/src/providers/build.mjs", "blueprint/src/providers/modules/javascript.mjs", "blueprint/src/providers/modules/python-resolver.mjs"], true, context);
}
export function BPT_008(context) {
  return bptRow("BPT-008", "Emit HTTP-domain facts under declared provider capability.", ["blueprint/src/providers/build.mjs", "blueprint/src/providers/frameworks/http/index.mjs"], true, context);
}
export function BPT_009(context) {
  return bptRow("BPT-009", "Emit SQL schema facts under declared provider capability.", ["blueprint/src/providers/build.mjs", "blueprint/src/providers/schemas/sql.mjs"], true, context);
}
export function BPT_010(context) {
  return bptRow("BPT-010", "Register one capability/permission-based provider system; validate identity, checksum, licence, isolation, & supported/unsupported result.", [], false, context);
}
export function BPT_011(context) {
  return bptRow("BPT-011", "Ingest rules/documents only as evidence-bound declarations/claims, never observed code facts or authority.", [], false, context);
}
export function BPT_012(context) {
  return bptRow("BPT-012", "Run trusted providers repository-read-only, network-free, process-bounded, cancellable, & crash/hang-typed.", [], false, context);
}
export function BPT_013(context) {
  return bptRow("BPT-013", "Assign stable repo/file/entity/occurrence/claim/evidence/generation identities & reconcile rename/move deterministically.", [], false, context);
}
export function BPT_014(context) {
  return bptRow("BPT-014", "Bind evidence to source address, span/hash, provider/version, generation, truth class, confidence, & freshness.", ["blueprint/src/graph/static-provider.mjs", "blueprint/src/graph/store-sqlite.mjs"], true, context);
}
export function BPT_015(context) {
  return bptRow("BPT-015", "Stage, verify, & atomically publish immutable SQLite generations while preserving last-known-good.", ["blueprint/scripts/blueprint.mjs", "blueprint/src/graph/atomic-store-adoption.mjs", "blueprint/src/graph/static-provider.mjs", "blueprint/src/graph/store-sqlite.mjs"], true, context);
}
export function BPT_016(context) {
  return bptRow("BPT-016", "Lease one writer, enforce busy timeout/single-flight, migrate/rollback, & recover interrupted adoption.", ["blueprint/src/graph/barrier.mjs", "blueprint/src/graph/static-provider.mjs", "blueprint/src/graph/store-lease.mjs", "blueprint/src/graph/store-sqlite.mjs"], true, context);
}
export function BPT_017(context) {
  return bptRow("BPT-017", "Conservatively re-anchor by exact entity/text/fingerprint/unique normalized text, else stale/ambiguous.", [], false, context);
}
export function BPT_018(context) {
  return bptRow("BPT-018", "Resolve cross-file identities exact-first; same-tier ambiguity stops & unsupported module semantics remain typed.", ["blueprint/src/providers/build.mjs", "blueprint/src/providers/modules/javascript.mjs", "blueprint/src/providers/modules/python-resolver.mjs"], true, context);
}
export function BPT_019(context) {
  return installedResult("BPT-019", () => nativeFixture({
    "src/main.rs": "fn main() { println!(\"freshness\"); }\n",
  }, (exe, root) => {
    const refresh = nativeCall(exe, ["refresh", "--repo-root", root], root);
    const fresh = nativeCall(exe, ["status", "--repo-root", root], root);
    if (fresh.state !== "fresh" || fresh.generationId !== refresh.generationId) throw new Error("status did not report current generation fresh");
    writeFileSync(join(root, "src/main.rs"), "fn main() { println!(\"changed\"); }\n");
    const stale = nativeCall(exe, ["status", "--repo-root", root], root);
    if (stale.state !== "stale" || stale.fresh === true) throw new Error("source edit remained old-fresh");
    return { generationId: refresh.generationId, before: fresh.state, after: stale.state };
  }));
}
// BPT-020/021 real, executable selective-invalidation fixtures (r5 requirement).
// This imports the real legacy dependency-DAG module (blueprint/src/graph/dependency-dag.mjs,
// still present in this repository ahead of native cutover) and exercises it with fixtures
// spanning source/provider/config/schema/generation-parent changes, asserting SELECTIVE (not
// full-rebuild) invalidation: a changed `config` parent must invalidate exactly the projections
// that declare `config` as a dependency (contracts/processes/conventions/orientation) and must
// NOT invalidate projections that do not (bm25/structural_search/signatures). This is a real,
// importable, runnable function — see bpt-windows.test.mjs for the companion negative control
// that injects a fault (a DAG missing the `config` edge) and asserts the check then fails.
export async function bpt020DependencyDagInvalidation(context) {
  const root = resolveRoot(context);
  const modulePath = join(root, "blueprint/src/graph/dependency-dag.mjs");
  if (!existsSync(modulePath)) {
    return {
      status: "insufficient",
      evidenceKind: "source",
      detail: { id: "BPT-020" },
      reason: "BPT-020: legacy dependency-dag.mjs module not found; cannot exercise selective-invalidation fixture",
    };
  }
  let mod;
  try {
    mod = await import(pathToFileURL(modulePath).href);
  } catch (error) {
    return {
      status: "failed",
      evidenceKind: "source",
      detail: { id: "BPT-020", error: error.message },
      reason: `BPT-020: failed to import dependency-dag.mjs: ${error.message}`,
    };
  }
  const check = evaluateSelectiveInvalidation(mod);
  return {
    status: check.pass ? "insufficient" : "failed",
    evidenceKind: "source",
    detail: { id: "BPT-020", ...check },
    reason: check.pass
      ? "BPT-020: legacy dependency-dag.mjs demonstrates real selective (not full-rebuild) invalidation across source/provider/config/schema/generation parents (fixture-level proof), but DELIVERED status is void for the native path per REC-02 until engine/crates/membrane-blueprint lands the same DAG and this case is requalified against it"
      : `BPT-020: selective-invalidation fixture failed against the real legacy dependency-dag.mjs: ${check.reason}`,
  };
}
export function BPT_020(context) {
  return bpt020DependencyDagInvalidation(context);
}

// BPT-021 covers full cold-vs-incremental build equivalence across nine canon cases; this module
// cannot install/build the native or legacy toolchain (worker never runs builds), so it does not
// claim full parity. It DOES reuse the same real dependency-DAG fixture as BPT-020 to prove the
// selective-invalidation portion of the claim ("selective (not full-rebuild) invalidation asserted
// distinct from Phase-2 claim invalidation" per this row's own negativeControls text), and records
// the remaining eight equivalence scenarios (add/remove/rename/move/provider/crash/dirty/no-op) as
// not evaluated by this module rather than fabricating a pass.
export async function BPT_021(context) {
  const dagResult = await bpt020DependencyDagInvalidation(context);
  const legacy = bptRow("BPT-021", "Make incremental build semantically equivalent to full build across add/remove/rename/move/config/provider/crash/dirty/no-op.", ["blueprint/src/providers/build.mjs", "blueprint/watchman/reconcile.mjs"], true, context);
  return {
    status: "insufficient",
    evidenceKind: "source",
    detail: { id: "BPT-021", dagSelectiveInvalidation: dagResult, canonicalArtifactCheck: legacy },
    reason: "BPT-021: selective-invalidation portion of the equivalence claim is proven against real legacy dependency-dag.mjs (see detail.dagSelectiveInvalidation); the remaining add/remove/rename/move/provider/crash/dirty/no-op cold-vs-incremental equivalence scenarios require an installed build run this edit-only module cannot perform and are not evaluated here",
  };
}
export function BPT_023(context) {
  return installedResult("BPT-023", () => nativeFixture({
    "src/main.rs": "fn helper() {}\nfn main() { helper(); }\n",
  }, (exe, root) => {
    nativeCall(exe, ["refresh", "--repo-root", root], root);
    const result = nativeCall(exe, ["recall", "--repo-root", root, "--seed", "helper", "--direction", "both"], root);
    if (!result.generationId || !result.resolution || !Array.isArray(result.nodes) || !Array.isArray(result.omissions)) throw new Error("recall omitted resolution, generation, nodes, or omissions");
    const state = result.resolution.state;
    if (!["resolved", "ambiguous", "unresolved"].includes(state)) throw new Error(`recall returned unknown resolution state ${state}`);
    return { generationId: result.generationId, resolutionState: state, nodeCount: result.nodes.length };
  }));
}
export function BPT_024(context) {
  return installedResult("BPT-024", () => nativeFixture({
    "src/main.rs": "fn dependency() {}\nfn caller() { dependency(); }\nfn main() { caller(); }\n",
  }, (exe, root) => {
    nativeCall(exe, ["refresh", "--repo-root", root], root);
    const policies = ["in", "out", "both"].map((direction) => nativeCall(exe, ["recall", "--repo-root", root, "--seed", "caller", "--direction", direction, "--depth", "2"], root));
    for (const result of policies) if (!result.generationId || !result.resolution || !Array.isArray(result.nodes) || !Array.isArray(result.edges) || !Array.isArray(result.omissions)) throw new Error("recall policy omitted bounded result fields");
    return { policyCount: policies.length, directions: ["in", "out", "both"], states: policies.map((result) => result.state) };
  }));
}
export function BPT_025(context) {
  return installedResult("BPT-025", () => nativeFixture({
    "src/main.rs": "fn first() {}\nfn middle() { first(); }\nfn last() { middle(); }\n",
  }, (exe, root) => {
    nativeCall(exe, ["refresh", "--repo-root", root], root);
    const result = nativeCall(exe, ["path", "--repo-root", root, "--from", "last", "--to", "first", "--depth", "4"], root);
    if (!result.generationId || !Array.isArray(result.path) || !Array.isArray(result.edges) || !Array.isArray(result.omissions) || typeof result.found !== "boolean") throw new Error("path omitted atomic evidence fields");
    for (const edge of result.edges) if (!edge.edgeId && !edge.id) throw new Error("path edge omitted stable identity");
    return { generationId: result.generationId, found: result.found, pathLength: result.path.length, edgeCount: result.edges.length };
  }));
}
export function BPT_027(context) {
  return installedResult("BPT-027", () => nativeFixture({
    "src/main.rs": "fn leaf() {}\nfn branch() { leaf(); }\nfn main() { branch(); }\n",
  }, (exe, root) => {
    const refresh = nativeCall(exe, ["refresh", "--repo-root", root], root);
    const result = nativeCall(exe, ["expand", "--repo-root", root, "--seed", "branch", "--direction", "both", "--limit", "1", "--generation", refresh.generationId], root);
    if (!result.generationId || !Array.isArray(result.omissions) || !result.candidateSet) throw new Error("bounded expand omitted generation, candidate set, or omissions");
    const mismatch = nativeCall(exe, ["expand", "--repo-root", root, "--seed", "branch", "--generation", "closed-generation"], root);
    if (mismatch.state !== "suppressed" || !/stale_generation|generation/i.test(JSON.stringify(mismatch))) throw new Error("old generation did not return typed suppressed response");
    return { generationId: result.generationId, omissionCount: result.omissions.length, oldGenerationState: mismatch.state };
  }));
}
export function BPT_028(context) {
  return installedResult("BPT-028", () => nativeFixture({
    "src/main.rs": "fn searchable() {}\nfn main() { searchable(); }\n",
  }, (exe, root) => {
    const refresh = nativeCall(exe, ["refresh", "--repo-root", root], root);
    const result = nativeCall(exe, ["search", "--repo-root", root, "--query", "searchable", "--generation", refresh.generationId], root);
    if (!result.generationId || result.generationId !== refresh.generationId || !Array.isArray(result.candidates) || !Array.isArray(result.omissions)) throw new Error("search omitted generation-bound candidates or omissions");
    return { generationId: result.generationId, candidateCount: result.candidates.length };
  }));
}
export function BPT_029(context) {
  return installedResult("BPT-029", () => nativeFixture({
    "src/main.rs": "fn anchor() {}\nfn main() { anchor(); }\n",
  }, (exe, root) => {
    nativeCall(exe, ["refresh", "--repo-root", root], root);
    const result = nativeCall(exe, ["resolve", "--repo-root", root, "--target", "anchor"], root);
    if (!result.generationId || !result.resolution || !result.candidateSet) throw new Error("resolve omitted generation, resolution, or candidate set");
    return { generationId: result.generationId, resolutionState: result.resolution.state, candidateState: result.candidateSet.state };
  }));
}
export function BPT_030(context) {
  return installedResult("BPT-030", () => nativeFixture({
    "src/main.rs": "fn child() {}\nfn parent() { child(); }\nfn main() { parent(); }\n",
  }, (exe, root) => {
    nativeCall(exe, ["refresh", "--repo-root", root], root);
    const result = nativeCall(exe, ["expand", "--repo-root", root, "--seed", "parent", "--direction", "out", "--depth", "2"], root);
    if (!result.generationId || !Array.isArray(result.nodes) || !Array.isArray(result.edges) || !result.depths || typeof result.depths !== "object") throw new Error("expand omitted typed neighborhood fields");
    return { generationId: result.generationId, nodeCount: result.nodes.length, edgeCount: result.edges.length };
  }));
}
export function BPT_031(context) {
  return installedResult("BPT-031", () => nativeFixture({
    "src/main.rs": "fn first() {}\nfn middle() { first(); }\nfn last() { middle(); }\nfn main() { last(); }\n",
  }, (exe, root) => {
    nativeCall(exe, ["refresh", "--repo-root", root], root);
    const result = nativeCall(exe, ["path", "--repo-root", root, "--from", "last", "--to", "first", "--depth", "4"], root);
    if (!result.generationId || !Array.isArray(result.path) || !Array.isArray(result.edges) || !Array.isArray(result.omissions)) throw new Error("path omitted generation, nodes, edges, or omissions");
    return { generationId: result.generationId, found: result.found === true, pathLength: result.path.length };
  }));
}
export function BPT_032(context) {
  return installedResult("BPT-032", () => nativeFixture({
    "src/main.rs": "fn source() {}\nfn caller() { source(); }\nfn main() { caller(); }\n",
  }, (exe, root) => {
    const refresh = nativeCall(exe, ["refresh", "--repo-root", root], root);
    const result = nativeCall(exe, ["impact", "--repo-root", root, "--node", "source", "--depth", "2", "--generation", refresh.generationId], root);
    if (!result.generationId || !Array.isArray(result.impact) || !Array.isArray(result.edges) || !Array.isArray(result.omissions)) throw new Error("impact omitted bounded evidence fields");
    for (const item of result.impact) if (typeof item.class !== "string" || !item.edgeId) throw new Error("impact overstated adjacency without typed edge classification");
    return { generationId: result.generationId, impactCount: result.impact.length, edgeCount: result.edges.length };
  }));
}
export function BPT_033(context) {
  return bptRow("BPT-033", "Report liveness only as LIVE/UNREACHED/UNKNOWN with evidence; zero inbound edges never proves dead.", [], false, context);
}
export function BPT_034(context) {
  return bptRow("BPT-034", "Recommend tests with evidence/reason, uncovered impact, coverage, & omissions without unproved minimality.", [], false, context);
}
export function BPT_035(context) {
  return bptRow("BPT-035", "Decompose change risk into inspectable factors & keep co-change lower-authority.", ["blueprint/src/graph/analytics/change-impact.mjs", "blueprint/src/lib/application/service.mjs"], true, context);
}
export function BPT_036(context) {
  return bptRow("BPT-036", "Create/list/get named graph snapshots.", ["blueprint/scripts/blueprint.mjs", "blueprint/src/graph/snapshots.mjs"], true, context);
}
export function BPT_037(context) {
  return bptRow("BPT-037", "Report semantic changes since snapshot/generation/treeish while history never overwrites current truth.", ["blueprint/src/graph/analytics/change-impact.mjs", "blueprint/src/graph/snapshots.mjs", "blueprint/src/lib/application/service.mjs"], true, context);
}
export function BPT_038(context) {
  return bptRow("BPT-038", "Bind claims to facts & expose direct/indirect/unsupported/contradicted/ambiguous/stale grounding.", [], false, context);
}
export function BPT_039(context) {
  return bptRow("BPT-039", "Compare declared intent vs deterministic evidence while preserving both, mismatch, citation, generation, confidence, & invalidation.", [], false, context);
}
export function BPT_040(context) {
  return installedResult("BPT-040", () => nativeFixture({
    "src/main.rs": "fn component() {}\nfn main() { component(); }\n",
  }, (exe, root) => {
    const refresh = nativeCall(exe, ["refresh", "--repo-root", root], root);
    const result = nativeCall(exe, ["architecture", "--repo-root", root, "--task", "component", "--generation", refresh.generationId], root);
    if (!result.generationId || result.generationId !== refresh.generationId || !result.sourceSignature || !result.anchors || !result.callees || !result.callers) throw new Error("architecture omitted generation-bound cited views");
    return { generationId: result.generationId, state: result.state, sourceDisposition: result.sourceSignature.disposition };
  }));
}
export function BPT_041(context) {
  return installedResult("BPT-041", () => nativeFixture({
    "src/main.rs": "fn orient() {}\nfn main() { orient(); }\n",
  }, (exe, root) => {
    const refresh = nativeCall(exe, ["refresh", "--repo-root", root], root);
    const result = nativeCall(exe, ["architecture", "--repo-root", root, "--task", "orient", "--generation", refresh.generationId], root);
    if (!result.generationId || !result.state || !result.freshness || !result.omissions || !result.sourceSignature) throw new Error("orientation omitted generation, freshness, evidence, or omissions");
    return { generationId: result.generationId, state: result.state, freshness: result.freshness };
  }));
}
export function BPT_042(context) {
  return bptRow("BPT-042", "Serve same application semantics through daemon-owned IPC, bounded one-shot direct mode, CLI, JS SDK, & native MCP adapters.", [], false, context);
}
export function BPT_043(context) {
  return bptRow("BPT-043", "Enroll roots & run watcher/reconciler only under active tray daemon; stop with daemon & type watcher loss.", ["blueprint/src/service/server.mjs", "blueprint/watchman/repo-actor.mjs", "blueprint/watchman/supervisor.mjs"], true, context);
}
export function BPT_044(context) {
  return bptRow("BPT-044", "Return canonical result envelope + stable typed error/retry/partial-result taxonomy across adapters.", [], false, context);
}
export function BPT_046(context) {
  return installedResult("BPT-046", () => nativeFixture({
    "src/main.rs": "fn diagnose() {}\nfn main() { diagnose(); }\n",
  }, (exe, root) => {
    const refresh = nativeCall(exe, ["refresh", "--repo-root", root], root);
    const status = nativeCall(exe, ["status", "--repo-root", root], root);
    if (!refresh.generationId || !status.generationId || !status.state || typeof status.fresh !== "boolean" || !status.storePath) throw new Error("status omitted local diagnostic state");
    return { generationId: status.generationId, state: status.state, fresh: status.fresh, storePath: status.storePath };
  }));
}
export function BPT_047(context) {
  return bptRow("BPT-047", "Federate explicit repositories as independent generation/evidence/omission slices without merging node spaces.", ["blueprint/src/lib/application/service.mjs", "blueprint/src/lib/federation/index.mjs"], true, context);
}
export function BPT_049(context) {
  return bptRow("BPT-049", "Capture, list & compare findings baselines with deterministic delta identity.", [], false, context);
}
export function BPT_050(context) {
  return bptRow("BPT-050", "Export detected findings as bounded SARIF without granting remediation authority.", [], false, context);
}
export function BPT_051(context) {
  return bptRow("BPT-051", "Explain one finding through source-bound evidence & rule reasoning.", [], false, context);
}
export function BPT_052(context) {
  return bptRow("BPT-052", "Produce a source-bound evidence pack for selected findings through governed host path.", [], false, context);
}
export function BPT_053(context) {
  return bptRow("BPT-053", "Extract framework-specific gated event/database/deployment facts into Blueprint evidence.", ["blueprint/src/providers/build.mjs", "blueprint/src/providers/frameworks/index.mjs"], true, context);
}
export function BPT_054(context) {
  return bptRow("BPT-054", "Extract Terraform facts into generation-bound Blueprint evidence.", ["blueprint/src/providers/build.mjs", "blueprint/src/providers/iac/terraform.mjs"], true, context);
}
export function BPT_055(context) {
  return bptRow("BPT-055", "Produce a redacted Blueprint support bundle.", ["blueprint/scripts/blueprint.mjs", "blueprint/src/lib/operations/support-bundle.mjs"], true, context);
}
export function BPT_056(context) {
  return bptRow("BPT-056", "Recover Blueprint state after corruption.", ["blueprint/scripts/blueprint.mjs", "blueprint/src/lib/operations/repair.mjs"], true, context);
}
export function BPT_057(context) {
  return bptRow("BPT-057", "Refuse poisoned manifests or plugins before Blueprint acceptance.", ["blueprint/src/sdk/providers.mjs"], false, context);
}
export function BPT_058(context) {
  return bptRow("BPT-058", "Activate staged Blueprint self-update & recover interrupted apply transaction.", ["blueprint/scripts/cli/commands.mjs", "blueprint/src/lib/update/apply.mjs"], true, context);
}
export function BPT_059(context) {
  return bptRow("BPT-059", "Make local Blueprint explorer available through its owned shell.", ["blueprint/scripts/cli/commands.mjs", "blueprint/src/lib/explorer/static.mjs", "blueprint/src/lib/http-server.mjs"], true, context);
}
export function BPT_060(context) {
  return bptRow("BPT-060", "Prevent secret egress through redaction on Blueprint operational & MCP surfaces.", ["blueprint/scripts/blueprint-mcp.mjs", "blueprint/scripts/blueprint.mjs", "blueprint/src/lib/operations/support-bundle.mjs", "blueprint/src/lib/redaction.mjs"], true, context);
}
export function BPT_061(context) {
  return bptRow("BPT-061", "Admit only trusted signed update manifests & matching local artifacts.", ["blueprint/scripts/cli/commands.mjs", "blueprint/src/lib/update/apply.mjs", "blueprint/src/lib/update/manifest.mjs"], true, context);
}
export function BPT_062(context) {
  return bptRow("BPT-062", "Roll back one update only from receipt-bound verified app/store state.", ["blueprint/scripts/cli/commands.mjs", "blueprint/src/lib/update/rollback.mjs"], true, context);
}
export function BPT_063(context) {
  return bptRow("BPT-063", "Recognize GitHub Release archives & require signed update artifacts.", ["blueprint/scripts/cli/commands.mjs", "blueprint/src/lib/update/channel.mjs"], true, context);
}
export function BPT_064(context) {
  return bptRow("BPT-064", "Bind each Blueprint Explorer listener only to `127.0.0.1` on an ephemeral port.", ["blueprint/scripts/cli/commands.mjs", "blueprint/src/lib/explorer/index.mjs", "blueprint/src/lib/http-server.mjs"], true, context);
}
export function BPT_065(context) {
  return bptRow("BPT-065", "Detect BP001 imports whose resolved module does not export imported symbol.", ["blueprint/src/lib/findings/detect.mjs", "blueprint/src/lib/findings/registry.mjs", "blueprint/src/lib/findings/service.mjs", "blueprint/src/service/client.mjs", "blueprint/src/service/protocol.mjs", "blueprint/src/service/server.mjs"], true, context);
}
export function BPT_066(context) {
  return bptRow("BPT-066", "Detect BP002 import specifiers resolving to neither repository file nor package.", ["blueprint/src/lib/findings/detect.mjs", "blueprint/src/lib/findings/registry.mjs", "blueprint/src/lib/findings/service.mjs", "blueprint/src/service/client.mjs", "blueprint/src/service/protocol.mjs", "blueprint/src/service/server.mjs"], true, context);
}
export function BPT_067(context) {
  return bptRow("BPT-067", "Detect BP003 barrel re-exports whose target binding is absent.", ["blueprint/src/lib/findings/detect.mjs", "blueprint/src/lib/findings/registry.mjs", "blueprint/src/lib/findings/service.mjs", "blueprint/src/service/client.mjs", "blueprint/src/service/protocol.mjs", "blueprint/src/service/server.mjs"], true, context);
}
export function BPT_068(context) {
  return bptRow("BPT-068", "Require an unguessable in-memory session token for every Explorer API request.", ["blueprint/scripts/cli/commands.mjs", "blueprint/src/lib/explorer/index.mjs", "blueprint/src/lib/http-server.mjs"], true, context);
}
export function BPT_069(context) {
  return bptRow("BPT-069", "Reject every non-GET Explorer request before route dispatch.", ["blueprint/scripts/cli/commands.mjs", "blueprint/src/lib/explorer/index.mjs", "blueprint/src/lib/http-server.mjs"], true, context);
}
export function BPT_070(context) {
  return bptRow("BPT-070", "Never launch a browser child carrying Explorer session URL or token; expose URL only to caller.", ["blueprint/scripts/cli/commands.mjs", "blueprint/src/lib/explorer/index.mjs"], true, context);
}
export function BPT_071(context) {
  return bptRow("BPT-071", "Emit typed source-addressed cross-language bridge evidence only for explicit FFI/JNI/cgo/gRPC/PInvoke/WASM/COM seams under declared provider capability; never infer bridges from semantic similarity.", ["blueprint/src/providers/bridges/seams.mjs", "blueprint/src/providers/build.mjs"], true, context);
}

// ---------------------------------------------------------------------------
// BM03 / BM04 / BM05 (blueprint-membrane-acceptance.json, group BPT) --
// checked against the REAL landed native Rust source in
// engine/crates/membrane-blueprint (owned by sibling sub-lanes; this module
// only reads it, never edits it -- see myAllowlist). Each check greps the
// actual current source for the specific contract markers the requirement
// names, so a genuine gap reads as `failed`/`insufficient`, never a
// fabricated pass.
// ---------------------------------------------------------------------------

function nativeSourceCheck(id, requirement, relPath, markers, note, context) {
  const root = resolveRoot(context);
  const abs = join(root, relPath);
  const content = existsSync(abs) ? readFileSync(abs, "utf8") : null;
  if (content === null) {
    return {
      status: "insufficient",
      evidenceKind: "source",
      detail: { id, requirement, relPath },
      reason: `${id}: native artifact ${relPath} not found; sibling sub-lane has not yet landed this surface`,
    };
  }
  const hits = markers.filter((marker) => (marker instanceof RegExp ? marker.test(content) : content.includes(marker)));
  const allPresent = hits.length === markers.length;
  return {
    status: allPresent ? "passed" : "failed",
    evidenceKind: "source",
    detail: { id, requirement, relPath, foundMarkers: hits.map(String), missingMarkers: markers.filter((m) => !hits.includes(m)).map(String) },
    reason: allPresent
      ? `${id}: ${note} -- all contract markers present in the real landed native source at ${relPath} (structural presence proof, not a functional installed-path proof)`
      : `${id}: ${note} -- one or more required contract markers missing from ${relPath}: ${markers.filter((m) => !hits.includes(m)).map(String).join(", ")}`,
  };
}

export function BM03(context) {
  return installedResult("BM03", () => nativeFixture({
    "src/main.rs": "fn helper() {}\nfn main() { helper(); }\n",
  }, (exe, root) => {
    const refresh = nativeCall(exe, ["refresh", "--repo-root", root], root);
    const result = nativeCall(exe, ["architecture", "--repo-root", root, "--task", "main"], root);
    const sections = Object.entries(result).filter(([key, value]) => !["schemaVersion", "generationId", "state", "task", "resolution", "freshness", "omissions"].includes(key) && value && typeof value === "object");
    if (!refresh.generationId || !result.generationId || result.generationId !== refresh.generationId) throw new Error("architecture generation is not bound to refresh");
    if (sections.length < 8) throw new Error(`architecture returned only ${sections.length} orientation sections`);
    for (const [name, section] of sections) {
      for (const field of ["disposition", "returnedCount", "totalKnownCount", "truncated", "items"]) if (!(field in section)) throw new Error(`${name} omitted ${field}`);
      if (["partial", "unavailable", "not_evaluated"].includes(section.disposition) && typeof section.reason !== "string") throw new Error(`${name} omitted reason`);
    }
    return { generationId: result.generationId, sectionCount: sections.length, dispositions: sections.map(([, value]) => value.disposition).sort() };
  }));
}

export function BM04(context) {
  return installedResult("BM04", () => nativeFixture({
    "src/main.rs": "fn dynamic() { let name = \"helper\"; println!(\"{}\", name); }\nfn main() { dynamic(); }\n",
  }, (exe, root) => {
    nativeCall(exe, ["refresh", "--repo-root", root], root);
    const result = nativeCall(exe, ["impact", "--repo-root", root, "--node", "dynamic", "--depth", "4"], root);
    const text = JSON.stringify(result);
    const classes = new Set(["known_structural_dependency", "possible_impact", "unresolved_dynamic_surface", "not_observed"].filter((value) => text.includes(value)));
    if (!result.generationId || !Array.isArray(result.impact) || !Array.isArray(result.omissions)) throw new Error("impact omitted generation, classification, or omissions");
    if (!classes.size) throw new Error("impact returned no typed frontier classification");
    return { generationId: result.generationId, classes: [...classes], impactCount: result.impact.length, omissionCount: result.omissions.length };
  }));
}

export function BM05(context) {
  return installedResult("BM05", () => nativeFixture({
    "src/main.rs": "fn main() { println!(\"phase2\"); }\n",
  }, (exe, root) => {
    const refresh = nativeCall(exe, ["refresh", "--repo-root", root], root);
    const current = nativeCall(exe, ["status", "--repo-root", root, "--generation", refresh.generationId], root);
    if (current.generationId !== refresh.generationId || current.state !== "fresh") throw new Error("generation-pinned status did not remain fresh");
    // The installed CLI fails CLOSED on a mismatched generation: it refuses with
    // a typed `generation_mismatch` error and a non-zero exit, rather than
    // returning a JSON "suppressed" body. That refusal, captured here, IS the
    // proof this case is meant to establish -- a non-typed failure (wrong exit
    // code, crash, or a marker-less message) or an unexpected success both
    // still fail this case (negative control).
    const mismatch = nativeCallExpectRefusal(exe, ["search", "--repo-root", root, "--query", "phase2", "--generation", "missing-generation"], root, "generation_mismatch");
    if (!mismatch.refused) throw new Error(`mismatched generation did not fail closed: command exited 0 with output: ${mismatch.combined.slice(0, 300)}`);
    if (!mismatch.typed) throw new Error(`mismatched generation failed (exit ${mismatch.exitCode}) but not with the typed generation_mismatch refusal: ${mismatch.combined.slice(0, 300)}`);
    return { generationId: refresh.generationId, freshState: current.state, generationMismatchRejected: true, mismatchExitCode: mismatch.exitCode };
  }));
}

// ---------------------------------------------------------------------------
// OPT-01 (windows-amendment-acceptance.json, group EX) -- gated on NCL-02/NCL-05.
// This module cannot observe another lane's live pass/fail state (no cross-lane
// file reads outside interfaceHandoffs per SUBRULES), so it never claims OPT-01
// passed; it reports the explicit gating dependency as a typed blocked result.
// ---------------------------------------------------------------------------

export function OPT_01(context) {
  return {
    status: "insufficient",
    evidenceKind: "source",
    detail: { id: "OPT-01", gatedOn: ["NCL-02", "NCL-05"] },
    reason: "OPT-01: optional indexed lexical search parity is explicitly gated on NCL-02 and NCL-05 passing first (state OPTIONAL_AFTER_PARITY); this module has no authority to observe or assert those cross-lane results and never fabricates a pass ahead of that gate",
  };
}

export const BPT_CASES = {
  BPT_001, BPT_002, BPT_003, BPT_004, BPT_005, BPT_006, BPT_007, BPT_008,
  BPT_009, BPT_010, BPT_011, BPT_012, BPT_013, BPT_014, BPT_015, BPT_016,
  BPT_017, BPT_018, BPT_019, BPT_020, BPT_021, BPT_023, BPT_024, BPT_025,
  BPT_026, BPT_027, BPT_028, BPT_029, BPT_030, BPT_031, BPT_032, BPT_033,
  BPT_034, BPT_035, BPT_036, BPT_037, BPT_038, BPT_039, BPT_040, BPT_041,
  BPT_042, BPT_043, BPT_044, BPT_046, BPT_047, BPT_049, BPT_050, BPT_051,
  BPT_052, BPT_053, BPT_054, BPT_055, BPT_056, BPT_057, BPT_058, BPT_059,
  BPT_060, BPT_061, BPT_062, BPT_063, BPT_064, BPT_065, BPT_066, BPT_067,
  BPT_068, BPT_069, BPT_070, BPT_071,
  BM03, BM04, BM05, OPT_01,
};
