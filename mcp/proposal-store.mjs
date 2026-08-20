import { createHash } from "node:crypto";
import { DatabaseSync } from "node:sqlite";
import { basename, dirname, extname, join } from "node:path";

const digest = (value) => `sha256:${createHash("sha256").update(typeof value === "string" ? value : JSON.stringify(value)).digest("hex")}`;

export function eventDbFor(memoryDb) {
  if (process.env.MEMBRANE_EVENT_DB?.trim()) return process.env.MEMBRANE_EVENT_DB.trim();
  const extension = extname(memoryDb);
  const stem = basename(memoryDb, extension) || "cortex";
  return join(dirname(memoryDb), `${stem}.membrane-events.sqlite3`);
}

export class ProposalStore {
  constructor(path) {
    this.db = new DatabaseSync(path);
    this.db.exec(`CREATE TABLE IF NOT EXISTS membrane_knowledge_proposal (
      proposal_id TEXT PRIMARY KEY,
      repository_id TEXT NOT NULL,
      scope_id TEXT NOT NULL,
      emission_json TEXT NOT NULL,
      emission_sha256 TEXT NOT NULL,
      state TEXT NOT NULL CHECK(state IN ('pending','approved','rejected')),
      created_at TEXT NOT NULL,
      decided_at TEXT,
      reviewer TEXT
    ) STRICT`);
  }
  create({ proposalId, repositoryId, scopeId, emission }) {
    const emissionJson = JSON.stringify(emission);
    const emissionSha256 = digest(emissionJson);
    this.db.prepare("INSERT OR IGNORE INTO membrane_knowledge_proposal(proposal_id,repository_id,scope_id,emission_json,emission_sha256,state,created_at) VALUES(?,?,?,?,?,'pending',?)").run(proposalId, repositoryId, scopeId, emissionJson, emissionSha256, new Date().toISOString());
    const row = this.get(proposalId);
    if (!row || row.repository_id !== repositoryId || row.scope_id !== scopeId || row.emission_sha256 !== emissionSha256) throw new Error("knowledge_proposal_conflict");
    return row;
  }
  get(proposalId) {
    return this.db.prepare("SELECT proposal_id,repository_id,scope_id,emission_sha256,state,created_at,decided_at,reviewer FROM membrane_knowledge_proposal WHERE proposal_id=?").get(proposalId) || null;
  }
  review(proposalId, decision, reviewer) {
    if (!['approved', 'rejected'].includes(decision) || typeof reviewer !== "string" || !reviewer.trim()) throw new Error("knowledge_proposal_review_invalid");
    const changed = this.db.prepare("UPDATE membrane_knowledge_proposal SET state=?,decided_at=?,reviewer=? WHERE proposal_id=? AND state='pending'").run(decision, new Date().toISOString(), reviewer, proposalId).changes;
    if (changed !== 1) throw new Error("knowledge_proposal_not_pending");
    return this.get(proposalId);
  }
  close() { this.db.close(); }
}
