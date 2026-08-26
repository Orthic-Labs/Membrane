export const RESOURCES = ["deliveries", "providers", "repositories", "adapters", "devices", "memory", "sentinel", "alerts"];
export const SUBSYSTEMS = ["pull", "push", "cortex", "blueprint", "ledger", "adapt"];
export const DIMENSIONS = ["installation", "adapter", "delivery"];
const SEVERITY = { critical: 0, error: 1, warning: 2, info: 3, unknown: 4 };
const esc = value => String(value ?? "unknown").replace(/[&<>\"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));

export function operationEnvelope(snapshot) {
  if (!snapshot || typeof snapshot !== "object") return { kind: "error", code: "snapshot_unavailable", message: "No snapshot cached" };
  if (snapshot.result) {
    if (snapshot.result.kind === "error") return snapshot.result;
    if (snapshot.result.kind !== "success" || !snapshot.result.data || typeof snapshot.result.data !== "object") return { kind: "error", code: "snapshot_invalid", message: "Snapshot result data unavailable" };
    return { kind: "success", data: snapshot.result.data };
  }
  if (snapshot.payload && typeof snapshot.payload === "object") return { kind: "success", data: snapshot.payload };
  return { kind: "success", data: snapshot };
}
export function normalizeSnapshot(snapshot) {
  const envelope = operationEnvelope(snapshot);
  if (envelope.kind === "error") return { envelope, data: null, stale: false };
  const data = { ...(envelope.data || {}) };
  if (data.observedAtUnixMs == null && snapshot?.observed_at_unix_ms != null) data.observedAtUnixMs = snapshot.observed_at_unix_ms;
  const stale = snapshot?.stale === true || snapshot?.cached === true || snapshot?.source === "cache" || data.stale === true || data.cached === true || Number(data.cacheAgeMs ?? snapshot?.cache_age_ms ?? 0) > 0;
  return { envelope, data, stale };
}
const TYPED_REASON_STATES = Object.freeze({
  not_instrumented: "not_configured",
  root_not_enrolled: "root_not_enrolled",
  stale: "stale",
  stale_generation: "stale",
  blueprint_stale: "stale",
  transport_unavailable: "transport_unavailable",
  hub_inactive: "hub_inactive",
  resident_owner_active: "resident_owner_active",
});
const typedReasonState = reason => TYPED_REASON_STATES[String(reason ?? "").toLowerCase()] || null;
export const lifecycleReasonLabel = reason => ({
  not_instrumented: "Not configured",
  root_not_enrolled: "Root not enrolled",
  stale: "Stale",
  stale_generation: "Stale generation",
  blueprint_stale: "Blueprint stale",
  transport_unavailable: "Transport unavailable",
  hub_inactive: "Hub inactive",
  resident_owner_active: "Resident owner active",
}[String(reason ?? "").toLowerCase()] || String(reason ?? "No evidence"));
function stateText(section) {
  return typedReasonState(section?.reason) || section?.state || "unknown";
}
function itemText(item) { if (item == null) return "unknown"; if (["string", "number", "boolean"].includes(typeof item)) return String(item); return Object.entries(item).map(([k, v]) => `${k}: ${typeof v === "object" ? JSON.stringify(v) : v}`).join(" · ") || "unknown"; }
export function sortedAlerts(items = []) { return [...items].map((alert, index) => ({ alert, index })).sort((a, b) => (SEVERITY[String(a.alert?.severity || "unknown").toLowerCase()] ?? 4) - (SEVERITY[String(b.alert?.severity || "unknown").toLowerCase()] ?? 4) || String(a.alert?.reason || "unknown").localeCompare(String(b.alert?.reason || "unknown")) || a.index - b.index).map(({ alert }) => alert); }
const firstItem = section => Array.isArray(section?.items) ? section.items[0] || {} : {};
const number = value => value !== null && value !== undefined && Number.isFinite(Number(value)) ? new Intl.NumberFormat("en").format(Number(value)) : "—";
const time = value => { const date = new Date(value); return value && !Number.isNaN(date.valueOf()) ? date.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" }) : "Unknown"; };
const injected = adapter => Number(String(adapter?.delivering?.evidence || "").match(/([\d,]+) inject/i)?.[1]?.replaceAll(",", "") || 0);
const stateLabel = state => ({ available: "Available", degraded: "Degraded", unavailable: "Unavailable", not_configured: "Not configured", root_not_enrolled: "Root not enrolled", stale: "Stale", transport_unavailable: "Transport unavailable", hub_inactive: "Hub inactive", resident_owner_active: "Resident owner active", running: "Running", offline: "Offline", unknown: "Unknown" }[state] || "Unknown");
const serviceLabel = state => ({ running: "Running", degraded: "Degraded", offline: "Offline" }[state] || "Offline");
const pill = state => `<span class="state-pill state-${esc(state || "unknown")}"><i aria-hidden="true"></i>${esc(stateLabel(state))}</span>`;
const metric = (label, value, detail, state) => `<article class="metric state-${esc(state || "unknown")}"><span>${esc(label)}</span><strong>${esc(value)}</strong><small>${esc(detail)}</small></article>`;

function residentServiceState(data, runtime) {
  // Frozen producer mapping reaches this view as payload membraneState. A
  // CACHED payload state is trusted only while the current poll still
  // delivers a valid live snapshot; a previously cached Running never masks
  // degradation or loss of the live snapshot.
  const fresh = ["available", "live"].includes(String(runtime?.snapshotState ?? "").toLowerCase());
  const service = String(runtime?.serviceState ?? runtime?.service_state ?? "unknown").toLowerCase();
  if (service === "degraded") return "degraded";
  if (!runtime) return "offline";
  if (service !== "running" && service !== "unknown") return "offline";
  if (!fresh || !data) return "degraded";
  const membraneState = data?.membrane_state ?? data?.membraneState;
  if (membraneState) {
    const ps = String(membraneState).toLowerCase();
    if (["running", "degraded", "offline"].includes(ps)) return ps;
  }
  return "running";
}
export function dashboardModel(data, runtime) {
  const sections = data?.sections || {}, subsystems = data?.subsystems || {}, memory = firstItem(sections.memory), sentinel = firstItem(sections.sentinel), delivery = firstItem(sections.deliveries), adapters = firstItem(sections.adapters).adapters || [];
  const serviceState = residentServiceState(data, runtime);
  return { sections, subsystems, memory, sentinel, delivery, adapters, serviceState, serviceStatus: serviceLabel(serviceState), memoryCount: memory.memoryCount ?? memory.database?.memoryCount, injections: adapters.reduce((sum, item) => sum + injected(item), 0), clients: adapters.length, contradictions: sentinel.contradictions?.count, machines: Array.isArray(sections.devices?.items) ? sections.devices.items.length : null, sessions: data?.sessions?.count ?? null };
}

export function renderOverview(snapshot, root, runtime) {
  const { envelope, data, stale } = normalizeSnapshot(snapshot);
  if (envelope.kind === "error") { const status = serviceLabel(residentServiceState(null, runtime)); root.innerHTML = `<section class="error-state" role="status"><h2>Membrane ${esc(status)}</h2><p>${esc(envelope.message || envelope.code || "Snapshot unavailable")}</p><small>Source: <code>hub.snapshot</code></small></section>`; return; }
  const model = dashboardModel(data, runtime), sections = model.sections, subsystems = model.subsystems || {};
  const sourceRows = [["Providers", sections.providers], ["Blueprint repositories", sections.repositories]].map(([name, value]) => `<li><div><strong>${name}</strong><small>${esc(lifecycleReasonLabel(value?.reason))}</small></div>${pill(stateText(value))}</li>`).join("");
  const health = RESOURCES.map(name => `<li><span>${esc(name)}</span>${pill(stateText(sections[name]))}<small>${esc(lifecycleReasonLabel(sections[name]?.reason))}</small></li>`).join("");
  const subsystemHealth = SUBSYSTEMS.map(name => `<li><span>${esc(name)}</span>${pill(stateText(subsystems[name]))}<small>${esc(lifecycleReasonLabel(subsystems[name]?.reason))}</small></li>`).join("");
  const adapters = model.adapters.length ? model.adapters.map(adapter => `<tr><td><strong>${esc(adapter.client || adapter.id)}</strong></td><td>${pill(stateText(adapter.active))}</td><td>${esc(adapter.delivering?.evidence || "No delivery evidence")}</td><td>${esc(adapter.active?.evidence || "Unknown")}</td></tr>`).join("") : `<tr><td colspan="4" class="unknown">No client evidence available</td></tr>`;
  root.innerHTML = `<div class="hub-shell"><aside class="sidebar"><div class="brand"><b>M</b><div><span>MEMBRANE</span><strong>Hub</strong></div></div><nav aria-label="Hub sections"><a class="active" href="#overview">Overview</a><a href="#memories">Memories</a><a href="#ledger">Ledger</a><a href="#sources">Sources</a><a href="#fleet">Fleet</a><a href="#sessions">Sessions</a></nav><div class="sidebar-state">${pill(model.serviceState)}<small>Membrane resident service</small></div></aside><div class="hub-content"><header class="topbar" id="overview"><div><p class="eyebrow">SYSTEM OVERVIEW</p><h1>Membrane ${esc(model.serviceStatus)}</h1></div><div class="observed"><span>${stale ? "Cached snapshot" : "Live snapshot"}</span><strong>${esc(time(data.observedAtUnixMs))}</strong></div></header><section class="metrics" aria-label="Key metrics">${metric("Memories", number(model.memoryCount), "Durable Cortex records", stateText(sections.memory))}${metric("Memory injections", number(model.injections), "Observed across clients", stateText(sections.deliveries))}${metric("Clients", number(model.clients), "Known adapters", stateText(sections.adapters))}${metric("Cortex contradictions", number(model.contradictions), "Needs review", stateText(subsystems.cortex ?? sections.sentinel))}${metric("Machines", number(model.machines), model.machines == null ? "Not configured" : "Observed fleet", stateText(sections.devices))}${metric("Sessions", number(model.sessions), model.sessions == null ? "Not configured" : "Observed sessions", model.sessions == null ? "unknown" : "available")}</section><section class="dashboard-grid"><article class="panel wide" id="memories"><header><div><p class="eyebrow">MEMORIES</p><h2>Delivery activity</h2></div>${pill(stateText(sections.memory))}</header><div class="memory-hero"><strong>${number(model.memoryCount)}</strong><span>durable memories</span></div><dl><div><dt>Last non-empty delivery</dt><dd>${esc(time(model.delivery.lastNonEmptyAt))}</dd></div><div><dt>Database</dt><dd>${esc(model.memory.database?.status || "Unknown")}</dd></div></dl></article><article class="panel" id="ledger"><header><div><p class="eyebrow">LEDGER</p><h2>Document index</h2></div>${pill(stateText(subsystems.ledger))}</header><div class="empty-stat"><strong>${stateLabel(stateText(subsystems.ledger))}</strong><span>${esc(lifecycleReasonLabel(subsystems.ledger?.reason) === "No evidence" ? "No Ledger evidence available" : lifecycleReasonLabel(subsystems.ledger?.reason))}</span></div><p class="note">Ledger owns document navigation/index — Cortex owns memory/sentinel.</p></article><article class="panel" id="sources"><header><div><p class="eyebrow">SOURCES</p><h2>Knowledge inputs</h2></div></header><ul class="source-list">${sourceRows}</ul></article><article class="panel wide"><header><div><p class="eyebrow">CLIENTS</p><h2>Adapters & delivery</h2></div>${pill(stateText(sections.adapters))}</header><div class="table-wrap"><table><thead><tr><th>Client</th><th>Active</th><th>Delivery</th><th>Last observed</th></tr></thead><tbody>${adapters}</tbody></table></div></article><article class="panel" id="fleet"><header><div><p class="eyebrow">FLEET</p><h2>Machines</h2></div>${pill(stateText(sections.devices))}</header><div class="empty-stat"><strong>${number(model.machines)}</strong><span>${model.machines == null ? "Machine telemetry is not configured" : "machines observed"}</span></div></article><article class="panel" id="sessions"><header><div><p class="eyebrow">SESSIONS</p><h2>Active contexts</h2></div>${pill(model.sessions == null ? "unknown" : "available")}</header><div class="empty-stat"><strong>${number(model.sessions)}</strong><span>${model.sessions == null ? "Session telemetry is not configured" : "sessions observed"}</span></div></article><article class="panel wide"><header><div><p class="eyebrow">HEALTH</p><h2>Operational resources</h2></div></header><ul class="health-list">${health}</ul></article><article class="panel wide"><header><div><p class="eyebrow">HEALTH</p><h2>Subsystems</h2></div></header><ul class="health-list">${subsystemHealth}</ul></article></section><footer>Read-only local snapshot · schema ${esc(data.schemaVersion || "unknown")} · source <code>hub.snapshot</code></footer></div></div>`;
}
