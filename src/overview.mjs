const SEVERITY = { critical: 0, error: 1, warning: 2, info: 3, unknown: 4 };

const esc = value => String(value ?? "unknown").replace(/[&<>\"']/g, c => ({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;", "'":"&#39;"}[c]));
export function operationEnvelope(snapshot) {
  if (!snapshot || typeof snapshot !== "object") return { kind: "error", code: "snapshot_unavailable", message: "No snapshot cached" };
  if (snapshot.result) {
    if (snapshot.result.kind === "error") return snapshot.result;
    if (snapshot.result.kind !== "success" || !snapshot.result.data || typeof snapshot.result.data !== "object") {
      return { kind: "error", code: "snapshot_invalid", message: "Snapshot result data unavailable" };
    }
    return { kind: "success", data: snapshot.result.data };
  }
  if (snapshot.payload && typeof snapshot.payload === "object") {
    // New snapshot shape has `sections` map; older envelope carries payload directly.
    // Normalize both: if payload has sections, use it; otherwise fall through.
    return { kind: "success", data: snapshot.payload };
  }
  // Generic case: snapshot may be SnapshotV1 directly (with sections) or a plain map.
  return { kind: "success", data: snapshot };
}
export function normalizeSnapshot(snapshot) {
  const envelope = operationEnvelope(snapshot);
  if (envelope.kind === "error") return { envelope, data: null, stale: false };
  const data = { ...(envelope.data || {}) };
  if (data.observedAtUnixMs == null && snapshot?.observed_at_unix_ms != null) data.observedAtUnixMs = snapshot.observed_at_unix_ms;
  // Staleness: check top-level cacheAgeMs/stale and per-section cacheAgeMs generically.
  const sections = data.sections || data;
  const hasStaleSection = sections && typeof sections === "object"
    ? Object.values(sections).some(s => s && typeof s === "object" && Number(s.cacheAgeMs || 0) > 0)
    : false;
  const stale = snapshot?.stale === true || snapshot?.cached === true || snapshot?.source === "cache" || data.stale === true || data.cached === true || Number(data.cacheAgeMs ?? snapshot?.cache_age_ms ?? 0) > 0 || hasStaleSection;
  return { envelope, data, stale };
}
function meta(section, data) {
  return `<small class="provenance">source operation: <code>${esc(section.operation || data.operation || "hub.snapshot")}</code> · observed: ${esc(section.observedAtUnixMs || data.observedAtUnixMs || "unknown")} · schema: ${esc(section.schema || `hub.${data.schemaVersion || "unknown"}`)}</small>`;
}
function stateText(section) {
  return section?.state || "unknown";
}
function itemText(item) {
  if (item == null) return "unknown";
  if (typeof item === "string" || typeof item === "number" || typeof item === "boolean") return String(item);
  return Object.entries(item).map(([k, v]) => `${k}: ${typeof v === "object" ? JSON.stringify(v) : v}`).join(" · ") || "unknown";
}
export function sortedAlerts(items = []) {
  return [...items].map((alert, index) => ({ alert, index })).sort((a, b) => {
    const sa = SEVERITY[String(a.alert?.severity || "unknown").toLowerCase()] ?? 4;
    const sb = SEVERITY[String(b.alert?.severity || "unknown").toLowerCase()] ?? 4;
    return sa - sb || String(a.alert?.reason || "unknown").localeCompare(String(b.alert?.reason || "unknown")) || a.index - b.index;
  }).map(({ alert }) => alert);
}
function card(name, section, data, stale) {
  const state = stateText(section);
  const items = Array.isArray(section?.items) ? section.items : [];
  const unknown = state === "unavailable" || state === "unknown" || !section;
  return `<article class="card state-${esc(state)}${stale ? " stale" : ""}" tabindex="0" aria-label="${esc(name)} ${esc(state)}"><header><h2>${esc(name)}</h2><span class="badge">${esc(state)}</span></header><p>${esc(section?.reason || "unknown — no evidence")}</p><ul>${(items.length ? items : [unknown ? "unknown — no evidence" : "none observed"]).map(item => `<li>${esc(itemText(item))}</li>`).join("")}</ul>${meta(section || {}, data)}<small class="evidence">evidence: ${esc(section?.evidence || "unknown")}; resolver: ${esc(section?.resolver || "unknown")}${stale ? " · cached/stale" : ""}</small></article>`;
}
export function renderOverview(snapshot, root) {
  const { envelope, data, stale } = normalizeSnapshot(snapshot);
  if (envelope.kind === "error") {
    root.innerHTML = `<section class="error-state" role="status"><h2>Hub unavailable</h2><p>${esc(envelope.message || envelope.code || "unknown error")}</p><small>source operation: <code>hub.snapshot</code> · observed: unknown · schema: unknown</small></section>`;
    return;
  }
  // Generic: iterate over whatever sections the snapshot declares (open map).
  const sections = data.sections || {};
  // If snapshot uses legacy flat shape (no sections key), treat data itself as sections map excluding meta keys.
  const metaKeys = new Set(["schemaVersion","observedAtUnixMs","productId","stale","cacheAgeMs","operation","schema"]);
  let sectionEntries = Object.entries(sections);
  if (sectionEntries.length === 0 && !data.sections) {
    sectionEntries = Object.entries(data).filter(([k,v]) => !metaKeys.has(k) && v && typeof v === "object" && "state" in v);
  }
  const cards = sectionEntries.map(([name, section]) => card(name, section, data, stale)).join("");

  // Dimensions are now just sections; no hardcoded grid.
  // Keep alerts separate if present.
  const alerts = sortedAlerts(data.alerts?.items || sections.alerts?.items || []);
  const alertHtml = alerts.length ? alerts.map(a => `<li class="alert severity-${esc(String(a.severity || "unknown").toLowerCase())}"><strong>${esc(a.severity || "unknown")}</strong> ${esc(a.reason || "unknown")} · evidence: ${esc(a.evidence || "unknown")} · resolver: ${esc(a.resolver || "unknown")}</li>`).join("") : `<li class="unknown">unknown — no alert evidence</li>`;
  root.innerHTML = `<div class="snapshot-head" role="status"><strong>${stale ? "Cached snapshot — stale" : "Read-only Hub snapshot"}</strong><span>observed: ${esc(data.observedAtUnixMs || "unknown")} · schema: ${esc(data.schemaVersion || "unknown")} · source operation: <code>hub.snapshot</code></span></div><section class="grid" aria-label="Hub resources">${cards || '<p class="empty">No sections reported</p>'}</section><section class="alerts" aria-label="Alerts"><h2>Alerts</h2><ol>${alertHtml}</ol></section>`;
}
