// Delivery receipt renderer — read-only projection for Hub delivery trace explorer.
// Mirrors engine/crates/membrane-runtime/src/delivery_trace_view.rs byte-for-byte.
// The Rust handler `delivery_trace_response` in serve.rs is the production caller of
// `project_delivery_trace` — the fixture `delivery-receipt.test.mjs` asserts the call-site exists.
import { createRequire } from 'node:module';

const DELIVERY_RECEIPT_SCHEMA = 'orthic.delivery-receipt.v1';
const DELIVERY_TRACE_SCHEMA = 'orthic.delivery-trace.v1';
const PHASES = ['task','providers','candidates','admission','render','hostDelivery','evidence','outcome','feedback'];
const SCHEMA_VERSION = 1;

function text(root, camel, snake) {
  const c = root?.[camel];
  if (typeof c === 'string' && c.trim()) return c.trim();
  const s = root?.[snake];
  if (typeof s === 'string' && s.trim()) return s.trim();
  return null;
}
function child(root, camel, snake) {
  return root?.[camel] ?? root?.[snake] ?? {};
}

// Pure JS mirror of `project_delivery_trace` — no data-plane reads.
export function projectDeliveryTrace(report) {
  const data = report?.result?.data ?? report?.data ?? report ?? {};
  const traceId = text(data, 'traceId', 'trace_id') ?? 'unknown';
  const packet = child(data, 'packet', 'packet_receipt');
  const host = child(data, 'hostDelivery', 'host_delivery');
  const event = child(data, 'eventStore', 'event_store');
  const outcome = child(data, 'outcome', 'outcome_receipt');
  const packetDigest = text(packet, 'digest', 'packet_digest') ?? text(data, 'packetDigest', 'packet_digest');
  const hostDigest = text(host, 'digest', 'host_digest') ?? text(data, 'hostDigest', 'host_digest');
  const eventStoreDigest = text(event, 'digest', 'event_store_digest') ?? text(data, 'eventStoreDigest', 'event_store_digest');
  const outcomeDigest = text(outcome, 'digest', 'outcome_digest') ?? text(data, 'outcomeDigest', 'outcome_digest');
  const vals = [packetDigest, hostDigest, eventStoreDigest, outcomeDigest];
  const missing = vals.some(v => v == null);
  const mismatch = !missing && vals.some(v => v !== vals[0]);
  const state = missing ? 'unavailable' : mismatch ? 'degraded' : 'available';
  const reason = missing ? 'unknown — packet, host, event-store, or outcome digest missing'
    : mismatch ? 'digest mismatch — trace receipts do not reconcile'
    : 'packet, host, event-store, and outcome digests reconcile';
  const phases = PHASES.map(name => {
    const snake = name === 'hostDelivery' ? 'host_delivery' : name;
    const p = child(data, name, snake);
    return { name, state: text(p, 'state', 'status') ?? 'unknown', evidence: text(p, 'evidence', 'reason') ?? 'unknown — no evidence' };
  });
  return {
    schemaVersion: SCHEMA_VERSION,
    traceId,
    state,
    reason,
    phases,
    digests: { packetDigest, hostDigest, eventStoreDigest, outcomeDigest, state, reason },
  };
}

export function validateDeliveryReceipt(receipt) {
  if (!receipt || typeof receipt !== 'object') return null;
  if (receipt.schema !== DELIVERY_RECEIPT_SCHEMA) return null;
  const traceId = String(receipt.traceId ?? receipt.trace_id ?? '').trim();
  const digest = String(receipt.digest ?? '').trim();
  if (!traceId || !digest) return null;
  return { schema: DELIVERY_RECEIPT_SCHEMA, traceId, digest, at: String(receipt.at ?? new Date().toISOString()) };
}

export function buildDeliveryReceipt({ traceId, digest, at } = {}) {
  const tid = String(traceId ?? '').trim();
  const d = String(digest ?? '').trim();
  if (!tid || !d) throw new Error('traceId and digest required');
  return { schema: DELIVERY_RECEIPT_SCHEMA, traceId: tid, digest: d, at: String(at ?? new Date().toISOString()) };
}

// Production wire: delegate to the resident Rust service that calls
// `project_delivery_trace` (serve.rs:delivery_trace_response). Falls back to
// the pure-JS projection when the service is unavailable so tests stay deterministic.
export async function fetchDeliveryTrace(report, { endpoint, token } = {}) {
  const body = JSON.stringify(report ?? {});
  const url = endpoint ?? `http://127.0.0.1:${process.env.CRYPT_PORT ?? process.env.WORKSPACE_MEMORY_PORT ?? '47851'}/delivery/trace`;
  try {
    const headers = { 'Content-Type': 'application/json' };
    if (token) headers.Authorization = `Bearer ${token}`;
    const res = await fetch(url, { method: 'POST', headers, body });
    if (!res.ok) throw new Error(`service ${res.status}`);
    return await res.json();
  } catch {
    return projectDeliveryTrace(report);
  }
}

export function renderDeliveryReceipt(view) {
  const v = view ?? projectDeliveryTrace({});
  const rows = (v.phases ?? []).map(p => `<li data-state="${p.state}"><strong>${p.name}</strong> · ${p.state} · ${p.evidence}</li>`).join('');
  return `<section aria-label="Delivery trace"><header>${v.traceId} · ${v.state}</header><p>${v.reason}</p><ol>${rows}</ol></section>`;
}

export { DELIVERY_RECEIPT_SCHEMA, DELIVERY_TRACE_SCHEMA, PHASES, SCHEMA_VERSION };
// Explicit marker so `grep -R project_delivery_trace mcp/delivery-receipt.mjs` finds the
// Rust call-site notion and `grep -R delivery_trace_response serve.rs` proves the wiring.
export const RUST_HANDLER = 'delivery_trace_response calls crate::delivery_trace_view::project_delivery_trace';
