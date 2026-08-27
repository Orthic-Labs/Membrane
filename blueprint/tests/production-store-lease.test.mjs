// Acceptance coverage for the production lease wiring. The lower-level
// store-lease suite proves the OS primitive; these tests prove actual mutable
// entry points cannot bypass it.

import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { acquireStoreLease, isStoreLeaseHeld, readStoreLeaseMetadata } from "../src/graph/store-lease.mjs";
import { createBlueprintApplicationService } from "../src/lib/application/service.mjs";
import { RootRegistry } from "../src/lib/application/root-registry.mjs";
import { RepositoryActor } from "../watchman/repo-actor.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const CLI = join(HERE, "..", "scripts", "blueprint.mjs");
const HOLDER = join(HERE, "fixtures", "store-lease-holder.mjs");

function fixtureRepo() {
  const root = mkdtempSync(join(tmpdir(), "blueprint-production-lease-"));
  writeFileSync(join(root, "sample.mjs"), "export const answer = 42;\n");
  return root;
}

function dbPath(root) {
  return join(root, ".agent", "graph", "graph.db");
}

function graphBuild(root) {
  return spawnSync(process.execPath, [CLI, "graph", "build", "--root", root, "--out", ".agent"], {
    cwd: root,
    encoding: "utf8",
    env: { ...process.env, BLUEPRINT_TREESITTER: "0" },
    timeout: 20_000,
  });
}

async function waitFor(predicate, timeoutMs = 5000) {
  const started = Date.now();
  while (!predicate()) {
    if (Date.now() - started > timeoutMs) throw new Error("waitFor timed out");
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
}

function childExit(child) {
  return new Promise((resolve) => child.once("exit", (code, signal) => resolve({ code, signal })));
}

test("canonical one-shot graph publication fails typed while a resident owner holds the store", () => {
  const root = fixtureRepo();
  const path = dbPath(root);
  mkdirSync(dirname(path), { recursive: true });
  const resident = acquireStoreLease(path, { ownerKind: "hub", ownerInstanceId: "resident-test" });
  try {
    const result = graphBuild(root);
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, /resident_owner_active/);
    assert.equal(existsSync(path), false, "failed contender must not publish a store");
    assert.equal(readStoreLeaseMetadata(path).owner_instance_id, "resident-test");
  } finally {
    resident.release();
    rmSync(root, { recursive: true, force: true });
  }
});

test("canonical one-shot publication recovers after a crashed owner and publishes transactionally", async () => {
  const root = fixtureRepo();
  const path = dbPath(root);
  mkdirSync(dirname(path), { recursive: true });
  const holder = spawn(process.execPath, [HOLDER, path, "hang"], { stdio: ["ignore", "pipe", "pipe"] });
  let output = "";
  holder.stdout.on("data", (chunk) => { output += chunk.toString(); });
  try {
    await waitFor(() => output.includes('"ready":true'));
    const exited = childExit(holder);
    holder.kill("SIGKILL");
    const exit = await exited;
    assert.equal(exit.signal, "SIGKILL");
    await waitFor(() => !isStoreLeaseHeld(path));

    const result = graphBuild(root);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(existsSync(path), true);
    assert.equal(readStoreLeaseMetadata(path).owner_kind, "one_shot");
    assert.equal(isStoreLeaseHeld(path), false, "bounded publication must release before process exit");
  } finally {
    try { holder.kill("SIGKILL"); } catch { /* already exited */ }
    rmSync(root, { recursive: true, force: true });
  }
});

test("Hub-hosted repository actor holds the resident lease for its writable lifetime", async () => {
  const root = fixtureRepo();
  try {
    const built = graphBuild(root);
    assert.equal(built.status, 0, built.stderr);
    const path = dbPath(root);
    const actor = new RepositoryActor({ root, ownerId: "hub-instance-test" });
    await actor.initialize();
    try {
      assert.equal(isStoreLeaseHeld(path), true);
      assert.equal(readStoreLeaseMetadata(path).owner_kind, "hub");
      assert.equal(readStoreLeaseMetadata(path).owner_instance_id, "hub-instance-test");
      assert.throws(
        () => acquireStoreLease(path, { ownerKind: "one_shot" }),
        (error) => error.code === "resident_owner_active",
      );
      const residentService = createBlueprintApplicationService({
        freshnessOwnership: "resident",
        rootRegistry: new RootRegistry([{ root }]),
      });
      const result = await residentService.search({ repoRoot: root, query: "answer", allowStale: true });
      assert.ok(Array.isArray(result.results), "resident query must read without self-contending for the actor lease");
      assert.equal(result.freshnessReceipt.details.readOnly, true);
    } finally {
      await actor.stop();
    }
    assert.equal(isStoreLeaseHeld(path), false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("RepositoryActor without a supervisor owner is a bounded one-shot owner", async () => {
  const root = fixtureRepo();
  try {
    const built = graphBuild(root);
    assert.equal(built.status, 0, built.stderr);
    const actor = new RepositoryActor({ root });
    await actor.initialize();
    try {
      assert.equal(readStoreLeaseMetadata(dbPath(root)).owner_kind, "one_shot");
    } finally {
      await actor.stop();
    }
    assert.equal(isStoreLeaseHeld(dbPath(root)), false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
