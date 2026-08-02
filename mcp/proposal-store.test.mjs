import assert from "node:assert/strict";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { ProposalStore } from "./proposal-store.mjs";

test("C12 proposal survives restart and requires one explicit review decision", () => {
  const path = join(mkdtempSync(join(tmpdir(), "membrane-proposal-")), "events.sqlite3");
  let store = new ProposalStore(path);
  const pending = store.create({ proposalId: "proposal-1", repositoryId: "repo", scopeId: "scope", emission: { text: "candidate" } });
  assert.equal(pending.state, "pending");
  store.close();
  store = new ProposalStore(path);
  assert.equal(store.get("proposal-1").state, "pending");
  assert.equal(store.review("proposal-1", "approved", "operator").state, "approved");
  assert.throws(() => store.review("proposal-1", "rejected", "operator"), /not_pending/);
  store.close();
});
