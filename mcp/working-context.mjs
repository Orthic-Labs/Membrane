import { createHash } from "node:crypto";
import { DatabaseSync } from "node:sqlite";

const MAX_ITEMS = 128;
const MAX_TOKENS = 8192;
const ephemeralContexts = new Map();
const scratchpads = new Map();

const digest = (value) => `sha256:${createHash("sha256").update(JSON.stringify(value)).digest("hex")}`;
const instant = (value, label) => {
  const parsed = new Date(value);
  if (!value || Number.isNaN(parsed.valueOf())) throw new Error(`${label}_invalid`);
  return parsed.toISOString();
};
const tokenEstimate = (value) => Math.ceil(Buffer.byteLength(JSON.stringify(value), "utf8") / 4);
const contextKey = (path, sessionId, taskId) => `${path}\0${sessionId}\0${taskId}`;

export function workingContext({ contextId, sessionId, taskId, items = [], expiresAt, maxItems = MAX_ITEMS, maxTokens = MAX_TOKENS, authority = "A0", sourceRefs = [], durable = false }) {
  if (!sessionId || !taskId || !Array.isArray(items) || !Array.isArray(sourceRefs) || !["A0", "A1"].includes(authority)) throw new Error("working_context_identity_invalid");
  const expiry = instant(expiresAt, "working_context_expiry");
  if (items.length > Math.min(maxItems, MAX_ITEMS) || maxItems < 1 || maxItems > MAX_ITEMS || maxTokens < 1 || maxTokens > MAX_TOKENS || tokenEstimate(items) > maxTokens) throw new Error("working_context_bounds_invalid");
  if (authority === "A1" && sourceRefs.length === 0) throw new Error("working_context_provenance_required");
  const body = { schema: "orthic.working-context.v1", session_id: sessionId, task_id: taskId, items: [...items], expires_at: expiry, max_items: maxItems, max_tokens: maxTokens, authority, source_refs: [...sourceRefs], priority: "high", injectable: true, durable: durable === true };
  return { context_id: contextId || `working-context-${digest(body).slice(7, 31)}`, ...body };
}

export function scratchpad({ sessionId, taskId, items = [], expiresAt }) {
  if (!sessionId || !taskId || !Array.isArray(items) || items.length > MAX_ITEMS) throw new Error("scratchpad_bounds_invalid");
  if (items.some((item) => item && typeof item === "object" && ["chain_of_thought", "private_reasoning", "reasoning"].some((key) => key in item))) throw new Error("scratchpad_private_reasoning_forbidden");
  return { schema: "orthic.scratchpad.v1", session_id: sessionId, task_id: taskId, items: [...items], expires_at: instant(expiresAt, "scratchpad_expiry"), searchable: false, consolidatable: false, authority: "none", durable: false };
}

export function temporalFact({ factId, subject, predicate, object, observedAt, scopeId, authority = "A0", veracity = "unknown", sourceRefs = [], validFrom, validUntil, supersedes }) {
  if (!factId || !subject || !predicate || object === undefined || !scopeId || !["A0", "A1", "A2"].includes(authority) || !Array.isArray(sourceRefs)) throw new Error("temporal_fact_identity_invalid");
  const observed = instant(observedAt, "temporal_fact_observed_at");
  const start = validFrom ? instant(validFrom, "temporal_fact_valid_from") : observed;
  const end = validUntil ? instant(validUntil, "temporal_fact_valid_until") : undefined;
  if (end && end <= start) throw new Error("temporal_fact_interval_invalid");
  if (authority !== "A0" && sourceRefs.length === 0) throw new Error("temporal_fact_provenance_required");
  return { schema: "orthic.temporal-fact.v1", fact_id: factId, subject, predicate, object, observed_at: observed, scope_id: scopeId, authority, veracity, source_refs: [...sourceRefs], valid_from: start, ...(end ? { valid_until: end } : {}), ...(supersedes ? { supersedes } : {}) };
}

export function applyTemporalFact(facts, next, { singleValuedPredicates = [] } = {}) {
  if (!singleValuedPredicates.includes(next.predicate)) return [...facts, next];
  const current = facts.filter((fact) => fact.predicate === next.predicate && fact.subject === next.subject && fact.scope_id === next.scope_id && !fact.valid_until && fact.veracity === "supported");
  const closed = facts.map((fact) => current.includes(fact) ? { ...fact, valid_until: next.valid_from || next.observed_at, veracity: "superseded" } : fact);
  return [...closed, { ...next, ...(current.length ? { supersedes: current.map((fact) => fact.fact_id).sort().join(",") } : {}) }];
}

export class WorkingContextStore {
  constructor(path) {
    this.path = path;
    this.db = new DatabaseSync(path);
    this.db.exec(`CREATE TABLE IF NOT EXISTS membrane_working_context (
      context_id TEXT PRIMARY KEY,
      session_id TEXT NOT NULL,
      task_id TEXT NOT NULL,
      payload_json TEXT NOT NULL,
      payload_sha256 TEXT NOT NULL,
      expires_at TEXT NOT NULL,
      state TEXT NOT NULL CHECK(state IN ('active','closed')),
      created_at TEXT NOT NULL,
      closed_at TEXT
    ) STRICT;
    CREATE INDEX IF NOT EXISTS membrane_working_context_active
      ON membrane_working_context(session_id,task_id,state,expires_at);
    CREATE TABLE IF NOT EXISTS membrane_temporal_fact (
      fact_id TEXT PRIMARY KEY,
      subject TEXT NOT NULL,
      predicate TEXT NOT NULL,
      scope_id TEXT NOT NULL,
      payload_json TEXT NOT NULL,
      payload_sha256 TEXT NOT NULL,
      observed_at TEXT NOT NULL,
      valid_from TEXT NOT NULL,
      valid_until TEXT,
      veracity TEXT NOT NULL
    ) STRICT;
    CREATE INDEX IF NOT EXISTS membrane_temporal_fact_asof
      ON membrane_temporal_fact(scope_id,subject,predicate,valid_from,valid_until)`);
  }

  saveContext(input) {
    const context = workingContext(input);
    if (!context.durable) {
      const key = contextKey(this.path, context.session_id, context.task_id);
      const values = ephemeralContexts.get(key) || new Map();
      values.set(context.context_id, context);
      ephemeralContexts.set(key, values);
      return context;
    }
    const payload = JSON.stringify(context);
    const payloadSha256 = digest(payload);
    this.db.prepare("INSERT OR IGNORE INTO membrane_working_context(context_id,session_id,task_id,payload_json,payload_sha256,expires_at,state,created_at) VALUES(?,?,?,?,?,?,'active',?)").run(context.context_id, context.session_id, context.task_id, payload, payloadSha256, context.expires_at, new Date().toISOString());
    const row = this.db.prepare("SELECT payload_json,payload_sha256,state FROM membrane_working_context WHERE context_id=?").get(context.context_id);
    if (!row || row.payload_sha256 !== payloadSha256 || row.state !== "active") throw new Error("working_context_conflict");
    return JSON.parse(row.payload_json);
  }

  activeContexts({ sessionId, taskId, asOf = new Date().toISOString() }) {
    const at = instant(asOf, "working_context_as_of");
    const durable = this.db.prepare("SELECT payload_json FROM membrane_working_context WHERE session_id=? AND task_id=? AND state='active' AND expires_at>? ORDER BY created_at,context_id").all(sessionId, taskId, at).map((row) => JSON.parse(row.payload_json));
    const key = contextKey(this.path, sessionId, taskId);
    const ephemeral = [...(ephemeralContexts.get(key)?.values() || [])].filter((context) => context.expires_at > at);
    return [...durable, ...ephemeral].sort((left, right) => left.context_id.localeCompare(right.context_id));
  }

  closeContext(contextId) {
    for (const values of ephemeralContexts.values()) if (values.delete(contextId)) return true;
    return this.db.prepare("UPDATE membrane_working_context SET state='closed',closed_at=? WHERE context_id=? AND state='active'").run(new Date().toISOString(), contextId).changes === 1;
  }

  recordTemporalFact(input, { singleValuedPredicates = [] } = {}) {
    const next = temporalFact(input);
    this.db.exec("BEGIN IMMEDIATE");
    try {
      if (singleValuedPredicates.includes(next.predicate)) {
        const current = this.db.prepare("SELECT fact_id FROM membrane_temporal_fact WHERE scope_id=? AND subject=? AND predicate=? AND valid_until IS NULL AND veracity='supported' ORDER BY observed_at,fact_id").all(next.scope_id, next.subject, next.predicate);
        if (current.length) {
          next.supersedes = current.map((row) => row.fact_id).join(",");
          this.db.prepare("UPDATE membrane_temporal_fact SET valid_until=?,veracity='superseded' WHERE scope_id=? AND subject=? AND predicate=? AND valid_until IS NULL AND veracity='supported'").run(next.valid_from, next.scope_id, next.subject, next.predicate);
        }
      }
      const payload = JSON.stringify(next);
      const payloadSha256 = digest(payload);
      this.db.prepare("INSERT OR IGNORE INTO membrane_temporal_fact(fact_id,subject,predicate,scope_id,payload_json,payload_sha256,observed_at,valid_from,valid_until,veracity) VALUES(?,?,?,?,?,?,?,?,?,?)").run(next.fact_id, next.subject, next.predicate, next.scope_id, payload, payloadSha256, next.observed_at, next.valid_from, next.valid_until || null, next.veracity);
      const row = this.db.prepare("SELECT payload_json,payload_sha256 FROM membrane_temporal_fact WHERE fact_id=?").get(next.fact_id);
      if (!row || row.payload_sha256 !== payloadSha256) throw new Error("temporal_fact_conflict");
      this.db.exec("COMMIT");
      return JSON.parse(row.payload_json);
    } catch (error) {
      this.db.exec("ROLLBACK");
      throw error;
    }
  }

  temporalFactsAsOf({ scopeId, subject, predicate, asOf }) {
    const at = instant(asOf, "temporal_fact_as_of");
    return this.db.prepare("SELECT payload_json,valid_until,veracity FROM membrane_temporal_fact WHERE scope_id=? AND subject=? AND predicate=? AND valid_from<=? AND (valid_until IS NULL OR valid_until>?) ORDER BY valid_from,fact_id").all(scopeId, subject, predicate, at, at).map((row) => ({ ...JSON.parse(row.payload_json), ...(row.valid_until ? { valid_until: row.valid_until } : {}), veracity: row.veracity }));
  }

  close() { this.db.close(); }
}

export class ScratchpadStore {
  save(path, input) {
    const pad = scratchpad(input);
    scratchpads.set(contextKey(path, pad.session_id, pad.task_id), pad);
    return pad;
  }
  load(path, { sessionId, taskId, asOf = new Date().toISOString() }) {
    const key = contextKey(path, sessionId, taskId);
    const pad = scratchpads.get(key);
    if (!pad || pad.expires_at <= instant(asOf, "scratchpad_as_of")) return null;
    return pad;
  }
  clear(path, { sessionId, taskId }) {
    return scratchpads.delete(contextKey(path, sessionId, taskId));
  }
}
