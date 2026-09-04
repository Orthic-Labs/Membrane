import assert from "node:assert/strict";
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { readPendingDomains } from "../src/graph/delta-store.mjs";
import { getGenerationEnvelope, closeStore, openStore } from "../src/graph/store-sqlite.mjs";
import { completePendingDocDomain } from "../src/lib/phase2-completion.mjs";
import { BlueprintRepositoryWorker } from "../src/graph/watchman.mjs";

const ROOT = join(import.meta.dirname, "..");
const CLI = join(ROOT, "scripts/blueprint.mjs");
const FIXTURE = join(ROOT, "evals/fixture-repos/typescript-commerce");

function makeRepo() {
  const repo = mkdtempSync(join(tmpdir(), "blueprint-phase2-watch-"));
  cpSync(FIXTURE, repo, { recursive: true });
  return repo;
}

function run(repo, args) {
  return spawnSync(process.execPath, [CLI, ...args], { cwd: repo, encoding: "utf8" });
}

test("watcher automatically consumes doc pending while preserving explicit Phase-2 judgment work", async () => {
  const repo = makeRepo();
  try {
    writeFileSync(
      join(repo, "docs/SECOND.md"),
      "# Secondary\n\nThe service is implemented in `src/service.ts`.\n",
    );
    const build = run(repo, ["build", "--out", ".agent"]);
    assert.equal(build.status, 0, build.stderr);

    writeFileSync(
      join(repo, "docs/SECOND.md"),
      "# Secondary\n\nThe service is implemented in `src/service.ts`.\nThe store is implemented in `src/store.ts`.\n",
    );

    const result = await new BlueprintRepositoryWorker({ root: repo }).ingest("docs/SECOND.md");
    assert.equal(result.applied, true);

    const db = openStore(join(repo, ".agent/graph/graph.db"));
    try {
      assert.equal(readPendingDomains(db).includes("doc"), false, "automatic doc producer must have an automatic consumer");
      assert.equal(
        db.prepare("SELECT value FROM watch_state WHERE key='phase2_completion_state'").get()?.value,
        "pending_judgment",
        "the watcher must not fabricate semantic judgment merely to clear doc freshness",
      );
      assert.ok(db.prepare("SELECT value FROM watch_state WHERE key='phase2_completion_generation'").get()?.value);
    } finally { closeStore(db); }

    const plan = JSON.parse(readFileSync(join(repo, ".agent/phase2-plan.json"), "utf8"));
    assert.equal(plan.complete, false);
    assert.ok(
      (plan.verdicts?.verify?.length ?? 0) > 0 || (plan.dimensions?.synthesize?.length ?? 0) > 0,
      "remaining Phase-2 work must stay explicit in the durable plan",
    );
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("document-domain completion is generation fenced and cannot clear a newer pending state", () => {
  const repo = makeRepo();
  try {
    writeFileSync(
      join(repo, "docs/SECOND.md"),
      "# Secondary\n\nThe service is implemented in `src/service.ts`.\n",
    );
    const build = run(repo, ["build", "--out", ".agent"]);
    assert.equal(build.status, 0, build.stderr);

    writeFileSync(
      join(repo, "docs/SECOND.md"),
      "# Secondary\n\nThe service is implemented in `src/service.ts`.\nChanged prose.\n",
    );
    const delta = run(repo, ["delta", "docs/SECOND.md", "--out", ".agent"]);
    assert.equal(delta.status, 0, delta.stderr);

    const db = openStore(join(repo, ".agent/graph/graph.db"));
    try {
      assert.equal(readPendingDomains(db).includes("doc"), true);
      const result = completePendingDocDomain(db, repo, {
        outDir: ".agent",
        beforeFinalize({ db: writable }) {
          const envelope = getGenerationEnvelope(writable);
          const manifest = { ...envelope.manifest, generationId: "gen:simulated-newer-source" };
          writable.prepare("UPDATE generation SET value=? WHERE key='manifest'").run(JSON.stringify(manifest));
        },
      });
      assert.equal(result.state, "superseded");
      assert.equal(readPendingDomains(db).includes("doc"), true, "an older completion must not clear a newer generation's pending mark");
    } finally { closeStore(db); }
  } finally { rmSync(repo, { recursive: true, force: true }); }
});
