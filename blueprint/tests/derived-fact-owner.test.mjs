import assert from "node:assert/strict";
import { cpSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { BlueprintRepositoryWorker } from "../src/graph/watchman.mjs";
import { buildGraphGeneration } from "../src/graph/static-provider.mjs";
import { closeStore, getGenerationEnvelope, listDerivedFactOwners, openStore } from "../src/graph/store-sqlite.mjs";

// derived_fact_owner ledger (P2 requirement 3): every fact in `fact_owner`
// must record which generation and which repository produced it. The ledger
// is the existing `fact_owner` table (Migration 4) extended with
// generation_id/repo_root (Migration 12) — these tests pin that lineage is
// actually populated, both by a full build and by the incremental delta path,
// not merely schema-present-but-empty.

const FIXTURE = join(import.meta.dirname, "..", "evals/fixture-repos/typescript-commerce");

function makeRepo(prefix) {
  const repo = realpathSync(mkdtempSync(join(tmpdir(), prefix)));
  cpSync(FIXTURE, repo, { recursive: true });
  return repo;
}

test("full build stamps every fact_owner row with the generation and repo that produced it", () => {
  const repo = makeRepo("blueprint-owner-full-");
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    const db = openStore(join(repo, ".agent/graph/graph.db"));
    try {
      const envelope = getGenerationEnvelope(db);
      const generationId = envelope.manifest.generationId;
      assert.ok(generationId, "generation must have a sealed generationId");
      const owners = listDerivedFactOwners(db);
      assert.ok(owners.length > 0, "full build must produce fact_owner rows");
      for (const owner of owners) {
        assert.equal(owner.generationId, generationId, `fact ${owner.factId} must be attributed to the sealed generation`);
        assert.equal(owner.repoRoot, repo, `fact ${owner.factId} must be attributed to the repo it was built from`);
      }
    } finally { closeStore(db); }
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("incremental delta re-attributes only the touched file's facts to the new generation", async () => {
  const repo = makeRepo("blueprint-owner-delta-");
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    const dbPath = join(repo, ".agent/graph/graph.db");
    const before = openStore(dbPath);
    let originalGenerationId;
    let untouchedOwners;
    try {
      originalGenerationId = getGenerationEnvelope(before).manifest.generationId;
      // config.ts is neither imported by nor an importer of service.ts, so it
      // sits outside service.ts's dependent-repair closure entirely — a true
      // negative control for the bounded-delta contract. (store.ts is a
      // dependency OF service.ts and routes.ts a dependent, so either would
      // legitimately be touched by repair and is not a clean control here.)
      untouchedOwners = listDerivedFactOwners(before, { path: "src/config.ts" });
      assert.ok(untouchedOwners.length > 0);
    } finally { closeStore(before); }

    const path = join(repo, "src/service.ts");
    writeFileSync(path, `${readFileSync(path, "utf8")}\nexport const ownerDeltaCheck = true;\n`);
    const result = await new BlueprintRepositoryWorker({ root: repo }).ingest("src/service.ts");
    assert.equal(result.applied, true);

    const after = openStore(dbPath);
    try {
      const finalGenerationId = getGenerationEnvelope(after).manifest.generationId;
      assert.notEqual(finalGenerationId, originalGenerationId, "an applied delta must reseal the generation identity");

      const touchedOwners = listDerivedFactOwners(after, { path: "src/service.ts" });
      assert.ok(touchedOwners.length > 0);
      for (const owner of touchedOwners) assert.equal(owner.repoRoot, repo, "the edited file's facts must carry the repo root");

      // The file node itself is always re-derived on a structural edit (its
      // lexical provider owns it unconditionally) — that row must move off
      // the pre-edit generation. It need not land on the exact FINAL
      // generation id: a dependent-closure repair can reseal the store again
      // after this file's own facts are written, which legitimately produces
      // an intermediate generation id for this file's own row. A pre-existing
      // symbol whose identity/content did not change under a non-lexical
      // provider likewise legitimately keeps its prior owning generation — it
      // genuinely was not re-produced by this delta, and a lineage ledger
      // that overwrote it would be lying about origin.
      const fileOwner = touchedOwners.find((owner) => owner.factKind === "node" && owner.providerId === "lexical");
      assert.ok(fileOwner, "the lexical file-node fact must be present");
      assert.notEqual(fileOwner.generationId, originalGenerationId, "the re-derived file node must move off the pre-edit generation");

      const newSymbolId = after.prepare("SELECT id FROM symbols WHERE name = 'ownerDeltaCheck'").get()?.id;
      assert.ok(newSymbolId, "the delta must have produced a symbol for the newly added export");
      const newSymbolOwner = touchedOwners.find((owner) => owner.factId === newSymbolId);
      assert.ok(newSymbolOwner, "the new symbol must have a fact_owner row");
      assert.notEqual(newSymbolOwner.generationId, originalGenerationId, "a brand-new symbol must not carry the pre-edit generation id");
      assert.ok(newSymbolOwner.generationId, "a brand-new symbol must carry a generation id");

      // An unrelated file's ownership rows are untouched by someone else's
      // edit — this is also the bounded-delta contract: a one-file edit must
      // not re-stamp facts outside its dependent-repair closure.
      const stillUnrelated = listDerivedFactOwners(after, { path: "src/config.ts" });
      assert.deepEqual(stillUnrelated, untouchedOwners);
    } finally { closeStore(after); }
  } finally { rmSync(repo, { recursive: true, force: true }); }
});
