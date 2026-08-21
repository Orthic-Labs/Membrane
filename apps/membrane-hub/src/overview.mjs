export const RESOURCES = ["deliveries", "providers", "repositories", "adapters", "devices", "memory", "sentinel", "alerts"];
export const DIMENSIONS = ["installation", "adapter", "delivery"];
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
const firstItem = section => Array.isArray(section?.items) ? section.items[0] || {} : {};
const number = value => value !== null && value !== undefined && Number.isFinite(Number(value)) ? new Intl.NumberFormat("en").format(Number(value)) : "—";
const time = value => { const date = new Date(value); return value && !Number.isNaN(date.valueOf()) ? date.toLocaleString(undefined,{dateStyle:"medium",timeStyle:"short"}) : "Unknown"; };
const injected = adapter => Number(String(adapter?.delivering?.evidence||"").match(/([\d,]+) inject/i)?.[1]?.replaceAll(",","")||0);
const stateLabel = state => ({available:"Healthy",degraded:"Needs attention",unavailable:"Unavailable",unknown:"Unknown"}[state]||"Unknown");
const pill = state => `<span class="state-pill state-${esc(state||"unknown")}"><i aria-hidden="true"></i>${esc(stateLabel(state))}</span>`;
const metric = (label,value,detail,state) => `<article class="metric state-${esc(state||"unknown")}"><span>${esc(label)}</span><strong>${esc(value)}</strong><small>${esc(detail)}</small></article>`;

export function dashboardModel(data) {
  const sections=data?.sections||{}; const memory=firstItem(sections.memory); const sentinel=firstItem(sections.sentinel); const delivery=firstItem(sections.deliveries); const adapters=firstItem(sections.adapters).adapters||[];
  return {sections,memory,sentinel,delivery,adapters,memoryCount:memory.memoryCount??memory.database?.memoryCount,injections:adapters.reduce((sum,item)=>sum+injected(item),0),clients:adapters.length,contradictions:sentinel.contradictions?.count,machines:Array.isArray(sections.devices?.items)?sections.devices.items.length:null,sessions:data?.sessions?.count??null};
}

export function renderOverview(snapshot, root) {
  const { envelope, data, stale } = normalizeSnapshot(snapshot);
  if (envelope.kind === "error") { root.innerHTML=`<section class="error-state" role="status"><h2>Hub unavailable</h2><p>${esc(envelope.message||envelope.code||"Unknown error")}</p><small>Source: <code>hub.snapshot</code></small></section>`; return; }
  const model=dashboardModel(data); const s=model.sections;
  const clients=model.adapters.length?model.adapters.map(adapter=>`<tr><td><strong>${esc(adapter.client||adapter.id)}</strong></td><td>${pill(adapter.active?.state)}</td><td>${esc(adapter.delivering?.evidence||"No delivery evidence")}</td><td>${esc(adapter.active?.evidence||"Unknown")}</td></tr>`).join(""):`<tr><td colspan="4" class="unknown">No client evidence available</td></tr>`;
  const sourceRows=[["Providers",s.providers],["Blueprint repositories",s.repositories]].map(([name,value])=>`<li><div><strong>${name}</strong><small>${esc(value?.reason||"No evidence")}</small></div>${pill(value?.state)}</li>`).join("");
  const health=RESOURCES.map(name=>`<li><span>${esc(name)}</span>${pill(s[name]?.state)}<small>${esc(s[name]?.reason||"No evidence")}</small></li>`).join("");
  root.innerHTML=`<div class="hub-shell"><aside class="sidebar"><div class="brand"><b>M</b><div><span>MEMBRANE</span><strong>Hub</strong></div></div><nav aria-label="Hub sections"><a class="active" href="#overview">Overview</a><a href="#memories">Memories</a><a href="#guide">Guide</a><a href="#sources">Sources</a><a href="#fleet">Fleet</a><a href="#sessions">Sessions</a></nav><div class="sidebar-state">${pill(RESOURCES.some(name=>s[name]?.state==="degraded"||s[name]?.state==="unavailable")?"degraded":"available")}<small>Local Mac</small></div></aside><div class="hub-content"><header class="topbar" id="overview"><div><p class="eyebrow">SYSTEM OVERVIEW</p><h1>Your memory system</h1></div><div class="observed"><span>${stale?"Cached snapshot":"Live snapshot"}</span><strong>${esc(time(data.observedAtUnixMs))}</strong></div></header><section class="metrics" aria-label="Key metrics">${metric("Memories",number(model.memoryCount),"Durable Cortex records",s.memory?.state)}${metric("Memory injections",number(model.injections),"Observed across clients",s.deliveries?.state)}${metric("Clients",number(model.clients),"Known adapters",s.adapters?.state)}${metric("Guide contradictions",number(model.contradictions),"Needs review",s.sentinel?.state)}${metric("Machines",number(model.machines),model.machines==null?"Not instrumented":"Observed fleet",s.devices?.state)}${metric("Sessions",number(model.sessions),model.sessions==null?"Not instrumented":"Observed sessions",model.sessions==null?"unknown":"available")}</section><section class="dashboard-grid"><article class="panel wide" id="memories"><header><div><p class="eyebrow">MEMORIES</p><h2>Delivery activity</h2></div>${pill(s.memory?.state)}</header><div class="memory-hero"><strong>${number(model.memoryCount)}</strong><span>durable memories</span></div><dl><div><dt>Last non-empty delivery</dt><dd>${esc(time(model.delivery.lastNonEmptyAt))}</dd></div><div><dt>Database</dt><dd>${esc(model.memory.database?.status||"Unknown")}</dd></div></dl></article><article class="panel" id="guide"><header><div><p class="eyebrow">GUIDE</p><h2>Ledger health</h2></div>${pill(s.sentinel?.state)}</header><div class="split-stat"><div><strong>${number(model.contradictions)}</strong><span>Contradictions</span></div><div><strong>${number(model.sentinel.proposals?.count)}</strong><span>Proposals</span></div></div><p class="note">${esc(model.sentinel.evidence?.reason||"No Guide evidence available")}</p></article><article class="panel" id="sources"><header><div><p class="eyebrow">SOURCES</p><h2>Knowledge inputs</h2></div></header><ul class="source-list">${sourceRows}</ul></article><article class="panel wide"><header><div><p class="eyebrow">CLIENTS</p><h2>Adapters & delivery</h2></div>${pill(s.adapters?.state)}</header><div class="table-wrap"><table><thead><tr><th>Client</th><th>Active</th><th>Delivery</th><th>Last observed</th></tr></thead><tbody>${clients}</tbody></table></div></article><article class="panel" id="fleet"><header><div><p class="eyebrow">FLEET</p><h2>Machines</h2></div>${pill(s.devices?.state)}</header><div class="empty-stat"><strong>${number(model.machines)}</strong><span>${model.machines==null?"Machine telemetry is not instrumented yet":"machines observed"}</span></div></article><article class="panel" id="sessions"><header><div><p class="eyebrow">SESSIONS</p><h2>Active contexts</h2></div>${pill(model.sessions==null?"unknown":"available")}</header><div class="empty-stat"><strong>${number(model.sessions)}</strong><span>${model.sessions==null?"Session telemetry is not instrumented yet":"sessions observed"}</span></div></article><article class="panel wide"><header><div><p class="eyebrow">HEALTH</p><h2>Subsystems</h2></div></header><ul class="health-list">${health}</ul></article></section><footer>Read-only local snapshot · schema ${esc(data.schemaVersion||"unknown")} · source <code>hub.snapshot</code></footer></div></div>`;
}
