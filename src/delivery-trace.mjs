const esc = value => String(value ?? 'unknown').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const phases = ['task','providers','candidates','admission','render','hostDelivery','evidence','outcome','feedback'];
const value = (o, camel, snake) => typeof o?.[camel] === 'string' && o[camel] ? o[camel] : typeof o?.[snake] === 'string' && o[snake] ? o[snake] : null;
const child = (o, camel, snake) => o?.[camel] ?? o?.[snake] ?? {};

export function deliveryTraceViewModel(snapshot) {
  const d = snapshot?.result?.data ?? snapshot?.data ?? snapshot ?? {};
  const traceId = value(d, 'traceId', 'trace_id') ?? 'unknown';
  const packet = child(d, 'packet', 'packet_receipt'); const host = child(d, 'hostDelivery', 'host_delivery');
  const event = child(d, 'eventStore', 'event_store'); const outcome = child(d, 'outcome', 'outcome_receipt');
  const digests = { packet: value(packet, 'digest', 'packet_digest') ?? value(d, 'packetDigest', 'packet_digest'), host: value(host, 'digest', 'host_digest') ?? value(d, 'hostDigest', 'host_digest'), eventStore: value(event, 'digest', 'event_store_digest') ?? value(d, 'eventStoreDigest', 'event_store_digest'), outcome: value(outcome, 'digest', 'outcome_digest') ?? value(d, 'outcomeDigest', 'outcome_digest') };
  const vals = Object.values(digests); const missing = vals.some(v => !v); const mismatch = !missing && vals.some(v => v !== vals[0]);
  const state = missing ? 'unavailable' : mismatch ? 'degraded' : 'available';
  const reason = missing ? 'unknown — packet, host, event-store, or outcome digest missing' : mismatch ? 'digest mismatch — trace receipts do not reconcile' : 'packet, host, event-store, and outcome digests reconcile';
  return { schemaVersion: 'delivery-trace.v1', traceId, state, reason, digests: { ...digests, state, reason }, phases: phases.map(name => { const p = child(d, name, name === 'hostDelivery' ? 'host_delivery' : name); return { name, state: value(p, 'state', 'status') ?? 'unknown', evidence: value(p, 'evidence', 'reason') ?? 'unknown — no evidence' }; }) };
}

export function renderDeliveryTrace(snapshot, root) {
  const vm = deliveryTraceViewModel(snapshot);
  const rows = vm.phases.map(p => `<li data-state="${esc(p.state)}"><strong>${esc(p.name)}</strong> · ${esc(p.state)} · ${esc(p.evidence)}</li>`).join('');
  root.innerHTML = `<section class="delivery-trace" aria-label="Delivery trace"><header><h2>Delivery trace</h2><small>read-only · ${esc(vm.traceId)} · ${esc(vm.state)}</small></header><p>${esc(vm.reason)}</p><dl><dt>Packet</dt><dd>${esc(vm.digests.packet)}</dd><dt>Host</dt><dd>${esc(vm.digests.host)}</dd><dt>Event store</dt><dd>${esc(vm.digests.eventStore)}</dd><dt>Outcome</dt><dd>${esc(vm.digests.outcome)}</dd></dl><ol>${rows}</ol></section>`;
}
