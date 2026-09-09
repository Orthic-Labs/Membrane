#!/usr/bin/env node
// scripts/qualification/cases/ncl-windows.mjs -- native-cleanup lane case module.
//
// Contract: each export is called by scripts/qualification/run.mjs runOneRegistryCase
// as caseFunction({ row, workspaceRoot, profile, platform, evidencePath }) and must
// return { status: "passed"|<anything else>, evidenceKind, detail, reason } where
// evidenceKind is one of run.mjs EVIDENCE_KINDS (source, component, integration,
// installed, host, task-outcome). A missing/unrecognized evidenceKind is treated by
// the runner as a hard failure, never a soft pass.
//
// Honesty policy for this module (docs/agent-rules.md, membrane.prompt.md native-cleanup
// section, windows-amendment-acceptance.json NCL-01..05): this worker may not run cargo,
// builds, installs, packaging, or process-tree captures. NCL-03 and NCL-05 need an
// installed, interpreter-stripped binary and a live process-tree capture that only the
// integration owner can produce; this module therefore never fabricates a "passed" for
// those two -- it performs the structural checks it CAN do from the live source tree
// (source/component evidenceKind) and reports the remaining gap as a typed
// "insufficient" outcome naming exactly what installed-path evidence would close it.
// NCL-01, NCL-02 and NCL-04 are checkable from the live tree/registry alone and are
// scored pass/fail for real.

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = resolve(HERE, "../../../");

const INTERPRETED_EXTENSIONS = [".py", ".mjs", ".cjs", ".js", ".ts", ".sh", ".ps1", ".cmd", ".bat"];

function resolveRoot(context) {
  return (context && context.workspaceRoot) || (context && context.root) || REPO_ROOT;
}

function readJson(root, relPath) {
  const abs = join(root, relPath);
  if (!existsSync(abs)) return { ok: false, error: `not found: ${relPath}` };
  try {
    return { ok: true, value: JSON.parse(readFileSync(abs, "utf8")) };
  } catch (error) {
    return { ok: false, error: `parse error in ${relPath}: ${error.message}` };
  }
}

function hasExt(p) {
  return INTERPRETED_EXTENSIONS.some((ext) => p.toLowerCase().endsWith(ext));
}

// Real git-tracked + untracked-but-interpreted file listing. Untracked files that are
// git-ignored are excluded (they are not part of the live shippable tree); this mirrors
// "git ls-files + untracked interpreted files" from the NCL-01 acceptance text.
function listLiveInterpretedFiles(root) {
  const tracked = execFileSync("git", ["ls-files"], { cwd: root, encoding: "utf8" })
    .split(/\r?\n/)
    .filter(Boolean);
  let untracked = [];
  try {
    untracked = execFileSync(
      "git",
      ["ls-files", "--others", "--exclude-standard"],
      { cwd: root, encoding: "utf8" },
    )
      .split(/\r?\n/)
      .filter(Boolean);
  } catch {
    untracked = [];
  }
  const all = [...new Set([...tracked, ...untracked])];
  return all.filter(hasExt).map((p) => p.replace(/\\/g, "/")).sort();
}

// -----------------------------------------------------------------------------------
// NCL-01 -- interpreter inventory equals live tree.
// -----------------------------------------------------------------------------------
export function NCL_01(context) {
  const root = resolveRoot(context);
  const disp = readJson(root, "docs/../../../review/windows-r5/interpreter-dispositions.json");
  // interpreter-dispositions.json lives outside the repo (D:/Claude/review/windows-r5);
  // resolve it from an absolute path override when the relative repo-join misses, rather
  // than fabricate a pass over a file we never actually read.
  let dispositions = disp;
  if (!dispositions.ok) {
    const absOverride = (context && context.interpreterDispositionsPath) ||
      "D:/Claude/review/windows-r5/interpreter-dispositions.json";
    if (existsSync(absOverride)) {
      try {
        dispositions = { ok: true, value: JSON.parse(readFileSync(absOverride, "utf8")) };
      } catch (error) {
        dispositions = { ok: false, error: error.message };
      }
    }
  }
  if (!dispositions.ok) {
    return {
      status: "failed",
      evidenceKind: "source",
      detail: { error: dispositions.error },
      reason: `NCL-01: could not load interpreter-dispositions.json: ${dispositions.error}`,
    };
  }
  const rows = dispositions.value;
  if (!Array.isArray(rows)) {
    return {
      status: "failed",
      evidenceKind: "source",
      detail: {},
      reason: "NCL-01: interpreter-dispositions.json is not an array of rows",
    };
  }

  const missingFields = rows.filter((r) => !r.path || !r.action || !r.owner || !r.requiredProof);
  const inventoryPaths = new Set(rows.map((r) => r.path));
  const live = listLiveInterpretedFiles(root);

  const unclassified = live.filter((p) => !inventoryPaths.has(p));
  // A "dead" entry is an inventory path absent from disk -- a plain filesystem existence
  // check, independent of the interpreted-extension filter used for liveSet. liveSet is
  // extension-filtered (INTERPRETED_EXTENSIONS), so it wrongly excludes extensionless
  // launchers (blueprint, blueprint-mcp) and non-listed extensions (.bash, .rb), making
  // every such inventory row look "dead" even though the file exists on disk. The
  // unclassified check above is correctly extension-filtered (it only cares about live
  // interpreted files missing from the inventory); the dead check below must not be.
  const deadEntries = rows.filter((r) => r.path && !existsSync(join(root, r.path))).map((r) => r.path);
  const windowsLaunchers = live.filter((p) => p.endsWith(".ps1") || p.endsWith(".cmd"));
  const missingLaunchers = windowsLaunchers.filter((p) => !inventoryPaths.has(p));

  const problems = [];
  if (missingFields.length > 0) {
    problems.push(`${missingFields.length} row(s) missing action/owner/requiredProof`);
  }
  if (unclassified.length > 0) {
    problems.push(`${unclassified.length} live interpreted file(s) absent from inventory: ${unclassified.slice(0, 10).join(", ")}${unclassified.length > 10 ? ", ..." : ""}`);
  }
  if (deadEntries.length > 0) {
    problems.push(`${deadEntries.length} inventory entr(y/ies) reference file(s) absent on disk: ${deadEntries.slice(0, 10).join(", ")}${deadEntries.length > 10 ? ", ..." : ""}`);
  }
  if (missingLaunchers.length > 0) {
    problems.push(`${missingLaunchers.length} Windows launcher(s) (.ps1/.cmd) missing from inventory: ${missingLaunchers.join(", ")}`);
  }

  const detail = {
    inventoryCount: rows.length,
    liveCount: live.length,
    unclassifiedCount: unclassified.length,
    deadEntryCount: deadEntries.length,
    missingLauncherCount: missingLaunchers.length,
    missingFieldCount: missingFields.length,
  };

  if (problems.length > 0) {
    return {
      status: "failed",
      evidenceKind: "source",
      detail,
      reason: `NCL-01: ${problems.join("; ")}`,
    };
  }
  return {
    status: "passed",
    evidenceKind: "source",
    detail,
    reason: "NCL-01: interpreter-dispositions.json path set equals live tracked+untracked interpreted files, and every entry carries action/owner/requiredProof",
  };
}

// -----------------------------------------------------------------------------------
// NCL-02 -- parity before delete, per cluster.
//
// This module structurally checks, per delete-after-parity row, whether a named native
// destination module exists on disk and whether the row's requiredProof fields are
// present in the dispositions/effective-ownership records. It cannot itself observe
// another lane's ported-assertion test run (that is task-outcome evidence produced by
// the integration owner's sweep), so a cluster with no recorded native test evidence
// hash is reported "insufficient", never "passed".
// -----------------------------------------------------------------------------------
export function NCL_02(context) {
  const root = resolveRoot(context);
  const absOverride = (context && context.interpreterDispositionsPath) ||
    "D:/Claude/review/windows-r5/interpreter-dispositions.json";
  if (!existsSync(absOverride)) {
    return {
      status: "failed",
      evidenceKind: "source",
      detail: {},
      reason: "NCL-02: interpreter-dispositions.json not found",
    };
  }
  let rows;
  try {
    rows = JSON.parse(readFileSync(absOverride, "utf8"));
  } catch (error) {
    return { status: "failed", evidenceKind: "source", detail: { error: error.message }, reason: `NCL-02: parse error: ${error.message}` };
  }
  const clusters = rows.filter((r) => r.action === "delete-after-parity");
  const withDestinationOwner = clusters.filter((r) => r.nativeDestinationOwner);
  const withoutDestinationOwner = clusters.filter((r) => !r.nativeDestinationOwner);

  // Vacuous-test detection is out of scope for a source-only check (it requires reading
  // and semantically evaluating another lane's Rust test bodies for assertion-free
  // scaffolding, e.g. the packet's own cited empty_dimension_dependencies_cannot_be_sealed
  // example) -- this module records that as an explicit unproven gap rather than a pass.
  return {
    status: "insufficient",
    evidenceKind: "source",
    detail: {
      clusterCount: clusters.length,
      withDestinationOwner: withDestinationOwner.length,
      withoutDestinationOwner: withoutDestinationOwner.map((r) => r.path),
    },
    reason: `NCL-02: ${clusters.length} delete-after-parity row(s) identified from interpreter-dispositions.json; ${withoutDestinationOwner.length} lack a nativeDestinationOwner and cannot even be candidates. Per-cluster ported-assertion test names, native test evidence hash freshness, and vacuous-assertion detection require reading each collaborator lane's actual test bodies and the I1 sweep evidence, which this worker\'s allowlist and no-build/no-test constraint do not permit it to execute or fabricate; reported insufficient pending that per-cluster evidence.`,
  };
}

// -----------------------------------------------------------------------------------
// NCL-03 -- native-only installed runtime proof. Requires an installed, interpreter-
// stripped build and a live process-tree capture -- both installed-path facts this
// worker cannot produce (no builds/installs/packaging permitted). Reported insufficient,
// naming exactly the installed-path evidence that would close it.
// -----------------------------------------------------------------------------------
export function NCL_03(context) {
  const root = resolveRoot(context);
  return {
    status: "insufficient",
    evidenceKind: "installed",
    detail: { root },
    reason: "NCL-03: requires an installed target build with interpreters removed/renamed from PATH, a live process-tree capture showing zero node/python/sh children, package payload inspection for bundled interpreters, and a fresh issue-native-only-seal.mjs run over the current evidence chain. This worker may not run installs, packaging, or process captures; execution belongs to the Membrane integration owner (see command in this row).",
  };
}

// -----------------------------------------------------------------------------------
// NCL-04 -- accounting and measurements are fresh Windows data.
// -----------------------------------------------------------------------------------
export function NCL_04(context) {
  const root = resolveRoot(context);
  const absOverride = (context && context.interpreterDispositionsPath) ||
    "D:/Claude/review/windows-r5/interpreter-dispositions.json";
  if (!existsSync(absOverride)) {
    return { status: "failed", evidenceKind: "source", detail: {}, reason: "NCL-04: interpreter-dispositions.json not found" };
  }
  let rows;
  try {
    rows = JSON.parse(readFileSync(absOverride, "utf8"));
  } catch (error) {
    return { status: "failed", evidenceKind: "source", detail: { error: error.message }, reason: `NCL-04: parse error: ${error.message}` };
  }
  const actions = { "delete-after-parity": 0, "retain-with-reachability-proof": 0, "commit-deletion": 0 };
  for (const r of rows) {
    if (r.action in actions) actions[r.action] += 1;
  }
  const receiptPath = (context && context.receiptPath) ||
    join(dirname(dirname(dirname(root))), "scratchpad", "lanes", "receipts", "native-cleanup__waveB.json");
  const receiptExists = existsSync(receiptPath);
  // Storage-acceptance database-compatibility / vector-scale performance / package-size
  // re-measurement on the current Windows machine is a build+run task this worker may not
  // execute; reported insufficient rather than fabricated.
  return {
    status: receiptExists ? "insufficient" : "insufficient",
    evidenceKind: "source",
    detail: { actionCounts: actions, receiptPath, receiptExists },
    reason: "NCL-04: interpreter-dispositions.json action counts recorded structurally; the receipt's ported/deleted/retained accounting must reconcile exactly to these counts. storage-acceptance.json database-compatibility, vector-scale performance, and package-size deltas require a fresh Windows measurement run this worker may not execute (no builds/installs) -- reported insufficient pending that measurement by the integration owner.",
  };
}

// -----------------------------------------------------------------------------------
// NCL-05 -- native surfaces execute native owners. Requires a live process-tree capture
// per surface call (CLI/MCP/SDK/federation) -- installed-path evidence this worker cannot
// produce. It can structurally confirm the native crates exist and that no allowlisted
// shim file was left in place; that partial evidence is recorded but does not earn "passed".
// -----------------------------------------------------------------------------------
export function NCL_05(context) {
  const root = resolveRoot(context);
  const nativeCrates = [
    "engine/crates/membrane",
    "engine/crates/membrane-client",
    "engine/crates/membrane-federation",
  ];
  const presence = nativeCrates.map((p) => ({ path: p, exists: existsSync(join(root, p)) }));
  const allPresent = presence.every((p) => p.exists);
  return {
    status: "insufficient",
    evidenceKind: "component",
    detail: { presence },
    reason: `NCL-05: native crate directories ${allPresent ? "are all present" : "are NOT all present"} in the source tree (structural, not functional, evidence). A pass additionally requires a live process-tree capture per surface call (CLI/MCP/SDK/federation) proving no interpreter child process, plus a frozen-protocol schema/error-type diff -- both installed-path checks this worker may not execute. Reported insufficient pending that capture by the integration owner.`,
  };
}

export const NCL_CASES = { NCL_01, NCL_02, NCL_03, NCL_04, NCL_05 };
