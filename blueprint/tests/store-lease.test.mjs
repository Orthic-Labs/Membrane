// BlueprintStoreLeaseV1 — store write-ownership contract (§17.2.1).
//
// The properties under test map directly to the canon requirements:
//   - OS-level lock is the PRIMARY exclusion mechanism (not lease metadata).
//   - A crashed owner's lock is released by the OS with no cooperation from
//     the dead process; stale metadata must never permanently block a new
//     owner.
//   - PID alone is insufficient (reused); process_start_identity disambiguates.
//   - A contending writer NEVER becomes a second writer: it fails typed
//     resident_owner_active.

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import test from "node:test";
import {
  acquireStoreLease,
  currentProcessStartIdentity,
  isStoreLeaseHeld,
  readStoreLeaseMetadata,
  STORE_LEASE_SCHEMA,
} from "../src/graph/store-lease.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const HOLDER_SCRIPT = join(HERE, "fixtures", "store-lease-holder.mjs");

function withTempDb(fn) {
  const dir = mkdtempSync(join(tmpdir(), "blueprint-store-lease-"));
  const dbPath = join(dir, "graph.db");
  try {
    return fn(dbPath);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

function spawnHolder(dbPath, mode, extraArgs = []) {
  const child = spawn(process.execPath, [HOLDER_SCRIPT, dbPath, mode, ...extraArgs], { stdio: ["ignore", "pipe", "pipe"] });
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (d) => { stdout += d.toString(); });
  child.stderr.on("data", (d) => { stderr += d.toString(); });
  return {
    child,
    lines() { return stdout.split("\n").filter(Boolean).map((line) => JSON.parse(line)); },
    stderr() { return stderr; },
  };
}

async function waitFor(predicate, { timeoutMs = 5000, intervalMs = 20 } = {}) {
  const started = Date.now();
  while (!predicate()) {
    if (Date.now() - started > timeoutMs) throw new Error("waitFor timed out");
    await new Promise((r) => setTimeout(r, intervalMs));
  }
}

function exited(child) {
  return new Promise((resolve) => child.on("exit", (code, signal) => resolve({ code, signal })));
}

test("acquireStoreLease returns BlueprintStoreLeaseV1 metadata and releases cleanly", () => {
  withTempDb((dbPath) => {
    const handle = acquireStoreLease(dbPath, { ownerKind: "hub", ownerInstanceId: "test-owner-1" });
    assert.equal(handle.lease.schema, STORE_LEASE_SCHEMA);
    assert.equal(handle.lease.owner_kind, "hub");
    assert.equal(handle.lease.owner_instance_id, "test-owner-1");
    assert.equal(handle.lease.pid, process.pid);
    // Two independent computations of the same process's start identity can
    // differ by a millisecond of jitter (process.uptime() sampling), but must
    // agree closely — this is diagnostic identity, not a random token.
    assert.ok(
      Math.abs(handle.lease.process_start_identity - currentProcessStartIdentity()) < 1000,
      `process_start_identity drifted too far: ${handle.lease.process_start_identity} vs ${currentProcessStartIdentity()}`,
    );
    assert.ok(typeof handle.lease.acquired_at === "string" && !Number.isNaN(Date.parse(handle.lease.acquired_at)));
    assert.equal(handle.lease.heartbeat_at, handle.lease.acquired_at);
    assert.equal(handle.lease.lease_epoch, 1);

    assert.equal(isStoreLeaseHeld(dbPath), true);
    const onDisk = readStoreLeaseMetadata(dbPath);
    assert.deepEqual(onDisk, handle.lease);

    handle.release();
    assert.equal(isStoreLeaseHeld(dbPath), false);
    // The diagnostic sidecar is left describing the ended lease — it is
    // never the authority for "is this held right now"; isStoreLeaseHeld()
    // (backed by the real OS lock) is.
    assert.deepEqual(readStoreLeaseMetadata(dbPath), handle.lease);
    // Idempotent.
    assert.doesNotThrow(() => handle.release());
  });
});

test("heartbeat refreshes heartbeat_at without changing owner identity or epoch", async () => {
  withTempDb(async (dbPath) => {
    const handle = acquireStoreLease(dbPath, { ownerKind: "one_shot", ownerInstanceId: "hb-owner" });
    const first = handle.lease;
    await new Promise((r) => setTimeout(r, 5));
    const second = handle.heartbeat();
    assert.equal(second.owner_instance_id, first.owner_instance_id);
    assert.equal(second.pid, first.pid);
    assert.equal(second.lease_epoch, first.lease_epoch);
    assert.equal(second.acquired_at, first.acquired_at);
    assert.notEqual(second.heartbeat_at, first.heartbeat_at);
    assert.deepEqual(readStoreLeaseMetadata(dbPath), second);
    handle.release();
    assert.throws(() => handle.heartbeat(), (error) => error.code === "lease_already_released");
  });
});

test("lease_epoch increments across successive acquisitions of the same store", () => {
  withTempDb((dbPath) => {
    const first = acquireStoreLease(dbPath, { ownerKind: "one_shot" });
    assert.equal(first.lease.lease_epoch, 1);
    first.release();
    const second = acquireStoreLease(dbPath, { ownerKind: "one_shot" });
    assert.equal(second.lease.lease_epoch, 2);
    second.release();
  });
});

test("rejects an invalid owner_kind and a :memory: path", () => {
  withTempDb((dbPath) => {
    assert.throws(() => acquireStoreLease(dbPath, { ownerKind: "watcher" }), (error) => error.code === "invalid_owner_kind");
  });
  assert.throws(() => acquireStoreLease(":memory:", {}), (error) => error.code === "lease_requires_persistent_store");
});

test("a contending writer in the SAME process never becomes a second writer: typed resident_owner_active", () => {
  withTempDb((dbPath) => {
    const owner = acquireStoreLease(dbPath, { ownerKind: "hub", ownerInstanceId: "primary-owner" });
    assert.throws(
      () => acquireStoreLease(dbPath, { ownerKind: "one_shot", ownerInstanceId: "contender" }),
      (error) => {
        assert.equal(error.code, "resident_owner_active");
        // Diagnostic metadata names the actual OS-lock holder, for humans —
        // never consulted to decide the resident_owner_active outcome itself.
        assert.equal(error.details.lastKnownMetadata.owner_instance_id, "primary-owner");
        return true;
      },
    );
    // The primary owner's lease is completely unaffected by the failed contender.
    assert.equal(readStoreLeaseMetadata(dbPath).owner_instance_id, "primary-owner");
    owner.release();
  });
});

test("crashed owner: OS lock releases on SIGKILL even though lease metadata is left stale, and a new owner can then acquire", async () => {
  await withTempDb(async (dbPath) => {
    const holder = spawnHolder(dbPath, "hang");
    try {
      await waitFor(() => holder.lines().some((line) => line.ready === true), { timeoutMs: 5000 });
      const readyLine = holder.lines().find((line) => line.ready === true);
      assert.equal(readyLine.lease.pid, holder.child.pid);

      // While the holder is alive, a contender in THIS process must fail —
      // and the stale-looking metadata still correctly names the live holder.
      assert.throws(
        () => acquireStoreLease(dbPath, { ownerKind: "one_shot", ownerInstanceId: "contender-while-alive" }),
        (error) => error.code === "resident_owner_active",
      );
      assert.equal(isStoreLeaseHeld(dbPath), true);
      const metadataWhileAlive = readStoreLeaseMetadata(dbPath);
      assert.equal(metadataWhileAlive.pid, holder.child.pid);

      // Simulate a crash: SIGKILL gives the process no chance to run its own
      // release() / finally block. Only the OS's own advisory-lock teardown
      // on fd close can free this lease.
      holder.child.kill("SIGKILL");
      const { signal } = await exited(holder.child);
      assert.equal(signal, "SIGKILL");

      // Metadata is now STALE — it still claims the dead pid as owner, and
      // it MUST NOT be treated as evidence the store is still held.
      const staleMetadata = readStoreLeaseMetadata(dbPath);
      assert.equal(staleMetadata.pid, holder.child.pid, "metadata is expected to be stale, not auto-repaired by the crash itself");

      // The real question: does the OS lock repair itself? Poll briefly —
      // process teardown/fd close is not always instantaneous under load.
      await waitFor(() => isStoreLeaseHeld(dbPath) === false, { timeoutMs: 5000 });

      // A brand-new owner acquires cleanly — no manual repair ritual, no
      // permanent block from the stale metadata.
      const recovered = acquireStoreLease(dbPath, { ownerKind: "hub", ownerInstanceId: "post-crash-owner" });
      assert.equal(recovered.lease.owner_instance_id, "post-crash-owner");
      assert.equal(recovered.lease.pid, process.pid);
      assert.notEqual(recovered.lease.pid, staleMetadata.pid, "recovered owner must not be confused with the crashed pid");
      // Epoch continued from the stale metadata rather than resetting silently.
      assert.equal(recovered.lease.lease_epoch, staleMetadata.lease_epoch + 1);
      // Metadata is now repaired in place — no longer describes the dead owner.
      assert.equal(readStoreLeaseMetadata(dbPath).owner_instance_id, "post-crash-owner");
      recovered.release();
    } finally {
      try { holder.child.kill("SIGKILL"); } catch { /* already dead */ }
    }
  });
});

test("concurrent acquisition across real OS processes: exactly one writer ever wins", async () => {
  await withTempDb(async (dbPath) => {
    const CONTENDERS = 6;
    const racers = Array.from({ length: CONTENDERS }, () => spawnHolder(dbPath, "race", ["400"]));
    const results = await Promise.all(racers.map(async (racer) => {
      const { code } = await exited(racer.child);
      assert.equal(code, 0, racer.stderr());
      const [outcome] = racer.lines();
      assert.ok(outcome, `racer produced no output; stderr: ${racer.stderr()}`);
      return outcome;
    }));

    const winners = results.filter((r) => r.ok === true);
    const losers = results.filter((r) => r.ok === false);
    assert.equal(winners.length, 1, `expected exactly one writer to win, got ${winners.length}: ${JSON.stringify(results)}`);
    assert.equal(losers.length, CONTENDERS - 1);
    for (const loser of losers) assert.equal(loser.code, "resident_owner_active");

    // Every racer released (or crashed out) by the time it exited, so the
    // store is free again.
    assert.equal(isStoreLeaseHeld(dbPath), false);
  });
});

test("isStoreLeaseHeld is a non-destructive probe: it never leaves a lock behind", () => {
  withTempDb((dbPath) => {
    assert.equal(isStoreLeaseHeld(dbPath), false, "no lock file yet");
    const handle = acquireStoreLease(dbPath, {});
    assert.equal(isStoreLeaseHeld(dbPath), true);
    handle.release();
    // Probing must not itself have acquired-and-leaked a lock: a fresh
    // acquire must still succeed immediately.
    const again = acquireStoreLease(dbPath, {});
    assert.ok(again.lease);
    again.release();
  });
});
