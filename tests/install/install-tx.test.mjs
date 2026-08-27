// tests/install/install-tx.test.mjs — MBR-203.
//
// Verifies the transactional install contract from the JS side. The framework
// runs the membrane binary as a subprocess, hands it a scratch root and a
// target root, and asserts the on-disk receipt matches the contract:
//   * a successful plan lands outcome=committed with every stage recorded,
//   * a failing stage triggers reverse-order rollback with outcome=rolled_back,
//   * the receipt is persisted to the scratch root before commit,
//   * --dry-run runs the plan but never renames scratch to target,
//   * an empty plan is a noop that still produces a committed receipt.
//
// The binary is auto-built if missing. Tests run end-to-end against the real
// Rust CLI; they are skipped when `MEMBRANE_SKIP_BUILD=1` is set and the
// binary is not on disk.

import assert from "node:assert/strict";
import test from "node:test";
import { spawn, spawnSync, execFileSync } from "node:child_process";
import {
  mkdtempSync,
  writeFileSync,
  readFileSync,
  existsSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(HERE, "..", "..");
const ENGINE_ROOT = join(REPO_ROOT, "engine");
const DEFAULT_BIN = join(
  ENGINE_ROOT,
  "target",
  "debug",
  process.platform === "win32" ? "membrane.exe" : "membrane",
);

function resolveBinary() {
  const fromEnv = process.env.MEMBRANE_BIN;
  if (fromEnv && existsSync(fromEnv)) return fromEnv;
  if (existsSync(DEFAULT_BIN)) return DEFAULT_BIN;
  if (process.env.MEMBRANE_SKIP_BUILD === "1") return null;
  execFileSync(
    process.platform === "win32" ? "pnpm.cmd" : "pnpm",
    ["exec", "rightkit", "cargo", "build", "--manifest-path", join(ENGINE_ROOT, "Cargo.toml"), "-p", "membrane", "--bin", "membrane"],
    { cwd: REPO_ROOT, stdio: "inherit" },
  );
  if (!existsSync(DEFAULT_BIN)) {
    throw new Error(
      `membrane binary not found at ${DEFAULT_BIN} after cargo build`,
    );
  }
  return DEFAULT_BIN;
}

function freshRoot(label) {
  return mkdtempSync(join(tmpdir(), `mbr-203-${label}-`));
}

function freshAbsentRoot(label) {
  const path = freshRoot(label);
  rmSync(path, { recursive: true });
  return path;
}

function writePlanFile(scratchRoot, plan) {
  const path = join(scratchRoot, "plan.json");
  writeFileSync(path, JSON.stringify(plan, null, 2));
  return path;
}

const shellSuccess = process.platform === "win32" ? "cmd /c exit 0" : "true";
// Qualification temp roots are created under the workspace's short, stable
// path in CI. Avoid embedded Windows quote parsing because the Rust runner
// already supplies the command as one `cmd /C` argument.
const shellQuote = (value) => process.platform === "win32"
  ? String(value)
  : `'${String(value).replaceAll("'", "'\\''")}'`;

function writeActionHelper(scratchRoot) {
  const helper = join(scratchRoot, "install-action-helper.mjs");
  writeFileSync(helper, `import { appendFileSync, readFileSync } from "node:fs";
const [mode, file, value] = process.argv.slice(2);
if (mode === "append" || mode === "append-fail") {
  appendFileSync(file, value + "\\n");
  if (mode === "append-fail") process.exit(1);
} else if (mode === "assert-receipt") {
  const receipt = JSON.parse(readFileSync(file, "utf8"));
  if (receipt.stages_completed.length !== Number(value)) {
    console.error("expected " + value + " stages, got " + JSON.stringify(receipt.stages_completed));
    process.exit(1);
  }
  if (value === "1" && receipt.stages_completed[0] !== "Enumerate") {
    console.error("unexpected first stage " + JSON.stringify(receipt.stages_completed));
    process.exit(1);
  }
}
`);
  return helper;
}

function action(helper, ...args) {
  return `node ${[helper, ...args].map(shellQuote).join(" ")}`;
}

function makePlan(scratchRoot, label, steps) {
  return {
    plan_id: `mbr-203-test-${label}-${Date.now()}`,
    scratch_root: scratchRoot,
    steps,
  };
}

function runInstall(bin, { scratchRoot, targetRoot, planPath, dryRun = false }) {
  const args = [
    "install",
    "--scratch-root", scratchRoot,
    "--target-root", targetRoot,
    "--plan", planPath,
  ];
  if (dryRun) args.push("--dry-run");
  return spawnSync(bin, args, { encoding: "utf8" });
}

const BIN = resolveBinary();
const buildNeeded = BIN === null;

test("3-stage plan produces outcome=committed with 3 stages_completed", { skip: buildNeeded }, () => {
  const scratch = freshRoot("three");
  const target = freshAbsentRoot("three-target");
  try {
    const plan = makePlan(scratch, "three", [
      { stage: "Enumerate", action: shellSuccess, rollback: shellSuccess },
      { stage: "WriteManifest", action: shellSuccess, rollback: shellSuccess },
      { stage: "MintLease", action: shellSuccess, rollback: shellSuccess },
    ]);
    const planPath = writePlanFile(scratch, plan);
    const result = runInstall(BIN, { scratchRoot: scratch, targetRoot: target, planPath });
    assert.equal(result.status, 0, `install failed: ${result.stderr}`);
    const receipt = JSON.parse(result.stdout);
    assert.equal(receipt.outcome.kind, "committed");
    assert.equal(receipt.stages_completed.length, 3);
    assert.equal(receipt.stages_completed[0], "Enumerate");
    assert.equal(receipt.stages_completed[1], "WriteManifest");
    assert.equal(receipt.stages_completed[2], "MintLease");
    assert.ok(existsSync(join(target, "install-receipt.json")), "receipt must move with the scratch tree on commit");
    assert.ok(!existsSync(scratch), "scratch must be gone after commit");
  } finally {
    rmSync(scratch, { recursive: true, force: true });
    rmSync(target, { recursive: true, force: true });
  }
});

test("3-stage plan where stage 2 fails produces outcome=rolled_back and reverse-order rollback", { skip: buildNeeded }, () => {
  const scratch = freshRoot("rollback");
  const target = freshAbsentRoot("rollback-target");
  // Track which commands the test framework observes running, in order.
  const observed = [];
  const observerPath = join(scratch, "observer.log");
  const helper = writeActionHelper(scratch);
  try {
    const plan = makePlan(scratch, "rollback", [
      // Stage 1: appends to the observer log; the rollback deletes the marker.
      {
        stage: "Enumerate",
        action: action(helper, "append", observerPath, "forward-enumerate"),
        rollback: action(helper, "append", observerPath, "rollback-enumerate"),
      },
      // Stage 2: appends to the observer log; the rollback also appends so
      // we can prove reverse order. The forward action FAILS, triggering the
      // rollback chain.
      {
        stage: "WriteManifest",
        action: action(helper, "append-fail", observerPath, "forward-writemanifest"),
        rollback: action(helper, "append", observerPath, "rollback-writemanifest"),
      },
      // Stage 3 never runs because stage 2 failed.
      {
        stage: "MintLease",
        action: action(helper, "append", observerPath, "forward-mintlease"),
        rollback: action(helper, "append", observerPath, "rollback-mintlease"),
      },
    ]);
    const planPath = writePlanFile(scratch, plan);
    const result = runInstall(BIN, { scratchRoot: scratch, targetRoot: target, planPath });
    // The install command exits non-zero on rollback.
    assert.notEqual(result.status, 0, "install must fail when a stage fails");
    assert.ok(result.stderr.includes("install rolled back"), `stderr should mention rollback: ${result.stderr}`);
    // The observer log proves the rollback chain ran in reverse order:
    // forward-enumerate ran (stage 1 succeeded), forward-writemanifest ran
    // (stage 2 ran and failed), then rollback-enumerate ran (only stage 1's
    // rollback — stage 2 had no completed prior steps).
    const log = readFileSync(observerPath, "utf8");
    assert.ok(log.includes("forward-enumerate"), `forward stage 1 must run: ${log}`);
    assert.ok(log.includes("forward-writemanifest"), `forward stage 2 must run before failing: ${log}`);
    assert.ok(log.includes("rollback-enumerate"), `rollback of stage 1 must run: ${log}`);
    // Stage 3 must never run because stage 2 failed.
    assert.ok(!log.includes("forward-mintlease"), `stage 3 forward must NOT run: ${log}`);
    assert.ok(!log.includes("rollback-mintlease"), `stage 3 rollback must NOT run: ${log}`);
    // The order matters: rollback-enumerate runs AFTER forward-writemanifest.
    const forwardManifest = log.indexOf("forward-writemanifest");
    const rollbackEnumerate = log.indexOf("rollback-enumerate");
    assert.ok(forwardManifest < rollbackEnumerate, `rollback must run after the failing forward action: ${log}`);
    // The on-disk receipt records the rolled-back outcome.
    const onDisk = JSON.parse(readFileSync(join(scratch, "install-receipt.json"), "utf8"));
    assert.match(onDisk.outcome.kind, /rolled_back/);
    assert.equal(onDisk.stages_completed.length, 1);
    assert.equal(onDisk.stages_completed[0], "Enumerate");
    assert.ok(onDisk.rollback_actions.length >= 1, "rollback_actions must record what ran");
    // The target root must NOT exist — commit was never reached.
    assert.ok(!existsSync(target), "target root must NOT exist after rollback");
  } finally {
    rmSync(scratch, { recursive: true, force: true });
    rmSync(target, { recursive: true, force: true });
  }
});

test("receipt file exists on disk after stage 1 before stage 2", { skip: buildNeeded }, () => {
  const scratch = freshRoot("persistence");
  const target = freshAbsentRoot("persistence-target");
  const helper = writeActionHelper(scratch);
  try {
    // Stage 1's forward action verifies the receipt is already on disk
    // with stages_completed count = 0 (the framework persists the initial
    // receipt BEFORE the first stage runs). Stage 2's forward action
    // verifies the receipt is already on disk with stages_completed
    // count = 1 (the framework persists after every stage). Both
    // assertions use the Node helper so same plan runs on Windows & POSIX.
    const plan = makePlan(scratch, "persistence", [
      {
        stage: "Enumerate",
        action: action(helper, "assert-receipt", join(scratch, "install-receipt.json"), "0"),
        rollback: shellSuccess,
      },
      {
        stage: "WriteManifest",
        action: action(helper, "assert-receipt", join(scratch, "install-receipt.json"), "1"),
        rollback: shellSuccess,
      },
      { stage: "MintLease", action: shellSuccess, rollback: shellSuccess },
    ]);
    const planPath = writePlanFile(scratch, plan);
    const result = runInstall(BIN, { scratchRoot: scratch, targetRoot: target, planPath });
    // If either grep assertion inside stage 1 or stage 2 fails, the
    // install rolls back and the binary exits non-zero with a clear
    // reason.
    assert.equal(result.status, 0, `install failed: ${result.stderr}`);
    const receipt = JSON.parse(result.stdout);
    assert.equal(receipt.outcome.kind, "committed");
    // Post-commit, the receipt under the target carries the same shape
    // — the on-disk receipt moved with the scratch tree.
    const onDisk = JSON.parse(readFileSync(join(target, "install-receipt.json"), "utf8"));
    assert.equal(onDisk.stages_completed.length, 3);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
    rmSync(target, { recursive: true, force: true });
  }
});

test("--dry-run never renames scratch to target", { skip: buildNeeded }, () => {
  const scratch = freshRoot("dryrun");
  const target = freshAbsentRoot("dryrun-target");
  try {
    const plan = makePlan(scratch, "dryrun", [
      { stage: "Enumerate", action: shellSuccess, rollback: shellSuccess },
      { stage: "WriteManifest", action: shellSuccess, rollback: shellSuccess },
    ]);
    const planPath = writePlanFile(scratch, plan);
    const result = runInstall(BIN, {
      scratchRoot: scratch,
      targetRoot: target,
      planPath,
      dryRun: true,
    });
    assert.equal(result.status, 0, `install --dry-run failed: ${result.stderr}`);
    assert.ok(existsSync(scratch), "scratch root must survive --dry-run");
    assert.ok(!existsSync(target), "target root must NOT exist after --dry-run");
    const receipt = JSON.parse(result.stdout);
    // Dry-run stops at Pending — commit was never called.
    assert.match(receipt.outcome.kind, /pending/);
    // The receipt on disk under the scratch root records the same state.
    const onDisk = JSON.parse(readFileSync(join(scratch, "install-receipt.json"), "utf8"));
    assert.match(onDisk.outcome.kind, /pending/);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
    rmSync(target, { recursive: true, force: true });
  }
});

test("empty plan is noop (outcome=committed, 0 stages)", { skip: buildNeeded }, () => {
  const scratch = freshRoot("empty");
  const target = freshAbsentRoot("empty-target");
  try {
    const plan = makePlan(scratch, "empty", []);
    const planPath = writePlanFile(scratch, plan);
    const result = runInstall(BIN, { scratchRoot: scratch, targetRoot: target, planPath });
    assert.equal(result.status, 0, `empty install failed: ${result.stderr}`);
    const receipt = JSON.parse(result.stdout);
    assert.equal(receipt.outcome.kind, "committed");
    assert.equal(receipt.stages_completed.length, 0);
    assert.equal(receipt.schema_version, 1);
    // The receipt moved to the target root on commit.
    assert.ok(existsSync(join(target, "install-receipt.json")), "receipt must land under target on commit");
    assert.ok(!existsSync(scratch), "scratch must be gone after commit");
  } finally {
    rmSync(scratch, { recursive: true, force: true });
    rmSync(target, { recursive: true, force: true });
  }
});
