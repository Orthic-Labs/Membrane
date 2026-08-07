// tests/install/uninstall-rust.test.mjs — MBR-205.
//
// End-to-end check that the `membrane uninstall` binary:
//   * refuses every candidate when the ownership table is missing,
//   * echoes the authorised set on --dry-run without removing anything,
//   * refuses unowned paths in --dry-run output,
//   * persists an UninstallReceiptV1 next to the ownership table.
//
// The binary is auto-built if missing. The test is skipped when
// `MEMBRANE_SKIP_BUILD=1` is set and the binary is not on disk.

import assert from "node:assert/strict";
import test from "node:test";
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
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
const DEFAULT_BIN = join(ENGINE_ROOT, "target/debug/membrane");

function resolveBinary() {
  const fromEnv = process.env.MEMBRANE_BIN;
  if (fromEnv && existsSync(fromEnv)) return fromEnv;
  if (existsSync(DEFAULT_BIN)) return DEFAULT_BIN;
  if (process.env.MEMBRANE_SKIP_BUILD === "1") return null;
  execFileSync(
    "cargo",
    ["build", "-p", "membrane", "--bin", "membrane"],
    { cwd: ENGINE_ROOT, stdio: "inherit" },
  );
  if (!existsSync(DEFAULT_BIN)) {
    throw new Error(
      `membrane binary not found at ${DEFAULT_BIN} after cargo build`,
    );
  }
  return DEFAULT_BIN;
}

function freshRoot(label) {
  return mkdtempSync(join(tmpdir(), `mbr-205-rust-${label}-`));
}

function writeOwnershipTable(root, table) {
  writeFileSync(
    join(root, "ownership.json"),
    JSON.stringify(table, null, 2),
    "utf8",
  );
}

function makeTable(receiptRoot, claims) {
  const digest = createHash("sha256")
    .update(receiptRoot)
    .digest("hex");
  return {
    installationId: `sha256:${digest}`,
    claims,
  };
}

function runUninstall(bin, { receiptRoot, candidates, dryRun = false }) {
  const args = ["uninstall", "--receipt-root", receiptRoot];
  for (const candidate of candidates) {
    args.push("--candidate", candidate);
  }
  if (dryRun) args.push("--dry-run");
  return spawnSync(bin, args, { encoding: "utf8" });
}

const BIN = resolveBinary();
const buildNeeded = BIN === null;

test("uninstall --dry-run prints authorised set and refuses unowned paths", { skip: buildNeeded }, () => {
  const root = freshRoot("dryrun");
  try {
    const ownedManifest = join(root, "manifest.json");
    const ownedLease = join(root, "lease.json");
    const stray = join(root, "stray.txt");
    writeFileSync(ownedManifest, "{}\n", "utf8");
    writeFileSync(ownedLease, "{}\n", "utf8");
    writeFileSync(stray, "user data\n", "utf8");
    writeOwnershipTable(
      root,
      makeTable(root, [
        {
          kind: "Manifest",
          path: ownedManifest,
          receiptId: "plan-1",
          registeredAtUnixMs: 1_755_000_000_000,
        },
        {
          kind: "Lease",
          path: ownedLease,
          receiptId: "sup-1",
          registeredAtUnixMs: 1_755_000_000_000,
        },
      ]),
    );

    const result = runUninstall(BIN, {
      receiptRoot: root,
      candidates: [ownedManifest, ownedLease, stray],
      dryRun: true,
    });
    assert.equal(result.status, 0, `uninstall --dry-run failed: ${result.stderr}`);
    const payload = JSON.parse(result.stdout);
    assert.equal(payload.mode, "uninstall");
    assert.equal(payload.dry_run, true);
    assert.deepEqual(
      payload.authorised.map(String).sort(),
      [ownedManifest, ownedLease].sort(),
    );
    // The refused list contains the stray path; the on-disk stray file
    // is untouched (--dry-run never removes anything).
    const refusedPaths = payload.refused.map(String);
    assert.ok(
      refusedPaths.includes(stray),
      `expected stray path in refused list, got ${refusedPaths}`,
    );
    assert.ok(existsSync(stray), "stray file must survive --dry-run");
    assert.ok(existsSync(ownedManifest), "manifest must survive --dry-run");
    assert.ok(existsSync(ownedLease), "lease must survive --dry-run");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("uninstall refuses every candidate when the ownership table is missing", { skip: buildNeeded }, () => {
  const root = freshRoot("empty");
  try {
    const candidate = join(root, "manifest.json");
    writeFileSync(candidate, "{}\n", "utf8");
    // No ownership.json on disk — the binary must treat this as empty.
    const result = runUninstall(BIN, {
      receiptRoot: root,
      candidates: [candidate],
      dryRun: true,
    });
    assert.equal(result.status, 0, `uninstall --dry-run failed: ${result.stderr}`);
    const payload = JSON.parse(result.stdout);
    assert.deepEqual(payload.authorised, []);
    assert.deepEqual(payload.refused.map(String), [candidate]);
    // The candidate is still on disk.
    assert.ok(existsSync(candidate), "missing table must not delete anything");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("uninstall removes an authorised path and persists UninstallReceiptV1", { skip: buildNeeded }, () => {
  const root = freshRoot("remove");
  try {
    const owned = join(root, "manifest.json");
    writeFileSync(owned, "{}\n", "utf8");
    writeOwnershipTable(
      root,
      makeTable(root, [
        {
          kind: "Manifest",
          path: owned,
          receiptId: "plan-1",
          registeredAtUnixMs: 1_755_000_000_000,
        },
      ]),
    );

    const result = runUninstall(BIN, {
      receiptRoot: root,
      candidates: [owned],
    });
    assert.equal(result.status, 0, `uninstall failed: ${result.stderr}`);
    const receipt = JSON.parse(result.stdout);
    assert.equal(receipt.schemaVersion, 1);
    assert.equal(receipt.outcome, "completed");
    assert.deepEqual(receipt.removed.map(String), [owned]);
    assert.ok(!existsSync(owned), "owned path must be gone after uninstall");
    const onDisk = JSON.parse(
      readFileSync(join(root, "uninstall-receipt.json"), "utf8"),
    );
    assert.equal(onDisk.outcome, "completed");
    assert.deepEqual(onDisk.removed.map(String), [owned]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("uninstall refuses unowned paths and leaves them on disk", { skip: buildNeeded }, () => {
  const root = freshRoot("unowned");
  try {
    const owned = join(root, "manifest.json");
    const stray = join(root, "stray.txt");
    writeFileSync(owned, "{}\n", "utf8");
    writeFileSync(stray, "user data\n", "utf8");
    writeOwnershipTable(
      root,
      makeTable(root, [
        {
          kind: "Manifest",
          path: owned,
          receiptId: "plan-1",
          registeredAtUnixMs: 1_755_000_000_000,
        },
      ]),
    );

    const result = runUninstall(BIN, {
      receiptRoot: root,
      candidates: [owned, stray],
    });
    assert.equal(result.status, 0, `uninstall failed: ${result.stderr}`);
    const receipt = JSON.parse(result.stdout);
    assert.equal(receipt.outcome, "completed");
    assert.deepEqual(receipt.removed.map(String), [owned]);
    assert.deepEqual(receipt.refused.map(String), [stray]);
    assert.ok(!existsSync(owned), "owned path must be gone");
    assert.ok(existsSync(stray), "unowned path must survive uninstall");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
