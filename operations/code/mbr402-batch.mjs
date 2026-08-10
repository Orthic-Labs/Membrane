// MBR-402: fail-closed, one-provider-call code-operation batches.
import { CODE_BATCH_MAX_ITEMS, CODE_BATCH_MAX_BYTES, CODE_BATCH_MAX_TOKENS, CODE_BATCH_MAX_DEADLINE_MS } from "./code-batch-limits.mjs";
export const BATCH_LIMITS = Object.freeze({ items: CODE_BATCH_MAX_ITEMS, bytes: CODE_BATCH_MAX_BYTES, tokens: CODE_BATCH_MAX_TOKENS, deadlineMs: CODE_BATCH_MAX_DEADLINE_MS });

const terminal = (id, code, detail = undefined) => ({ id, status: "terminal", code, ...(detail ? { detail } : {}) });
const bytes = (value) => Buffer.byteLength(JSON.stringify(value), "utf8");
const tokenCount = (item) => Number.isInteger(item.tokens) && item.tokens >= 0 ? item.tokens : Math.ceil(bytes(item) / 4);

/** Validate admission, make exactly one provider call, and return request-order receipts. */
export async function executeCodeBatch({ items, deadlineMs = BATCH_LIMITS.deadlineMs, authorize, provider, signal }) {
  if (!Array.isArray(items) || items.length === 0) throw new Error("batch_items_required");
  if (items.length > BATCH_LIMITS.items) throw new Error("batch_item_limit");
  if (bytes(items) > BATCH_LIMITS.bytes) throw new Error("batch_byte_limit");
  if (!Number.isInteger(deadlineMs) || deadlineMs < 1 || deadlineMs > BATCH_LIMITS.deadlineMs) throw new Error("batch_deadline_limit");
  if (items.reduce((sum, item) => sum + tokenCount(item), 0) > BATCH_LIMITS.tokens) throw new Error("batch_token_limit");
  const started = Date.now();
  const receipts = new Array(items.length);
  const admitted = [];
  const seen = new Set();
  for (let index = 0; index < items.length; index += 1) {
    const item = items[index]; const id = typeof item?.id === "string" ? item.id : `item-${index}`;
    if (!item || typeof item !== "object" || Array.isArray(item)) { receipts[index] = terminal(id, "invalid_item"); continue; }
    if (seen.has(id)) { receipts[index] = terminal(id, "duplicate_item"); continue; }
    seen.add(id);
    if (signal?.aborted) { receipts[index] = terminal(id, "cancelled"); continue; }
    if (Date.now() - started >= deadlineMs) { receipts[index] = terminal(id, "deadline_exceeded"); continue; }
    try {
      const admission = await authorize(item);
      if (!admission || admission.ok === false) { receipts[index] = terminal(id, admission?.code || "denied"); continue; }
      if (!["repositoryRoot", "authority", "generationId", "sourceHash"].every((key) => typeof admission[key] === "string" && admission[key].length > 0)) {
        receipts[index] = terminal(id, "admission_incomplete"); continue;
      }
      admitted.push({ index, id, item, admission });
    } catch { receipts[index] = terminal(id, "denied"); }
  }
  if (admitted.length) {
    let result; let providerError = false;
    const remaining = Math.max(1, deadlineMs - (Date.now() - started));
    const controller = new AbortController();
    const onAbort = () => controller.abort(signal?.reason || new Error("cancelled"));
    if (signal) {
      if (signal.aborted) onAbort();
      else signal.addEventListener("abort", onAbort, { once: true });
    }
    const timer = setTimeout(() => controller.abort(new Error("deadline_exceeded")), remaining);
    try {
      result = await provider(admitted.map(({ id, item, admission }) => {
        const { caller: _caller, repository: _repository, scopeId: _scopeId, scopeGrantId: _scopeGrantId, taskGrantLevel: _taskGrantLevel, ...operation } = item;
        return { id, ...operation, repositoryRoot: admission.repositoryRoot, authority: admission.authority, generationId: admission.generationId, sourceHash: admission.sourceHash };
      }), controller.signal);
    } catch { providerError = true; result = null; }
    clearTimeout(timer);
    signal?.removeEventListener("abort", onAbort);
    if (providerError) {
      const code = signal?.aborted ? "cancelled" : (Date.now() - started >= deadlineMs ? "deadline_exceeded" : "provider_error");
      for (const entry of admitted) receipts[entry.index] = terminal(entry.id, code);
      return receipts;
    }
    const rows = Array.isArray(result) ? result : [];
    const byId = new Map();
    let invalidRows = false;
    for (const row of rows) {
      if (!row || typeof row.id !== "string" || !seen.has(row.id) || byId.has(row.id)) { invalidRows = true; continue; }
      byId.set(row.id, row);
    }
    for (const entry of admitted) {
      const row = byId.get(entry.id); const expected = entry.admission;
      if (!row || !expected.authority || row.repositoryRoot !== expected.repositoryRoot || row.generationId !== expected.generationId || row.sourceHash !== expected.sourceHash || row.authority !== expected.authority) receipts[entry.index] = terminal(entry.id, "provider_mismatch");
      else receipts[entry.index] = { id: entry.id, status: "success", data: row.data ?? row };
    }
    if (invalidRows) for (const entry of admitted) if (receipts[entry.index]?.status === "success") receipts[entry.index] = terminal(entry.id, "provider_items_invalid");
  }
  return receipts;
}
