const MAX_ITEMS = 128;
const MAX_TOKENS = 8192;

export function workingContext({ sessionId, taskId, items = [], expiresAt, maxItems = MAX_ITEMS, maxTokens = MAX_TOKENS, authority = "A0", sourceRefs = [] }) {
  if (!sessionId || !taskId || !expiresAt || !["A0", "A1"].includes(authority)) throw new Error("working_context_identity_invalid");
  if (items.length > Math.min(maxItems, MAX_ITEMS) || maxTokens < 1 || maxTokens > MAX_TOKENS) throw new Error("working_context_bounds_invalid");
  return { schema: "orthic.working-context.v1", session_id: sessionId, task_id: taskId, items: [...items], expires_at: expiresAt, max_items: maxItems, max_tokens: maxTokens, authority, source_refs: [...sourceRefs] };
}

export function scratchpad({ sessionId, taskId, items = [], expiresAt }) {
  if (!sessionId || !taskId || !expiresAt || items.length > MAX_ITEMS) throw new Error("scratchpad_bounds_invalid");
  return { schema: "orthic.scratchpad.v1", session_id: sessionId, task_id: taskId, items: [...items], expires_at: expiresAt, searchable: false, authority: "A0" };
}

export function temporalFact({ factId, subject, predicate, object, observedAt, scopeId, authority = "A0", veracity = "unknown", sourceRefs = [], validFrom, validUntil, supersedes }) {
  if (!factId || !predicate || !observedAt || !scopeId || !["A0", "A1", "A2"].includes(authority)) throw new Error("temporal_fact_identity_invalid");
  return { schema: "orthic.temporal-fact.v1", fact_id: factId, subject, predicate, object, observed_at: observedAt, scope_id: scopeId, authority, veracity, source_refs: [...sourceRefs], ...(validFrom ? { valid_from: validFrom } : {}), ...(validUntil ? { valid_until: validUntil } : {}), ...(supersedes ? { supersedes } : {}) };
}

export function applyTemporalFact(facts, next, { singleValuedPredicates = [] } = {}) {
  if (!singleValuedPredicates.includes(next.predicate)) return [...facts, next];
  return [...facts, next].map((fact) => fact === next ? fact : (fact.predicate === next.predicate && fact.subject === next.subject && fact.veracity === "supported" ? { ...fact, valid_until: next.observed_at, veracity: "superseded", supersedes: next.fact_id } : fact));
}
