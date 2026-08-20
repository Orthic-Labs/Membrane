import assert from "node:assert/strict";
import { closeStore, openStore } from "../src/graph/store-sqlite.mjs";
import { computeFullLedger, diffLedgerAgainstTree, updateLeafChain } from "../src/graph/merkle-ledger.mjs";
import test from "node:test";

const treeA = [
  { path: "src/a.ts", contentDigest: "xxh128:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" },
  { path: "src/nested/b.ts", contentDigest: "xxh128:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" },
];
const treeB = [
  { path: "src/a.ts", contentDigest: "xxh128:cccccccccccccccccccccccccccccccc" },
  { path: "src/nested/c.ts", contentDigest: "xxh128:dddddddddddddddddddddddddddddddd" },
];

test("Merkle root changes only when a leaf changes and diff is exact", () => {
  const db = openStore(":memory:");
  try {
    const rootA = computeFullLedger(db, treeA);
    assert.equal(diffLedgerAgainstTree(db, rootA, treeA).changed.length, 0);
    const diff = diffLedgerAgainstTree(db, rootA, treeB);
    assert.deepEqual(diff.changed, ["src/a.ts"]);
    assert.deepEqual(diff.added, ["src/nested/c.ts"]);
    assert.deepEqual(diff.removed, ["src/nested/b.ts"]);
    const rootB = updateLeafChain(db, "src/a.ts", treeB[0].contentDigest);
    updateLeafChain(db, "src/nested/b.ts", null);
    updateLeafChain(db, "src/nested/c.ts", treeB[1].contentDigest);
    assert.notEqual(rootA, rootB);
  } finally {
    closeStore(db);
  }
});

test("delta-built Merkle root equals clean full-build root", () => {
  const deltaDb = openStore(":memory:");
  const cleanDb = openStore(":memory:");
  try {
    computeFullLedger(deltaDb, treeA);
    updateLeafChain(deltaDb, "src/a.ts", treeB[0].contentDigest);
    updateLeafChain(deltaDb, "src/nested/b.ts", null);
    updateLeafChain(deltaDb, "src/nested/c.ts", treeB[1].contentDigest);
    const deltaRoot = deltaDb.prepare("SELECT digest FROM generation_leaf WHERE path = ''").get().digest;
    const cleanRoot = computeFullLedger(cleanDb, treeB);
    assert.equal(deltaRoot, cleanRoot);
  } finally {
    closeStore(deltaDb);
    closeStore(cleanDb);
  }
});
