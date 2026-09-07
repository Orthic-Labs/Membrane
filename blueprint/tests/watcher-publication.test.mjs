import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { buildGraphGeneration, scanSourcesForPublication } from "../src/graph/static-provider.mjs";
import { getGenerationEnvelope } from "../src/graph/store-sqlite.mjs";
import { computeManifestDigest } from "../src/graph/generation-identity.mjs";
import { createBlueprintApplicationService } from "../src/lib/application/service.mjs";
import { RootRegistry } from "../src/lib/application/root-registry.mjs";
import { RepositoryActor, appendWatchEvents } from "../watchman/repo-actor.mjs";
import { publishCurrentSourceObservation } from "../watchman/reconcile.mjs";

function fixture() {
  const root = mkdtempSync(join(tmpdir(), "blueprint-watcher-publication-"));
  const git = (args) => execFileSync("git", args, { cwd: root, stdio: "ignore", windowsHide: true });
  git(["init", "--quiet"]);
  writeFileSync(join(root, "app.ts"), "export const original = 1;\n");
  git(["add", "app.ts"]);
  git(["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "--quiet", "-m", "fixture"]);
  buildGraphGeneration(root, { outDir: ".agent", persist: true });
  return root;
}

test("watcher publishes modify/add/delete for resident read-only queries", async () => {
  const root = fixture();
  const actor = new RepositoryActor({ root, ownerId: "publication-fixture" });
  try {
    await actor.initialize();
    assert.equal(actor.publishSourceObservation().published, true);
    const service = createBlueprintApplicationService({ freshnessOwnership: "resident", rootRegistry: new RootRegistry([{ root }]) });
    let prior = getGenerationEnvelope(actor.db).manifest.generationId;
    for (const [eventKind, path, name, text] of [
      ["modify", "app.ts", "modifiedAutomatically", "export const modifiedAutomatically = 2;\n"],
      ["create", "added.ts", "addedAutomatically", "export const addedAutomatically = 3;\n"],
      ["delete", "added.ts", "addedAutomatically", null],
    ]) {
      if (text === null) rmSync(join(root, path)); else writeFileSync(join(root, path), text);
      actor.ingest([{ eventKind, path }]);
      await actor.flush(true);
      const envelope = getGenerationEnvelope(actor.db);
      assert.notEqual(envelope.manifest.generationId, prior);
      assert.equal(envelope.manifest.manifestDigest, computeManifestDigest(envelope.manifest, envelope.sourceObservation));
      const beforeQuery = JSON.stringify(envelope);
      const response = await service.search({ repoRoot: root, query: name, allowStale: true });
      assert.equal(response.freshnessReceipt.freshness, "fresh", `${eventKind}: ${JSON.stringify(response.freshnessReceipt)}`);
      assert.equal(response.freshnessReceipt.details.readOnly, true);
      assert.equal(response.results.some((row) => row.name === name), eventKind !== "delete");
      assert.equal(JSON.stringify(getGenerationEnvelope(actor.db)), beforeQuery, "query never publishes or repairs");
      prior = envelope.manifest.generationId;
    }
  } finally { await actor.stop(); rmSync(root, { recursive: true, force: true }); }
});

test("publication refuses unobserved changes, gaps, pending repair, truncation & a changed generation", async () => {
  const root = fixture();
  const actor = new RepositoryActor({ root });
  try {
    await actor.initialize();
    assert.equal(publishCurrentSourceObservation(actor.db, root).published, true);
    const original = JSON.stringify(getGenerationEnvelope(actor.db));
    const refuse = (options = {}) => {
      assert.equal(publishCurrentSourceObservation(actor.db, root, options).published, false);
      assert.equal(JSON.stringify(getGenerationEnvelope(actor.db)), original);
    };
    writeFileSync(join(root, "unobserved.ts"), "export const unseen = 1;\n");
    refuse();
    rmSync(join(root, "unobserved.ts"));
    for (const [key, value] of [["event_gap", "1"], ["repair_progress", "{}"], ["domains_pending", "doc"]]) {
      actor.db.prepare("INSERT OR REPLACE INTO watch_state(key,value) VALUES (?,?)").run(key, value);
      refuse();
      actor.db.prepare("DELETE FROM watch_state WHERE key=?").run(key);
    }
    refuse({ scan: (...args) => ({ ...scanSourcesForPublication(...args), traversalTruncated: true }) });
    // Simulates a journal callback after scan, before publication transaction.
    refuse({ scan: (...args) => {
      const source = scanSourcesForPublication(...args);
      appendWatchEvents(actor.db, [{ eventKind: "modify", path: "app.ts" }]);
      return source;
    } });
  } finally { await actor.stop(); rmSync(root, { recursive: true, force: true }); }
});

test("publication refuses same-status dirty-file changes during attestation", async () => {
  const root = fixture();
  const actor = new RepositoryActor({ root });
  try {
    await actor.initialize();
    writeFileSync(join(root, "app.ts"), "export const firstDirtyValue = 2;\n");
    actor.ingest([{ eventKind: "modify", path: "app.ts" }]);
    await actor.flush(true);
    const before = JSON.stringify(getGenerationEnvelope(actor.db));
    let reads = 0;
    const result = publishCurrentSourceObservation(actor.db, root, { scan: (...args) => {
      const source = scanSourcesForPublication(...args);
      if (++reads === 1) writeFileSync(join(root, "app.ts"), "export const laterDirtyValue = 3;\n");
      return source;
    } });
    assert.equal(reads, 2);
    assert.equal(result.published, false);
    assert.equal(result.reason, "source_mismatch");
    assert.equal(JSON.stringify(getGenerationEnvelope(actor.db)), before);
  } finally { await actor.stop(); rmSync(root, { recursive: true, force: true }); }
});
