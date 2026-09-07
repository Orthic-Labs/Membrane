import assert from "node:assert/strict";
import { cpSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { METHODS } from "../src/service/protocol.mjs";
import { createBlueprintApplicationService } from "../src/lib/application/service.mjs";
import { createAdmission, DECISION_ACTIONS } from "../src/lib/admission.mjs";
import { buildGraphGeneration } from "../src/graph/static-provider.mjs";
import { closeStore, openStore } from "../src/graph/store-sqlite.mjs";

const ROOT = join(import.meta.dirname, "..");

test("Blueprint exposes recall as its sole context-admission operation", () => {
  assert.ok(METHODS.includes("recall"));
  assert.equal(METHODS.includes("orient"), false);
  const service = createBlueprintApplicationService({ allowEmbeddedRoot: true });
  assert.equal(typeof service.recall, "function");
  assert.equal("orient" in service, false);
  const admission = createAdmission({ readGeneration: () => null, createContextCandidateSet: () => null });
  assert.equal(typeof admission.recall, "function");
  assert.equal("orient" in admission, false);
});

test("packaging has no retired Blueprint executable alias", () => {
  const packageJson = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8"));
  assert.equal(packageJson.bin["membrane-blueprint"], undefined);
});

// BPT-041: recall must return an allow/continue/block/noop orientation with
// scope, generation, freshness, evidence, omissions, receipt and next action,
// and the HOST enforces it. These tests drive the real application service —
// never `decision()` directly — so a decision builder that is defined but
// never wired into production fails them.

const RECALL_FIXTURE = join(ROOT, "evals/fixture-repos/typescript-commerce");

function gitInit(repo) {
  const git = (...args) => {
    const result = spawnSync("git", args, { cwd: repo, encoding: "utf8" });
    assert.equal(result.status, 0, `git ${args.join(" ")}: ${result.stderr}`);
    return result.stdout.trim();
  };
  git("init", "-q");
  git("config", "user.email", "blueprint@test.invalid");
  git("config", "user.name", "blueprint test");
  git("add", "-A");
  git("commit", "-qm", "fixture");
  return git("rev-parse", "HEAD");
}

function recallRepo({ git = false } = {}) {
  const repo = mkdtempSync(join(tmpdir(), "blueprint-orientation-"));
  cpSync(RECALL_FIXTURE, repo, { recursive: true });
  const head = git ? gitInit(repo) : null;
  buildGraphGeneration(repo, { outDir: ".agent", persist: true });
  return { repo, head };
}

// The sealed generation records what it was indexed at; overwriting that row is
// how a test puts the served generation behind the worktree without waiting for
// a watcher. `head` present but moved on => enumerable stale paths; `head`
// unresolvable => enumeration fails and the whole generation is suppressed.
function sealIndexedAt(repo, sourceObservation) {
  const db = openStore(join(repo, ".agent/graph/graph.db"));
  try {
    db.prepare(
      "INSERT INTO generation(key,value) VALUES ('sourceObservation',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
    ).run(JSON.stringify(sourceObservation));
  } finally {
    closeStore(db);
  }
}

// Resident ownership observes source without repairing it, which is the only
// mode in which a stale generation survives to the response boundary.
const residentService = () =>
  createBlueprintApplicationService({ allowEmbeddedRoot: true, freshnessOwnership: "resident" });

function assertOrientationEnvelope(result) {
  assert.ok(DECISION_ACTIONS.includes(result.action), `unknown action ${result.action}`);
  assert.equal(result.schemaVersion, 1);
  assert.ok(result.generationId, "generation must be reported");
  assert.equal(result.freshnessReceipt.schema, "BlueprintFreshnessReceiptV1");
  assert.equal(result.receipt, result.freshnessReceipt);
  assert.ok(Array.isArray(result.allowedScopes), "scope must be reported");
  assert.ok(Array.isArray(result.omissions), "omissions must be reported");
  assert.ok(result.evidence, "evidence must be reported");
  assert.equal(result.evidence.candidateCount, result.candidateSet.candidates.length);
  assert.ok("nextAction" in result, "next action must be reported");
  assert.ok(result.claimBoundary, "claim boundary must be reported");
}

test("BPT-041 recall returns allow with clean claims on a fresh generation", async () => {
  const { repo } = recallRepo();
  try {
    const service = createBlueprintApplicationService({ allowEmbeddedRoot: true });
    const result = await service.recall({ repoRoot: repo, query: "placeOrder", limit: 10 });
    assertOrientationEnvelope(result);
    assert.equal(result.action, "allow");
    assert.equal(result.reasonCode, "recalled");
    assert.ok(result.candidateSet.candidates.length > 0);
    assert.equal(result.nextAction, null);
    assert.equal(result.freshnessReceipt.suppression.required, false);
    assert.ok(result.allowedScopes.length > 0);
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("BPT-041 recall returns noop when nothing resolves to act on", async () => {
  const { repo } = recallRepo();
  try {
    const service = createBlueprintApplicationService({ allowEmbeddedRoot: true });
    const result = await service.recall({ repoRoot: repo, query: "zzz-no-such-symbol-anywhere", limit: 10 });
    assertOrientationEnvelope(result);
    assert.equal(result.action, "noop");
    assert.equal(result.reasonCode, "no_candidates");
    assert.equal(result.candidateSet.candidates.length, 0);
    assert.equal(result.recallCircuit.paths.length, 0);
    // A noop is still an honest empty answer, not a suppressed one.
    assert.equal(result.freshnessReceipt.suppression.required, false);
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("BPT-041 recall returns continue when evidence is served under a stale generation", async () => {
  const { repo, head } = recallRepo({ git: true });
  try {
    sealIndexedAt(repo, { head, dirty: false, statusDigest: "0".repeat(32) });
    const result = await residentService().recall({
      repoRoot: repo, query: "placeOrder", limit: 10, allowStale: true,
    });
    assertOrientationEnvelope(result);
    assert.equal(result.action, "continue");
    assert.equal(result.reasonCode, "recalled_changed_since_generation");
    assert.equal(result.freshnessReceipt.freshness, "changed_since_generation");
    assert.equal(result.freshnessReceipt.suppression.mode, "changed_paths");
    // Evidence is still served, and the caveat is explicit rather than implied.
    assert.ok(result.candidateSet.candidates.length > 0);
    assert.equal(result.nextAction, "blueprint build --out .agent");
    assert.equal(result.claimBoundary.cleanClaimAllowed, false);
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("BPT-041 recall returns block when the whole generation is withheld", async () => {
  const { repo } = recallRepo({ git: true });
  try {
    // An indexed revision git cannot resolve makes stale-path enumeration fail,
    // which suppresses every source-backed row rather than guessing it is fresh.
    sealIndexedAt(repo, { head: "0".repeat(40), dirty: false, statusDigest: "0".repeat(32) });
    const result = await residentService().recall({
      repoRoot: repo, query: "placeOrder", limit: 10, allowStale: true,
    });
    assertOrientationEnvelope(result);
    assert.equal(result.action, "block");
    assert.equal(result.reasonCode, "stale_generation_withheld");
    assert.equal(result.freshnessReceipt.suppression.mode, "whole_generation");
    assert.equal(result.nextAction, "blueprint build --out .agent");
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("BPT-041 a block decision is reported, not enforced, by Blueprint", async () => {
  const { repo } = recallRepo({ git: true });
  try {
    sealIndexedAt(repo, { head: "0".repeat(40), dirty: false, statusDigest: "0".repeat(32) });
    // Blueprint must not throw, must not withhold the envelope, and must not
    // silently drop what it suppressed: the host decides what to do with this.
    const result = await residentService().recall({
      repoRoot: repo, query: "placeOrder", limit: 10, allowStale: true,
    });
    assert.equal(result.action, "block");
    assert.ok(result.recallCircuit, "recall circuit must still be returned");
    assert.ok(result.candidateSet, "candidate set must still be returned");
    assert.ok(result.freshnessReceipt.staleSources, "stale-source evidence must still be returned");
    const suppressed = result.omissions.find((omission) => omission.reason === "stale_source_suppressed");
    assert.ok(suppressed, "suppressed evidence must be accounted for in omissions");
    assert.equal(suppressed.scope, "whole_generation");
    assert.ok(suppressed.count > 0, "omission must report how much evidence was withheld");
    // The same request without the block condition serves that evidence, so the
    // count above is real evidence withheld, not an empty result set.
    const fresh = await createBlueprintApplicationService({ allowEmbeddedRoot: true })
      .recall({ repoRoot: repo, query: "placeOrder", limit: 10 });
    assert.equal(fresh.candidateSet.candidates.length, suppressed.count);
  } finally { rmSync(repo, { recursive: true, force: true }); }
});
