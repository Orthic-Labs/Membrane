#!/usr/bin/env node
// scripts/qualification/cases/mem-lifecycle-windows.mjs — lifecycle-controller lane case module.
//
// Wave-A scope note (see receipt at
// <scratchpad>/lanes/receipts/lifecycle-controller__11.json): this module is
// authored edit-only, against the live repository tree, with no build and
// no runner execution. It provides:
//
//   - MEM_006..MEM_066 (the lifecycle-controller-owned windows-acceptance.json
//     rows): structural/contract attestations. Each checks that the file(s)
//     named by that row's canonicalImplementationRow (or, where the row
//     cites only historical/legacy prose, the current owning source file)
//     exist and contain at least one expected contract marker. These are
//     NOT functional/installed proofs; each result carries
//     `kind: "structural"` so the registry never confuses this with an
//     installed pass. The integration owner's installed-path run supplies
//     the functional evidence.
//   - LC_01..LC_06 (windows-amendment-acceptance.json): structural
//     attestations over the source files that implement each residency/
//     lease/discovery/activation clause described in this lane's
//     `laneTask`. Every `negativeControl` string listed on each LC row in
//     the registry has a matching executable negative-control test in
//     mem-lifecycle-windows.test.mjs that injects the described fault into
//     an isolated fixture root and asserts the check fails for that
//     specific reason.
//
// Every exported function accepts an optional `{ root }` so tests can point
// checks at a fixture tree instead of the live repository, exactly as in
// scripts/qualification/cases/pul-windows.mjs (the established pattern for
// this registry).

import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = resolve(HERE, "../../../");

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

// Installed lifecycle observations are supplied by membrane_lifecycle/install-release.ps1.
// This module validates their provenance and shape; it never turns source markers into
// an installed pass. The receipt is intentionally shared so runner and lifecycle lanes
// consume one unambiguous observation contract.
export function readLifecycleObservation(options = {}) {
  const path = options.lifecycleObservationPath || process.env.MEMBRANE_LIFECYCLE_OBSERVATION;
  if (!path || !existsSync(path)) return { ok: false, reason: "missing membrane.windows-lifecycle-observation.v1 receipt" };
  try {
    const value = JSON.parse(readFileSync(path, "utf8"));
    const scenarios = Array.isArray(value.scenarios) ? value.scenarios : [];
    const identity = value.buildIdentity;
    // Receipt-level shape/provenance only. This intentionally does NOT require
    // every scenario to be "passed": a scenario legitimately reports
    // "insufficient" (no installed command exists / unsafe against the shared
    // installed daemon) or "failed" (a real command ran and the observed
    // outcome did not match). Per-lane pass/fail is derived below, scoped to
    // that lane's own scenarios, so one lane's shortfall never contaminates
    // another lane's verdict.
    const shapeValid = value.schema === "membrane.windows-lifecycle-observation.v1" && value.platform === "windows" &&
      value.installed === true && value.generatedAt && Array.isArray(value.processTree) && scenarios.length > 0 &&
      identity?.root && identity?.generation && /^[0-9a-f]{64}$/i.test(String(identity.membraneSha256 || "")) &&
      scenarios.every((s) => typeof s.id === "string" && typeof s.lane === "string" &&
        ["passed", "failed", "insufficient"].includes(s.status) &&
        typeof s.reason === "string" && s.reason.length > 0 &&
        Array.isArray(s.actions) &&
        Array.isArray(s.processTreeBefore) && Array.isArray(s.processTreeDuring) && Array.isArray(s.processTreeAfter) &&
        s.actions.every((a) => typeof a.command === "string" && typeof a.exitCode === "number" &&
          typeof a.stdout === "string" && typeof a.stderr === "string" && typeof a.durationMs === "number") &&
        // A "passed" scenario must have produced at least one real, observed
        // action (non-empty stdout or stderr) proving genuine execution took
        // place. exitCode === 0 is NOT required here: several scenarios'
        // correct/expected outcome is a specific non-zero rejection exit
        // (e.g. provision-missing, reject-development-checkout expect the
        // installed CLI to reject with a non-zero code; health-probe expects
        // a typed unavailability exit). Each scenario's own validator in
        // observe-windows-lifecycle.ps1 already checks the exact exit-code
        // condition that scenario's pass means; this shape check only proves
        // the action is real, not fabricated.
        (s.status !== "passed" || (s.actions.length > 0 && s.actions.every((a) => a.stdout.length > 0 || a.stderr.length > 0))));
    return shapeValid ? { ok: true, value } : { ok: false, reason: "invalid lifecycle observation shape or provenance" };
  } catch (error) { return { ok: false, reason: `cannot read lifecycle observation: ${error.message}` }; }
}

const LC_EXPECTED_SCENARIOS = {
  "LC-01": ["hub-only", "coderight-only", "both", "holder-crash", "holder-exit", "final-holder-shutdown", "concurrent-acquire-renew-release", "drain-acquire-race", "restart-during-acquire", "stale-fencing", "survivor-continuity"],
  "LC-02": ["idle-refresh", "mid-build-refresh", "watcher-disabled-refresh", "hub-off-refresh"],
  "LC-03": ["fair-service", "deadline-cancellation", "scope-isolation", "deduplicated-work"],
  "LC-04": ["hub-off-explicit", "hub-background", "coderight-adopt", "provision-missing", "reject-corrupt", "reject-denied", "reject-unverifiable", "reject-development-checkout"],
  "LC-05": ["credential-race", "lease-incarnation", "tombstone", "reordered-response", "lost-response", "clock-rewind", "replay-bound"],
  "LC-06": ["canonical-roots", "health-probe", "startup-lock", "atomic-promotion", "hook-containment"],
};

function lifecycleRuntimeCheck(id, options, structural) {
  if (!options?.row) return structural;
  const observation = readLifecycleObservation(options);
  if (!observation.ok) return { id, kind: "installed", evidenceKind: "installed", status: "insufficient", pass: false, reason: `${id}: ${observation.reason}`, evidence: [] };
  const expected = LC_EXPECTED_SCENARIOS[id] || [];
  const laneScenarios = observation.value.scenarios.filter((s) => s.lane === id);
  const byId = new Map(laneScenarios.map((s) => [s.id, s]));
  // Scoped to this lane's own exercised scenarios only. observation.value.processTree
  // is a whole-machine snapshot at the moment the observer ran, not descendants of any
  // membrane action; it always contains the operator's own dev-machine node.exe/bash.exe
  // (this very qualification runner included), so folding it into a per-lane forbidden-
  // interpreter-child gate would make every LC row structurally unpassable on any real
  // developer or CI box regardless of what membrane.exe actually spawned.
  const forbiddenPattern = /^(node|python|python3|sh|bash)(\.exe)?$/i;
  const forbidden = laneScenarios
    .flatMap((s) => (s.processTreeDuring || []).concat(s.processTreeBefore || [], s.processTreeAfter || []))
    .filter((p) => forbiddenPattern.test(String(p?.name || "")));
  const missing = expected.filter((name) => !byId.has(name));
  const notPassed = expected.filter((name) => byId.has(name) && byId.get(name).status !== "passed");
  const pass = missing.length === 0 && notPassed.length === 0 && forbidden.length === 0;
  const detail = [];
  if (missing.length) detail.push(`missing scenarios: ${missing.join(", ")}`);
  if (notPassed.length) detail.push(`not passed: ${notPassed.map((name) => `${name} (${byId.get(name).status}: ${byId.get(name).reason})`).join("; ")}`);
  if (forbidden.length) detail.push("interpreter child observed");
  return {
    id, kind: "installed", evidenceKind: "installed",
    status: pass ? "passed" : "insufficient", pass,
    reason: pass ? `${id}: installed lifecycle scenarios passed with no interpreter children` : `${id}: ${detail.join("; ")}`,
    evidence: expected.map((name) => byId.get(name)).filter(Boolean),
  };
}

// Structural attestation: at least one of the given files exists and
// carries at least one of the given markers. Never claims functional
// correctness — only that the named implementation artifact is present and
// carries the expected contract surface. Mirrors
// scripts/qualification/cases/pul-windows.mjs::structuralCheck exactly.
function structuralCheck(id, options, relPaths, markers, note) {
  const root = resolveRoot(options);
  const files = Array.isArray(relPaths) ? relPaths : [relPaths];
  const missing = files.filter((p) => !readFileSafe(root, p));
  if (missing.length === files.length) {
    return {
      id,
      kind: "structural",
      evidenceKind: "source",
      status: "failed",
      pass: false,
      reason: `none of the canonical implementation files exist: ${files.join(", ")}`,
      evidence: files,
      note,
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
  const pass = hits.length > 0;
  return {
    id,
    kind: "structural",
    evidenceKind: "source",
    status: pass ? "passed" : "failed",
    pass,
    reason: pass
      ? `found ${hits.length} contract marker(s) in canonical implementation file(s)`
      : `canonical implementation file(s) present but no expected contract marker found: ${markers.map(String).join(", ")}`,
    evidence: hits.length > 0 ? hits : files,
    note,
  };
}

// A structural exclusion attestation: fails when a forbidden marker IS
// present in the named file(s). Used for LC negative-control clauses that
// describe a forbidden action rather than a required contract surface
// (e.g. "peer-held controller drained by non-final holder", "development
// checkout used").
function exclusionCheck(id, options, relPaths, forbiddenMarkers, note) {
  const root = resolveRoot(options);
  const files = Array.isArray(relPaths) ? relPaths : [relPaths];
  const hits = [];
  for (const relPath of files) {
    const content = readFileSafe(root, relPath);
    if (!content) continue;
    for (const marker of forbiddenMarkers) {
      const re = marker instanceof RegExp ? marker : new RegExp(marker.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
      if (re.test(content)) hits.push({ file: relPath, marker: String(marker) });
    }
  }
  const pass = hits.length === 0;
  return {
    id,
    kind: "exclusion",
    evidenceKind: "source",
    status: pass ? "passed" : "failed",
    pass,
    reason: pass
      ? `no forbidden pattern found across ${files.length} file(s)`
      : "forbidden pattern present",
    evidence: hits,
    scannedFiles: files.length,
    note,
  };
}

// ---------------------------------------------------------------------------
// MEM-006 .. MEM-066 — structural attestations (windows-acceptance.json).
// ---------------------------------------------------------------------------

export function MEM_006(options) {
  return structuralCheck("MEM-006", options,
    ["apps/membrane-tray-windows/src/supervisor.rs", "apps/membrane-tray-windows/src/process.rs"],
    [/spawn/i, /handshake/i], "tray starts exactly one headless daemon child and completes authenticated startup handshake");
}
export function MEM_008(options) {
  return structuralCheck("MEM-008", options, "engine/crates/membrane-runtime/src/service.rs",
    [/explicit/i, /resident/i, /hub/i], "explicit operations callable with Hub off; typed hub_inactive only for automatic/resident control");
}
export function MEM_009(options) {
  return structuralCheck("MEM-009", options,
    ["engine/crates/membrane/src/cli.rs", "engine/crates/membrane-runtime/src/cli.rs", "engine/crates/membrane-runtime/src/serve.rs"],
    [/run_cli/i, /membrane_runtime/i], "bounded explicit CLI/MCP entry points using canonical owners without auto-starting residency");
}
export function MEM_010(options) {
  return structuralCheck("MEM-010", options, "engine/crates/membrane-runtime/src/serve.rs",
    [/loopback/i, /authenticat/i], "authenticated bounded loopback HTTP runtime APIs");
}
export function MEM_012(options) {
  return structuralCheck("MEM-012", options, "engine/crates/membrane-runtime/src/mcp_http.rs",
    [/origin/i, /token/i], "authenticated Streamable-HTTP MCP rejects unsafe origin/host/token requests");
}
export function MEM_016(options) {
  return structuralCheck("MEM-016", options,
    ["apps/membrane-hub/src-tauri/src/dashboard_connection.rs", "engine/crates/membrane-runtime/src/service.rs"],
    [/capability/i, /snapshot|generation/i], "tray-owned daemon readiness/health/drain/generation reporting with hub_inactive kept only as wire compatibility");
}
export function MEM_018(options) {
  return structuralCheck("MEM-018", options, "engine/crates/membrane-runtime/src/installation_manifest.rs",
    [/manifest/i, /schema/i], "build/publish/verify installation manifest across components and schemas");
}
export function MEM_019(options) {
  return structuralCheck("MEM-019", options, "engine/crates/membrane-runtime/src/installation_manifest.rs",
    [/identity|lineage|fingerprint/i], "detect cloned installation, quarantine collision, rotate opaque identity");
}
export function MEM_020(options) {
  return structuralCheck("MEM-020", options,
    ["engine/crates/membrane-runtime/src/installation_manifest.rs", "engine/crates/membrane/src/install_tx.rs"],
    [/root|path/i], "isolate installed state from checkout/dev roots, ports, data, startup, PATH, and client projections");
}
export function MEM_021(options) {
  return structuralCheck("MEM-021", options, "engine/crates/membrane/src/update.rs",
    [/stage|candidate/i, /verify|signature/i], "transactionally stage an update candidate and verify cryptographic admission before activation");
}
export function MEM_054(options) {
  return structuralCheck("MEM-054", options, "apps/membrane-hub/src-tauri/src/dashboard_connection.rs",
    [/capability/i, /version|handshake/i], "handshake CodeRight against protocol/build/capability identities with typed incompatibility");
}
export function MEM_055(options) {
  return structuralCheck("MEM-055", options, "apps/membrane-tray-windows/src/supervisor.rs",
    [/restart/i, /supervis/i], "supervise one tray-owned daemon child and restart it under bounded lifecycle policy");
}
export function MEM_056(options) {
  return structuralCheck("MEM-056", options, "apps/membrane-tray-windows/src/process.rs",
    [/drain|terminate|kill/i], "drain and terminate tray-owned daemon child without leaving resident runtime");
}
export function MEM_057(options) {
  return structuralCheck("MEM-057", options, "engine/crates/membrane/src/update.rs",
    [/atomic|activat/i], "atomically activate one admitted update candidate");
}
export function MEM_058(options) {
  return structuralCheck("MEM-058", options, "engine/crates/membrane/src/update.rs",
    [/rollback|previous/i], "retain previous installed version and roll back failed activation transactionally");
}
export function MEM_059(options) {
  return structuralCheck("MEM-059", options, "engine/crates/membrane/src/uninstall.rs",
    [/uninstall/i], "uninstall Membrane runtime, tray and bindings through governed product path");
}
export function MEM_060(options) {
  return structuralCheck("MEM-060", options,
    ["engine/crates/membrane-runtime/src/authorization.rs", "engine/crates/membrane/src/update.rs"],
    [/downgrade/i], "reject version downgrade unless exact candidate has explicit downgrade authorization");
}
export function MEM_061(options) {
  return structuralCheck("MEM-061", options, "engine/crates/membrane-runtime/src/background_review.rs",
    [/single.?flight|admission/i], "schedule generic daemon jobs with single-flight coordination under configured admission control");
}
export function MEM_062(options) {
  return structuralCheck("MEM-062", options, "engine/crates/membrane-runtime/src/background_review.rs",
    [/cancel/i], "cancel daemon jobs on request cancellation or daemon shutdown");
}
export function MEM_063(options) {
  return structuralCheck("MEM-063", options, "engine/crates/membrane-runtime/src/background_review.rs",
    [/retry/i, /terminal/i], "bound daemon job retries with typed terminal outcomes");
}
export function MEM_064(options) {
  return structuralCheck("MEM-064", options, "engine/crates/membrane-runtime/src/background_review.rs",
    [/idle/i, /budget/i], "enforce idle and activity budgets for daemon jobs");
}
export function MEM_065(options) {
  return structuralCheck("MEM-065", options, "engine/crates/membrane-runtime/src/background_review.rs",
    [/token.?budget/i], "enforce daemon job token budgets");
}
export function MEM_066(options) {
  return structuralCheck("MEM-066", options, "engine/crates/membrane-runtime/src/background_review.rs",
    [/H5/, /observation/i], "emit H5 daemon scheduling observations");
}

// ---------------------------------------------------------------------------
// LC-01 .. LC-06 — structural attestations (windows-amendment-acceptance.json).
// ---------------------------------------------------------------------------

export function LC_01(options) {
  return lifecycleRuntimeCheck("LC-01", options, structuralCheck("LC-01", options,
    ["engine/crates/membrane-runtime/src/residency.rs", "engine/crates/membrane-client/src/residency.rs", "engine/crates/membrane-runtime/tests/residency_holders.rs"],
    [/holder/i, /final.?holder|drain/i], "residency matrix: Hub-only/CodeRight-only/both/crash/exit/final-holder drain/concurrent acquire-renew-release/stale fencing"));
}
export function LC_01_NC_peer_drain(options) {
  return exclusionCheck("LC-01-NC-peer-drain", options,
    ["engine/crates/membrane-runtime/src/residency.rs"],
    [/fn\s+drain\w*\s*\([^)]*\)\s*\{\s*\/\/\s*unconditional/i, /force_drain_any_holder/i],
    "peer-held controller drained by non-final holder must fail");
}
export function LC_01_NC_stale_mutate(options) {
  return exclusionCheck("LC-01-NC-stale-mutate", options,
    ["engine/crates/membrane-runtime/src/residency.rs"],
    [/allow_stale_mutation/i, /skip_fencing/i],
    "stale holder able to mutate must fail");
}

export function LC_02(options) {
  return lifecycleRuntimeCheck("LC-02", options, structuralCheck("LC-02", options,
    ["engine/crates/membrane/src/cli.rs", "engine/crates/membrane-runtime/src/cli.rs"],
    [/refresh/i, /blueprint|Blueprint/], "manual Blueprint refresh at any time from cli.rs entry point, publishing queryable generation freshness"));
}
export function LC_02_NC_enqueue_freshness(options) {
  return exclusionCheck("LC-02-NC-enqueue-freshness", options,
    ["engine/crates/membrane-runtime/src/cli.rs"],
    [/freshness\s*=\s*enqueue(d)?_at/i],
    "refresh reporting freshness from enqueue rather than publication must fail");
}
export function LC_02_NC_hub_blocked(options) {
  return exclusionCheck("LC-02-NC-hub-blocked", options,
    ["engine/crates/membrane-runtime/src/cli.rs"],
    [/refresh.*require.*hub|hub_required_for_refresh/i],
    "refresh blocked by absent Hub must fail");
}

export function LC_03(options) {
  return lifecycleRuntimeCheck("LC-03", options, structuralCheck("LC-03", options,
    ["engine/crates/membrane-runtime/tests/residency_holders.rs", "engine/crates/membrane-federation/src/request.rs"],
    [/scope/i, /deadline/i], "multi-client fairness/isolation: bounded fair service, own-deadline cancellation, scope isolation, dedup"));
}
export function LC_03_NC_cross_scope(options) {
  return exclusionCheck("LC-03-NC-cross-scope", options,
    ["engine/crates/membrane-federation/src/request.rs"],
    [/share_evidence_across_scope/i, /ignore_scope_boundary/i],
    "cross-scope evidence leak must fail");
}
export function LC_03_NC_starvation(options) {
  return exclusionCheck("LC-03-NC-starvation", options,
    ["engine/crates/membrane-runtime/tests/residency_holders.rs"],
    [/unbounded_wait/i, /no_fairness_bound/i],
    "starvation beyond declared bound must fail");
}

export function LC_04(options) {
  return lifecycleRuntimeCheck("LC-04", options, structuralCheck("LC-04", options,
    ["engine/crates/membrane/src/activation.rs", "engine/crates/membrane-runtime/src/installation_manifest.rs"],
    [/canonical|installed/i, /provision|install/i], "installed provisioning and independence: Hub-off explicit operations, Hub background residency, CodeRight installer-owned adoption"));
}
export function LC_04_NC_dev_checkout(options) {
  return exclusionCheck("LC-04-NC-dev-checkout", options,
    ["engine/crates/membrane/src/activation.rs"],
    [/discover.*PATH.*checkout/i, /use_development_checkout/i],
    "development checkout present on PATH must never be used");
}
export function LC_04_NC_incompatible_execute(options) {
  return exclusionCheck("LC-04-NC-incompatible-execute", options,
    ["engine/crates/membrane/src/activation.rs"],
    [/execute_incompatible_installed/i],
    "incompatible installed version must be updated through installer, not executed");
}

export function LC_05(options) {
  return lifecycleRuntimeCheck("LC-05", options, structuralCheck("LC-05", options,
    ["engine/crates/membrane-protocol/src/operations.rs", "engine/crates/membrane-client/src/residency.rs"],
    [/incarnation/i, /LeaseHandleV2|lease/i], "credential and lease v2 correctness: incarnation/sequence/idempotency, tombstones, reordered/lost-response recovery, replay admission bounds"));
}
export function LC_05_NC_replay_after_tombstone(options) {
  return exclusionCheck("LC-05-NC-replay-after-tombstone", options,
    ["engine/crates/membrane-protocol/src/operations.rs"],
    [/resurrect_tombstoned_lease/i, /revive_closed_incarnation/i],
    "replayed acquire after tombstone must fail");
}
export function LC_05_NC_clock_rewind(options) {
  return exclusionCheck("LC-05-NC-clock-rewind", options,
    ["engine/crates/membrane-protocol/src/operations.rs"],
    [/allow_wall_time_rewind/i, /accept_decreasing_authoritative_time/i],
    "clock rewind advancing lease must fail");
}

export function LC_06(options) {
  return lifecycleRuntimeCheck("LC-06", options, structuralCheck("LC-06", options,
    ["engine/crates/membrane/src/activation.rs", "engine/crates/membrane/tests/hook_containment.rs"],
    [/health|startup.?lock|atomic.?promot/i, /hook|enroll/i], "installer activation: canonical roots, health probe, startup lock, atomic promotion, host hook enrollment"));
}
export function LC_06_NC_enroll_outside_roots(options) {
  return exclusionCheck("LC-06-NC-enroll-outside-roots", options,
    ["engine/crates/membrane/src/activation.rs"],
    [/write_hook_outside_installed_root/i, /enroll_arbitrary_path/i],
    "enrollment writing outside installed roots must fail");
}
export function LC_06_NC_hook_interpreter(options) {
  return exclusionCheck("LC-06-NC-hook-interpreter", options,
    ["engine/crates/membrane/tests/hook_containment.rs"],
    [/Command::new\(\s*"(python|node|bash|sh)"\s*\)/i],
    "a hook that executes an interpreter must fail");
}

export const LIFECYCLE_CASES = {
  MEM_006, MEM_008, MEM_009, MEM_010, MEM_012, MEM_016, MEM_018, MEM_019, MEM_020, MEM_021,
  MEM_054, MEM_055, MEM_056, MEM_057, MEM_058, MEM_059, MEM_060, MEM_061, MEM_062, MEM_063,
  MEM_064, MEM_065, MEM_066,
  LC_01, LC_01_NC_peer_drain, LC_01_NC_stale_mutate,
  LC_02, LC_02_NC_enqueue_freshness, LC_02_NC_hub_blocked,
  LC_03, LC_03_NC_cross_scope, LC_03_NC_starvation,
  LC_04, LC_04_NC_dev_checkout, LC_04_NC_incompatible_execute,
  LC_05, LC_05_NC_replay_after_tombstone, LC_05_NC_clock_rewind,
  LC_06, LC_06_NC_enroll_outside_roots, LC_06_NC_hook_interpreter,
};

export default LIFECYCLE_CASES;
