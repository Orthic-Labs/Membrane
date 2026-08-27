/* Membrane Hub presentation model.
 *
 * This module is deliberately read-only. It consumes the authenticated Hub
 * snapshot, keeps typed reasons literal, and never fills an absent metric with
 * a plausible-looking zero. The dashboard is a compact view over one source of
 * truth; Ledger, Sources, and Subsystems are projections of that same payload.
 */

export const RESOURCES = ["deliveries", "providers", "repositories", "adapters", "devices", "memory", "sentinel", "alerts"];
export const SUBSYSTEMS = ["pull", "push", "cortex", "blueprint", "ledger", "adapt"];
export const DIMENSIONS = ["installation", "adapter", "delivery"];
export const VIEW_ORDER = ["overview", "ledger", "sources", "subsystems", "memories", "sessions"];

const SEVERITY = { critical: 0, error: 1, warning: 2, info: 3, unknown: 4 };
const esc = value => String(value ?? "unknown").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const finite = value => value !== null && value !== undefined && Number.isFinite(Number(value));
const number = value => finite(value) ? new Intl.NumberFormat("en").format(Number(value)) : "—";
const time = value => {
  const numeric = /^\d+$/.test(String(value ?? "")) ? Number(value) : null;
  if (numeric !== null && numeric < 946684800000) return "Unknown";
  const date = new Date(value);
  return value && !Number.isNaN(date.valueOf())
    ? date.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" })
    : "Unknown";
};

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

/* Typed lifecycle reasons are not rewritten into generic unavailable state. */
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

function stateLabel(state) {
  return ({
    available: "Available",
    degraded: "Degraded",
    unavailable: "Unavailable",
    not_configured: "Not configured",
    root_not_enrolled: "Root not enrolled",
    stale: "Stale",
    transport_unavailable: "Transport unavailable",
    hub_inactive: "Hub inactive",
    resident_owner_active: "Resident owner active",
    running: "Running",
    offline: "Offline",
    unknown: "Unknown",
  }[state] || "Unknown");
}

const serviceLabel = state => ({ running: "Running", degraded: "Degraded", offline: "Offline" }[state] || "Offline");
const serviceIndicator = state => ({ running: "available", degraded: "degraded", offline: "unavailable" }[state] || "unknown");

function firstItem(section) {
  return Array.isArray(section?.items) ? section.items[0] || {} : {};
}

function normalizeSections(data) {
  if (data?.sections && typeof data.sections === "object" && !Array.isArray(data.sections)) return data.sections;
  const sections = {};
  for (const name of RESOURCES) if (data?.[name] && typeof data[name] === "object") sections[name] = data[name];
  return sections;
}

function itemText(item) {
  if (item == null) return "unknown";
  if (["string", "number", "boolean"].includes(typeof item)) return String(item);
  return Object.entries(item).map(([key, value]) => `${key}: ${typeof value === "object" ? JSON.stringify(value) : value}`).join(" · ") || "unknown";
}

export function sortedAlerts(items = []) {
  return [...items]
    .map((alert, index) => ({ alert, index }))
    .sort((a, b) => (SEVERITY[String(a.alert?.severity || "unknown").toLowerCase()] ?? 4) - (SEVERITY[String(b.alert?.severity || "unknown").toLowerCase()] ?? 4) || String(a.alert?.reason || "unknown").localeCompare(String(b.alert?.reason || "unknown")) || a.index - b.index)
    .map(({ alert }) => alert);
}

const injected = adapter => Number(String(adapter?.delivering?.evidence || "").match(/([\d,]+) inject/i)?.[1]?.replaceAll(",", "") || 0);

function residentServiceState(data, runtime) {
  // A cached payload state is trusted only while this poll reports a live
  // snapshot. A previously cached Running value must never mask degradation.
  const fresh = ["available", "live"].includes(String(runtime?.snapshotState ?? "").toLowerCase());
  const service = String(runtime?.serviceState ?? runtime?.service_state ?? "unknown").toLowerCase();
  if (service === "degraded") return "degraded";
  if (!runtime) return "offline";
  if (service !== "running" && service !== "unknown") return "offline";
  if (!fresh || !data) return "degraded";
  const membraneState = data?.membrane_state ?? data?.membraneState;
  if (membraneState) {
    const state = String(membraneState).toLowerCase();
    if (["running", "degraded", "offline"].includes(state)) return state;
  }
  return "running";
}

function arrayAt(value, keys) {
  for (const key of keys) if (Array.isArray(value?.[key])) return value[key];
  return [];
}

function decisionEntries(data) {
  const ledger = data?.ledger && typeof data.ledger === "object" ? data.ledger : {};
  const sections = normalizeSections(data);
  const values = [
    ...arrayAt(data, ["recentDecisions", "decisions", "ledgerEntries"]),
    ...arrayAt(ledger, ["decisions", "entries", "items"]),
    ...arrayAt(sections.ledger, ["decisions", "entries", "items"]),
  ];
  return values.filter(item => item && typeof item === "object");
}

function sourceModel(data) {
  const sections = normalizeSections(data);
  const explorer = data?.sourcesExplorer && typeof data.sourcesExplorer === "object" ? data.sourcesExplorer : (data?.sources && typeof data.sources === "object" && !Array.isArray(data.sources) ? data.sources : {});
  const paths = [
    ...arrayAt(explorer, ["paths"]),
    ...arrayAt(data, ["sourcePaths"]),
    ...arrayAt(sections.repositories, ["paths"]),
  ].filter(item => item && typeof item === "object");
  const providers = (Array.isArray(explorer.providers) ? explorer.providers : (Array.isArray(sections.providers?.items) ? sections.providers.items : []))
    .filter(item => item && typeof item === "object" && ["path", "provider", "service", "id", "name", "sourceRef"].some(key => item[key] != null));
  return {
    explorer,
    repository: explorer.repository || data?.repository || {},
    generation: explorer.generation || data?.generation || {},
    readiness: explorer.readiness || data?.readiness || sections.repositories || {},
    parser: explorer.parser || data?.parser || {},
    contribution: explorer.recentContribution || explorer.contribution || data?.recentContribution || {},
    paths,
    providers,
    neighborhoods: arrayAt(explorer, ["neighborhoods"]),
    clocks: explorer.clocks || data?.clocks || {},
  };
}

export function dashboardModel(data, runtime) {
  const sections = normalizeSections(data);
  const subsystems = data?.subsystems || {};
  const memory = firstItem(sections.memory);
  const sentinel = firstItem(sections.sentinel);
  const delivery = firstItem(sections.deliveries);
  const adapterSection = firstItem(sections.adapters);
  const adapters = Array.isArray(adapterSection.adapters) ? adapterSection.adapters : [];
  const memoryCount = memory.memoryCount ?? memory.database?.memoryCount;
  const injectionValues = adapters.map(injected);
  const hasInjectionEvidence = adapters.some(adapter => /inject/i.test(String(adapter?.delivering?.evidence || "")));
  const serviceState = residentServiceState(data, runtime);
  return {
    sections,
    subsystems,
    memory,
    sentinel,
    delivery,
    adapters,
    serviceState,
    serviceStatus: serviceLabel(serviceState),
    memoryCount: finite(memoryCount) ? Number(memoryCount) : null,
    injections: hasInjectionEvidence ? injectionValues.reduce((sum, value) => sum + value, 0) : null,
    clients: Array.isArray(adapterSection.adapters) ? adapters.length : null,
    contradictions: finite(sentinel.contradictions?.count) ? Number(sentinel.contradictions.count) : null,
    machines: sections.devices?.state === "available" && Array.isArray(sections.devices.items) ? sections.devices.items.length : null,
    sessions: finite(data?.sessions?.count) ? Number(data.sessions.count) : null,
    admission: data?.admission && typeof data.admission === "object" ? data.admission : null,
    decisions: decisionEntries(data),
    sources: sourceModel(data),
    observedAt: data?.observedAtUnixMs,
  };
}

function reasonEvidence(value) {
  const reason = String(value?.reason ?? "No evidence");
  const label = lifecycleReasonLabel(reason);
  return label === reason ? reason : `${reason} · ${label}`;
}

// Verdict shapes: filled square = admitted/running, half square = degraded,
// hollow square = refused/unavailable, dash = never configured/unknown.
function pill(state, label = stateLabel(state)) {
  return `<span class="v state-${esc(state || "unknown")}" data-state="${esc(state || "unknown")}"><i aria-hidden="true"></i><b>${esc(label)}</b></span>`;
}

function row({ subject, verdict, evidence, observed, id, extra = "" }) {
  return `<li class="row"${id ? ` id="${esc(id)}"` : ""}${extra}><div class="subject"><strong>${esc(subject)}</strong>${evidence ? `<span class="evidence">${esc(evidence)}</span>` : ""}</div>${verdict ? pill(verdict) : ""}${observed ? `<time>${esc(observed)}</time>` : ""}</li>`;
}

function emptyState(title, detail, { code = "" } = {}) {
  return `<div class="empty-state"><strong>${esc(title)}</strong><p>${esc(detail)}</p>${code ? `<code>${esc(code)}</code>` : ""}</div>`;
}

function admissionTotals(admission) {
  if (!admission || !finite(admission.decisionsTotal) || !finite(admission.omissionsTotal)) return { total: null, omitted: null, admitted: null };
  const total = Number(admission.decisionsTotal);
  const omitted = Number(admission.omissionsTotal);
  if (total < 0 || omitted < 0 || omitted > total) return { total: null, omitted: null, admitted: null };
  return { total, omitted, admitted: total - omitted };
}

function reasonCounts(admission, key) {
  const values = Array.isArray(admission?.[key]) ? admission[key] : [];
  return values
    .filter(entry => entry && typeof entry.reason === "string" && finite(entry.count))
    .map(entry => ({ reason: entry.reason, count: Math.max(0, Number(entry.count)) }));
}

function admissionWindow(admission) {
  return finite(admission?.windowHours) ? `last ${Number(admission.windowHours)}h` : "window unavailable";
}

function percentage(value, total) {
  return finite(value) && finite(total) && Number(total) > 0 ? `${(Number(value) / Number(total) * 100).toFixed(1)}%` : "—";
}

function omissionState(reason) {
  const value = String(reason || "").toLowerCase();
  if (/(budget|attention|pressure|partial)/.test(value)) return "degraded";
  if (/(root|authority|scope|refus|reject|policy|blocked)/.test(value)) return "unavailable";
  return "unknown";
}

function decisionView(item) {
  const verdictValue = item?.verdict ?? item?.outcome ?? item?.decision ?? item?.status ?? item?.state;
  const verdict = typeof verdictValue === "object" ? (verdictValue.state || verdictValue.status || verdictValue.verdict) : verdictValue;
  const raw = String(verdict ?? "unknown").toLowerCase();
  const verdictState = raw.includes("admit") || raw === "available" || raw === "accepted" || raw === "allow" ? "available" : raw.includes("partial") || raw.includes("degrad") ? "degraded" : raw.includes("withhold") || raw.includes("refus") || raw.includes("reject") || raw === "unavailable" ? "unavailable" : raw.includes("config") ? "not_configured" : raw;
  const subject = item?.subject ?? item?.path ?? item?.sourceRef ?? item?.id ?? "Unknown subject";
  const subsystem = item?.subsystem ?? item?.owner ?? item?.axis ?? "unknown";
  const reason = item?.reason ?? (typeof item?.evidence === "object" ? item.evidence.reason : item?.evidence) ?? "No evidence";
  const observedValue = item?.observedAt ?? item?.observed_at ?? item?.observedAtUnixMs ?? item?.timestamp;
  const observed = observedValue == null ? "Unknown" : (finite(observedValue) ? time(Number(observedValue)) : String(observedValue));
  return { subject: String(subject), subsystem: String(subsystem), verdict: verdictState, verdictLabel: verdictState === "available" ? "Admitted" : verdictState === "degraded" ? "Partial" : verdictState === "unavailable" ? "Withheld" : stateLabel(verdictState), reason: String(reason), observed };
}

function renderDecisionRows(model, items = model.decisions) {
  if (!items.length) return `<tr><td colspan="5">${emptyState("No recent decisions in snapshot", "Ledger rows appear when Hub includes decision receipts; no row is inferred from delivery health.", { code: "decisions = unknown" })}</td></tr>`;
  return items.map(item => {
    const view = decisionView(item);
    return `<tr data-subsystem="${esc(view.subsystem)}" data-verdict="${esc(view.verdict)}" data-search="${esc(`${view.subject} ${view.subsystem} ${view.reason}`.toLowerCase())}"><td>${esc(view.observed)}</td><td><strong>${esc(view.subject)}</strong></td><td>${esc(view.subsystem)}</td><td>${pill(view.verdict, view.verdictLabel)}</td><td><code>${esc(view.reason)}</code></td></tr>`;
  }).join("");
}

const overviewResourceLabels = Object.freeze({
  deliveries: "Delivery",
  providers: "Providers",
  repositories: "Blueprint repositories",
  adapters: "Adapters",
  devices: "Devices",
  memory: "Memory",
  sentinel: "Sentinel",
  alerts: "Alerts",
});

function numericField(value, keys) {
  for (const key of keys) if (finite(value?.[key])) return Number(value[key]);
  return null;
}

function activityPoints(data) {
  const candidates = [
    data?.activityTrend,
    data?.activitySeries,
    data?.activity?.series,
    data?.telemetry?.activity,
    data?.admission?.activityTrend,
    data?.admission?.activitySeries,
  ];
  for (const candidate of candidates) {
    const values = Array.isArray(candidate) ? candidate : arrayAt(candidate, ["points", "items", "series"]);
    const points = values.map((item, index) => {
      if (!item || typeof item !== "object") return null;
      const admitted = numericField(item, ["admitted", "accepted", "admit"]);
      const held = numericField(item, ["heldBack", "held_back", "withheld", "omitted", "partial", "refused"]);
      if (admitted == null && held == null) return null;
      return {
        label: item.label ?? item.bucket ?? item.observedAt ?? item.observed_at ?? item.timestamp ?? index + 1,
        admitted,
        held,
      };
    }).filter(Boolean);
    if (points.length) return points;
  }
  return [];
}

function renderActivitySummary(model) {
  const metrics = [
    ["Memory", model.memoryCount, "durable memories"],
    ["Delivery", model.injections, "memory injections"],
    ["Clients", model.clients, "adapter clients"],
    ["Sentinel", model.contradictions, "findings"],
  ];
  const label = metrics.map(([, value, detail]) => `${number(value)} ${detail}`).join(" · ");
  return `<div class="activity-summary" aria-label="${esc(label)}">${metrics.map(([name, value, detail]) => `<span><i>${esc(name)}</i><b>${esc(number(value))}</b> ${esc(detail)}</span>`).join("")}</div>`;
}

function renderActivityTrend(model, data) {
  const points = activityPoints(data);
  if (!points.length) return `<div class="trend trend-empty" aria-label="Activity trend unavailable in snapshot"><span class="v state-unknown"><i aria-hidden="true"></i><b>Unknown</b></span><code>Activity trend unavailable</code></div>`;
  const max = points.reduce((value, point) => Math.max(value, point.admitted ?? 0, point.held ?? 0), 0);
  const interval = data?.activityTrend?.interval ?? data?.activity?.interval ?? data?.telemetry?.activity?.interval;
  const window = data?.activityTrend?.window ?? data?.activity?.window ?? data?.telemetry?.activity?.window;
  const bars = points.map(point => {
    const admittedHeight = point.admitted == null || max <= 0 ? 0 : Math.max(2, Math.round(point.admitted / max * 38));
    const heldHeight = point.held == null || max <= 0 ? 0 : Math.max(2, Math.round(point.held / max * 38));
    return `<i title="${esc(point.label)}">${point.held == null ? "" : `<b class="trend-held" style="height:${heldHeight}px"></b>`}${point.admitted == null ? "" : `<b class="trend-admitted" style="height:${admittedHeight}px"></b>`}</i>`;
  }).join("");
  const aria = points.map(point => `${point.label}: ${point.admitted == null ? "unknown" : `${number(point.admitted)} admitted`}, ${point.held == null ? "unknown" : `${number(point.held)} held back`}`).join("; ");
  const peak = max > 0 ? `peak ${number(max)}` : "peak unknown";
  return `<div class="trend" role="img" aria-label="${esc(`Activity trend · ${aria}`)}"><div class="trend-plot">${bars}</div><p class="trend-axis"><span><i class="trend-key trend-key-admitted"></i>admitted</span><span><i class="trend-key trend-key-held"></i>held back</span><em>${esc([peak, interval, window].filter(Boolean).join(" · "))}</em></p></div>`;
}

function boundaryValue(admission, keys) {
  return numericField(admission, keys);
}

function boundaryVerdicts(model) {
  const admission = model.admission;
  const totals = admissionTotals(admission);
  const unconfiguredFromSections = ["repositories", "providers"].filter(name => stateText(model.sections[name]) === "not_configured").length;
  const budget = finite(admission?.budgetPressureTotal) ? Number(admission.budgetPressureTotal) : null;
  const unconfigured = boundaryValue(admission, ["unconfiguredTotal", "unconfigured_total", "unconfiguredSources", "unconfigured_sources"]) ?? (unconfiguredFromSections || null);
  return [
    { label: "Admitted", state: "available", value: totals.admitted, detail: `${percentage(totals.admitted, totals.total)} of decisions`, href: "#ledger" },
    { label: "Withheld", state: "unavailable", value: totals.omitted, detail: `${percentage(totals.omitted, totals.total)} · typed omissions`, href: "#ledger" },
    { label: "Budget pressure", state: "degraded", value: budget, detail: budget == null ? "attention budget unavailable" : "subset of withheld", href: "#ledger", bar: false },
    { label: "Unconfigured", state: "not_configured", value: unconfigured, detail: unconfigured == null ? "source total unavailable" : `${number(unconfigured)} sources · never instrumented`, href: "#subsystems", bar: false },
  ];
}

function verdictShape(state) {
  return state === "available" ? "filled" : state === "degraded" ? "half" : state === "unavailable" ? "hollow" : "dash";
}

function renderBoundaryBar(model, verdicts) {
  const totals = admissionTotals(model.admission);
  const total = finite(totals.total) && Number(totals.total) > 0 ? Number(totals.total) : null;
  const known = verdicts.filter(item => item.bar !== false && finite(item.value) && Number(item.value) > 0);
  const knownTotal = known.reduce((sum, item) => sum + Number(item.value), 0);
  const residual = total == null ? null : Math.max(0, total - knownTotal);
  const segments = known.map(item => `<span class="boundary-segment segment-${esc(item.state)}" style="width:${esc(total == null ? 0 : Math.min(100, Number(item.value) / total * 100))}%" aria-label="${esc(`${item.label}: ${number(item.value)}`)}"></span>`).join("");
  const residualMarkup = residual > 0 ? `<span class="boundary-segment segment-unknown" style="width:${esc(total == null ? 0 : residual / total * 100)}%" aria-label="${esc(`${number(residual)} decisions not classified by available verdict fields`)}"></span>` : "";
  const aria = known.length ? known.map(item => `${item.label}: ${number(item.value)}`).join(", ") : "Admission verdict totals unavailable";
  const excluded = verdicts.find(item => item.bar === false && finite(item.value) && Number(item.value) > 0);
  const legendNote = model.admission
    ? [ `${number(totals.omitted)} withheld`, admissionWindow(model.admission), excluded ? `${number(excluded.value)} sources unconfigured, not counted here` : "" ].filter(Boolean).join(" · ")
    : "Admission ledger unavailable";
  return `<div class="boundary-bar-wrap"><div class="boundary-bar${known.length ? "" : " boundary-bar-empty"}" data-admission-chart role="img" aria-label="${esc(aria)}">${segments}${residualMarkup}</div><p class="boundary-legend"><span><b>${esc(number(totals.total))}</b> decisions at the boundary</span><span class="right">${esc(legendNote)}</span></p></div>`;
}

function renderBoundaryReasons(model) {
  if (!model.admission) return "";
  const omissions = reasonCounts(model.admission, "omissionsByReason");
  const typed = omissions.map(item => `<code>${esc(item.reason)}</code> ${esc(number(item.count))}`).join(" · ");
  const budget = finite(model.admission.budgetPressureTotal) ? `${number(model.admission.budgetPressureTotal)} candidates dropped at the attention budget, ${admissionWindow(model.admission)}` : "";
  if (!typed && !budget) return "";
  return `<p class="boundary-reasons">${typed ? `Typed omissions · ${typed}` : ""}${typed && budget ? " · " : ""}${esc(budget)}</p>`;
}

function renderBoundary(model) {
  const totals = admissionTotals(model.admission);
  const verdicts = boundaryVerdicts(model);
  const withheldSummary = model.admission ? `${number(totals.omitted)} of ${number(totals.total)} decisions withheld, ${admissionWindow(model.admission)}` : "Admission ledger unavailable";
  return `<section class="boundary" aria-label="Admission boundary"><span class="sr-only">${esc(withheldSummary)}</span><ul class="verdicts">${verdicts.map(item => `<li><a href="${esc(item.href)}" data-open-tab="${esc(item.href.slice(1))}"><div class="vhead"><span class="swatch shape-${verdictShape(item.state)} state-${esc(item.state)}" aria-hidden="true"></span><b>${esc(item.label)}</b><span class="go" aria-hidden="true">›</span></div><strong>${esc(number(item.value))}</strong><em>${esc(item.detail)}</em></a></li>`).join("")}</ul>${renderBoundaryBar(model, verdicts)}${renderBoundaryReasons(model)}</section>`;
}

function attentionGroups(model) {
  const resourceOrder = ["repositories", "providers", "memory", "devices", "alerts", "adapters", "deliveries", "sentinel"];
  const resources = resourceOrder
    .map(name => ({ subject: overviewResourceLabels[name], value: model.sections[name], target: name === "repositories" || name === "providers" ? "sources" : name === "memory" ? "memories" : "subsystems" }))
    .filter(({ value, subject }) => value && stateText(value) !== "unknown" && stateText(value) !== "available" && subject !== "Sentinel");
  if (model.contradictions != null && model.contradictions > 0) resources.push({ subject: "Sentinel findings", value: { state: "degraded", reason: `${number(model.contradictions)} contradiction${model.contradictions === 1 ? "" : "s"}` }, target: "memories" });
  const subsystems = SUBSYSTEMS
    .map(name => ({ subject: name, value: model.subsystems?.[name] }))
    .filter(({ value }) => value && stateText(value) !== "unknown" && stateText(value) !== "available");
  return { resources, subsystems };
}

function attentionLink(target) {
  return `<a class="attn-action" href="#${esc(target)}" data-open-tab="${esc(target)}">Open detail <span aria-hidden="true">→</span></a>`;
}

function renderAttention(model) {
  const { resources, subsystems } = attentionGroups(model);
  const count = resources.length + subsystems.length;
  const resourceRows = resources.map(({ subject, value, target }) => `<li><span class="what"><strong>${esc(subject)}</strong></span><span class="why">${esc(reasonEvidence(value))}</span>${pill(stateText(value))}${attentionLink(target)}</li>`).join("");
  const subsystemTokens = subsystems.map(({ subject, value }) => `<span class="subsystem-token" aria-label="${esc(`${subject}: ${reasonEvidence(value)}`)}"><strong>${esc(subject)}</strong>${pill(stateText(value))}</span>`).join("");
  const subsystemRow = subsystemTokens ? `<li class="attn-subsystems"><span class="what"><strong>Subsystems</strong></span><span class="why subsystem-token-list">${subsystemTokens}</span>${attentionLink("subsystems")}</li>` : "";
  const rows = resourceRows + subsystemRow;
  return `<section class="attention" aria-label="What needs me?"><div class="sec"><h3>What needs me?</h3><span class="n">${esc(number(count || null))}</span><a class="more" href="#subsystems" data-open-tab="subsystems">Details <span aria-hidden="true">→</span></a></div>${rows ? `<ul class="attn">${rows}</ul>` : `<p class="clear"><span class="v state-available"><i aria-hidden="true"></i><b>No exception recorded</b></span></p>`}</section>`;
}

function decisionSwatch(state) {
  return `<span class="decision-swatch shape-${verdictShape(state)} state-${esc(state)}" aria-hidden="true"></span>`;
}

function renderRecentDecisionsPanel(model) {
  const rows = model.decisions.slice(0, 5).map(item => {
    const view = decisionView(item);
    return `<li><time>${esc(view.observed)}</time>${decisionSwatch(view.verdict)}<span class="subj">${esc(view.subject)}</span><span class="rsn">${esc(view.reason)}</span></li>`;
  }).join("");
  const body = rows || `<li class="panel-empty">${emptyState("No recent decisions in snapshot", "Ledger rows appear when Hub includes decision receipts; no row is inferred from delivery health.", { code: "decisions = unknown" })}</li>`;
  return `<section class="panel decisions-panel"><header><h3>Recent decisions</h3><a class="more" href="#ledger" data-open-tab="ledger">All decisions <span aria-hidden="true">→</span></a></header><ul class="decisions">${body}</ul></section>`;
}

function sourceSplitEntries(model) {
  const explorer = model.sources?.explorer || {};
  const candidates = [explorer.admittedBySource, explorer.admittedByProvider, explorer.sourceSplit, explorer.split, explorer.breakdown];
  for (const candidate of candidates) {
    if (!Array.isArray(candidate)) continue;
    const values = candidate.map(item => ({
      subject: item?.source ?? item?.provider ?? item?.name ?? item?.id,
      count: numericField(item, ["admitted", "accepted", "count", "total"]),
    })).filter(item => item.subject != null && item.count != null);
    if (values.length) return values;
  }
  return [];
}

function renderSourcePanel(model) {
  const split = sourceSplitEntries(model);
  if (split.length) {
    const total = split.reduce((sum, item) => sum + item.count, 0);
    return `<section class="panel split-panel"><header><h3>Admitted by source</h3><a class="more" href="#sources" data-open-tab="sources">All sources <span aria-hidden="true">→</span></a></header><ul class="split">${split.map((item, index) => `<li><span class="nm">${esc(item.subject)}</span><span class="track"><b style="width:${esc(total > 0 ? item.count / total * 100 : 0)}%"></b></span><span class="ct">${esc(number(item.count))}</span><span class="pc">${esc(percentage(item.count, total))}</span></li>`).join("")}</ul></section>`;
  }
  const sources = model.sources.paths.length
    ? model.sources.paths.map(item => ({ subject: item.path || item.sourceRef || item.id, value: item }))
    : model.sources.providers.map(item => ({ subject: item.path || item.provider || item.service || item.id, value: { state: item.state || (item.ok === false ? "degraded" : "available"), reason: item.reason || item.evidence || item.status || "provider evidence" } }));
  const body = sources.length
    ? sources.slice(0, 4).map(item => `<li><span class="nm">${esc(valueText(item.subject))}</span><span class="source-evidence">${esc(reasonEvidence(item.value))}</span>${pill(stateText(item.value))}</li>`).join("")
    : `<li class="panel-empty">${emptyState("Source evidence unavailable", "No bounded paths or provider records are included in this snapshot.", { code: "sources = unknown" })}</li>`;
  return `<section class="panel split-panel"><header><h3>Bounded sources</h3><a class="more" href="#sources" data-open-tab="sources">All sources <span aria-hidden="true">→</span></a></header><ul class="split source-equivalent">${body}</ul></section>`;
}

function renderOverviewBody(model, stale, data) {
  const questions = "Is it alive? Is it being used? Is it grounded? What did it withhold? Is it hitting limits? What needs me?";
  const liveLabel = stale ? "Cached snapshot" : "Live snapshot";
  return `<div class="view view-overview"><header class="head"><div class="head-copy"><div class="title-line"><h1>Membrane ${esc(model.serviceStatus)}</h1>${pill(serviceIndicator(model.serviceState), model.serviceStatus)}</div><p class="lede">At a glance. Drill down for detail.</p><p class="sr-only overview-questions">${questions}</p>${renderActivitySummary(model)}</div><div class="head-side"><label class="range-control"><span>Window</span><select data-window-filter aria-label="Snapshot window">${model.admission && finite(model.admission.windowHours) ? `<option value="${esc(model.admission.windowHours)}">Last ${esc(model.admission.windowHours)}h</option>` : "<option>Observed window</option>"}<option value="all">All available</option></select></label>${renderActivityTrend(model, data)}<div class="observed"><span>${liveLabel}</span><strong>${esc(time(data.observedAtUnixMs))}</strong></div></div></header>${renderBoundary(model)}${renderAttention(model)}<div class="panels">${renderRecentDecisionsPanel(model)}${renderSourcePanel(model)}</div><footer>Read-only local snapshot · schema ${esc(data.schemaVersion || "unknown")} · source <code>hub.snapshot</code></footer></div>`;
}

function renderFilters(model) {
  const views = model.decisions.map(decisionView);
  const subsystems = [...new Set(views.map(item => item.subsystem).filter(Boolean))].sort();
  const verdicts = [...new Set(views.map(item => item.verdict).filter(Boolean))].sort();
  const options = (values, labels) => values.map(value => `<option value="${esc(value)}">${esc(labels[value] || stateLabel(value))}</option>`).join("");
  return `<form class="filters" data-ledger-filters><label><span>Subsystem</span><select data-filter-subsystem><option value="all">All subsystems</option>${options(subsystems, {})}</select></label><label><span>Verdict</span><select data-filter-verdict><option value="all">All verdicts</option>${options(verdicts, { available: "Admitted", degraded: "Partial", unavailable: "Withheld" })}</select></label><label class="filter-search"><span>Search</span><input type="search" data-filter-search placeholder="Search subjects or reasons" autocomplete="off"></label><button type="reset" class="filter-reset">Clear</button></form>`;
}

function wireLedgerFilters(root, model) {
  if (!root?.querySelector) return;
  const form = root.querySelector("[data-ledger-filters]");
  const body = root.querySelector("[data-ledger-body]");
  if (!form || !body) return;
  const rows = Array.from(body.querySelectorAll("tr[data-search]"));
  const apply = () => {
    const subsystem = form.querySelector("[data-filter-subsystem]")?.value || "all";
    const verdict = form.querySelector("[data-filter-verdict]")?.value || "all";
    const search = String(form.querySelector("[data-filter-search]")?.value || "").toLowerCase().trim();
    let visible = 0;
    for (const rowElement of rows) {
      const matches = (subsystem === "all" || rowElement.dataset.subsystem === subsystem) && (verdict === "all" || rowElement.dataset.verdict === verdict) && (!search || rowElement.dataset.search.includes(search));
      rowElement.hidden = !matches;
      if (matches) visible += 1;
    }
    const empty = body.querySelector("[data-filter-empty]");
    if (empty) empty.hidden = visible !== 0;
  };
  form.addEventListener("input", apply);
  form.addEventListener("change", apply);
  form.addEventListener("reset", () => window.requestAnimationFrame(apply));
  void model;
}

function renderLedgerAggregate(model) {
  if (!model.admission || model.decisions.length) return "";
  const totals = admissionTotals(model.admission);
  const budget = finite(model.admission.budgetPressureTotal) ? Number(model.admission.budgetPressureTotal) : null;
  const reasons = reasonCounts(model.admission, "omissionsByReason").map(item => `${item.reason}: ${number(item.count)}`).join(" · ");
  return `<section class="ledger-aggregate" aria-label="Aggregate admission receipts"><div><span>Admitted</span><strong>${esc(number(totals.admitted))}</strong></div><div><span>Withheld</span><strong>${esc(number(totals.omitted))}</strong></div><div><span>Budget pressure</span><strong>${esc(number(budget))}</strong></div><p>${esc(reasons || "No typed omission reasons recorded")} · row-level receipts are not included in this snapshot.</p></section>`;
}

function renderLedgerView(model, stale, data) {
  const status = model.admission ? `${number(model.admission.decisionsTotal)} decisions · ${admissionWindow(model.admission)}` : "Decision receipts unavailable";
  return `<div class="view view-ledger"><header class="view-head"><div><p class="eyebrow">RECORDED DECISIONS</p><h1>Ledger</h1><p class="lede">Every decision, recorded.</p></div><div class="observed"><span>${stale ? "Cached snapshot" : "Live snapshot"}</span><strong>${esc(status)}</strong></div></header>${renderFilters(model)}${renderLedgerAggregate(model)}<article class="panel ledger-panel"><header><div><p class="eyebrow">ADMISSION RECEIPTS</p><h2>Decision ledger</h2></div><span class="panel-note">${esc(status)}</span></header><div class="table-wrap"><table class="decision-table ledger-table"><thead><tr><th>Observed</th><th>Subject</th><th>Subsystem</th><th>Verdict</th><th>Reason</th></tr></thead><tbody data-ledger-body>${renderDecisionRows(model)}<tr class="filter-empty" data-filter-empty hidden><td colspan="5">${emptyState("No matching receipts", "Change filters to inspect another part of this snapshot.")}</td></tr></tbody></table></div></article><p class="view-foot">Source <code>hub.snapshot</code> · schema ${esc(data.schemaVersion || "unknown")} · rows are shown only when decision receipts are present.</p></div>`;
}

function valueText(value, fallback = "Unknown") {
  if (value == null || value === "") return fallback;
  if (typeof value === "object") return value.id ?? value.name ?? value.path ?? fallback;
  return String(value);
}

function renderSourceRows(model) {
  const sources = model.sources;
  const paths = sources.paths.length ? sources.paths.map(item => ({ subject: item.path || item.sourceRef || item.id, evidence: [item.kind, item.evidence].filter(Boolean).join(" · ") || "bounded path", state: item.state || "available" })) : sources.providers.map(item => ({ subject: item.path || item.provider || item.service || item.id, evidence: item.evidence || item.status || item.reason || "provider evidence", state: item.state || (item.ok === false ? "degraded" : "available") }));
  if (!paths.length) return `<li>${emptyState("No bounded sources in snapshot", "Sources appear when repository paths or provider evidence are included. No source row is inferred.", { code: "paths = unknown" })}</li>`;
  return paths.map(item => `<li class="source-row"><div><strong>${esc(valueText(item.subject))}</strong><code>${esc(valueText(item.evidence))}</code></div>${pill(stateText({ state: item.state }))}</li>`).join("");
}

function renderSourcesView(model, stale, data) {
  const source = model.sources;
  const readinessState = stateText(source.readiness);
  const contribution = source.contribution;
  return `<div class="view view-sources"><header class="view-head"><div><p class="eyebrow">REPOSITORY EVIDENCE</p><h1>Sources</h1><p class="lede">Bounded paths, parser evidence, and recent contribution.</p></div><div class="observed"><span>${stale ? "Cached snapshot" : "Live snapshot"}</span><strong>${esc(time(data.observedAtUnixMs))}</strong></div></header><section class="source-meta" aria-label="Source provenance"><dl><div><dt>Repository</dt><dd>${esc(valueText(source.repository.id || source.repository.origin || source.repository.path))}</dd></div><div><dt>Generation</dt><dd>${esc(valueText(source.generation.id || source.generation.index || source.generation.release))}</dd></div><div><dt>Readiness</dt><dd><span class="source-readiness">${pill(readinessState)}<code>${esc(valueText(source.readiness.reason, "No readiness evidence"))}</code></span></dd></div><div><dt>Parser</dt><dd>${esc([source.parser.name, source.parser.version].filter(Boolean).join(" ") || "Unknown")}</dd></div></dl></section><article class="panel sources-panel"><header><div><p class="eyebrow">CONNECTED SOURCES</p><h2>Bounded paths</h2></div><span class="panel-note">${esc(source.paths.length ? `${number(source.paths.length)} paths` : source.providers.length ? `${number(source.providers.length)} provider records` : "No source rows")}</span></header><ul class="source-list">${renderSourceRows(model)}</ul></article><article class="panel contribution-panel"><header><div><p class="eyebrow">RECENT CONTRIBUTION</p><h2>${esc(valueText(contribution.summary, "Contribution evidence"))}</h2></div></header><p class="contribution-copy">${esc(valueText(contribution.reason || contribution.evidence, source.paths.length ? "Paths are bounded to this snapshot." : "No contribution evidence in snapshot."))}</p>${Array.isArray(contribution.paths) && contribution.paths.length ? `<ul class="contribution-paths">${contribution.paths.map(path => `<li><code>${esc(path)}</code></li>`).join("")}</ul>` : ""}</article><p class="view-foot">Source <code>hub.snapshot</code> · clocks observed: <code>${esc(time(source.clocks.observedAtUnixMs ?? data.observedAtUnixMs))}</code></p></div>`;
}

function renderSubsystemsView(model, stale, data) {
  return `<div class="view view-subsystems"><header class="view-head"><div><p class="eyebrow">MEMBRANE AXES</p><h1>Subsystems</h1><p class="lede">Boundary configuration by subsystem, with typed evidence.</p></div><div class="observed"><span>${stale ? "Cached snapshot" : "Live snapshot"}</span><strong>${esc(time(data.observedAtUnixMs))}</strong></div></header><article class="panel subsystem-detail-panel"><header><div><p class="eyebrow">SIX NAMED SUBSYSTEMS</p><h2>Current state</h2></div><span class="panel-note">shape + word carry state</span></header><ul class="subsystem-detail-list">${SUBSYSTEMS.map(name => { const value = model.subsystems?.[name] || {}; const state = stateText(value); return `<li><div class="detail-subsystem"><strong>${esc(name)}</strong><code>${esc(reasonEvidence(value))}</code>${value?.evidence ? `<span>${esc(value.evidence)}</span>` : ""}</div>${pill(state)}<time>${esc(value?.observedAtUnixMs == null ? "Unknown" : time(value.observedAtUnixMs))}</time></li>`; }).join("")}</ul></article><article class="panel subsystem-note-panel"><header><div><p class="eyebrow">READING THE MAP</p><h2>Evidence first</h2></div></header><p>Each state is a direct projection of Hub-owned evidence. <code>not_instrumented</code> remains Not configured; it is not an outage. <code>transport_unavailable</code> remains distinct from parent Membrane health.</p></article><p class="view-foot">Source <code>hub.snapshot</code> · schema ${esc(data.schemaVersion || "unknown")}</p></div>`;
}

function renderSimpleView(model, tab, stale, data) {
  const sectionName = tab === "memories" ? "memory" : "deliveries";
  const section = model.sections[sectionName] || {};
  const state = stateText(section);
  const item = firstItem(section);
  const title = tab === "memories" ? "Memories" : "Sessions";
  const detail = tab === "memories" ? `${number(model.memoryCount)} durable memories observed in Cortex.` : "Session telemetry is not part of this snapshot.";
  return `<div class="view view-simple"><header class="view-head"><div><p class="eyebrow">${esc(title.toUpperCase())}</p><h1>${esc(title)}</h1><p class="lede">Read-only view over Hub snapshot evidence.</p></div><div class="observed"><span>${stale ? "Cached snapshot" : "Live snapshot"}</span><strong>${esc(time(data.observedAtUnixMs))}</strong></div></header><article class="panel simple-panel"><header><div><p class="eyebrow">CURRENT EVIDENCE</p><h2>${esc(title)}</h2></div>${pill(state)}</header>${state === "unknown" || state === "not_configured" ? emptyState(stateLabel(state), detail, { code: reasonEvidence(section) }) : `<div class="simple-value"><strong>${esc(tab === "memories" ? number(model.memoryCount) : number(item.count ?? data.sessions?.count))}</strong><span>${esc(detail)}</span></div>`}</article></div>`;
}

function stripAllEyebrows(markup) {
  return String(markup).replace(/<p class="eyebrow">[^<]*<\/p>/g, "");
}

export function renderOverview(snapshot, root, runtime) {
  const { envelope, data, stale } = normalizeSnapshot(snapshot);
  if (envelope.kind === "error") {
    const status = serviceLabel(residentServiceState(null, runtime));
    root.innerHTML = `<section class="error-state" role="status"><h2>Membrane ${esc(status)}</h2><p>${esc(envelope.message || envelope.code || "Snapshot unavailable")}</p><small>Source: <code>hub.snapshot</code></small></section>`;
    return;
  }
  const model = dashboardModel(data, runtime);
  root.innerHTML = stripAllEyebrows(renderOverviewBody(model, stale, data));
}

export function renderLedger(snapshot, root, runtime) {
  const { envelope, data, stale } = normalizeSnapshot(snapshot);
  if (envelope.kind === "error") { renderOverview(snapshot, root, runtime); return; }
  const model = dashboardModel(data, runtime);
  root.innerHTML = stripAllEyebrows(renderLedgerView(model, stale, data));
  wireLedgerFilters(root, model);
}

export function renderSources(snapshot, root, runtime) {
  const { envelope, data, stale } = normalizeSnapshot(snapshot);
  if (envelope.kind === "error") { renderOverview(snapshot, root, runtime); return; }
  root.innerHTML = stripAllEyebrows(renderSourcesView(dashboardModel(data, runtime), stale, data));
}

export function renderSubsystems(snapshot, root, runtime) {
  const { envelope, data, stale } = normalizeSnapshot(snapshot);
  if (envelope.kind === "error") { renderOverview(snapshot, root, runtime); return; }
  root.innerHTML = stripAllEyebrows(renderSubsystemsView(dashboardModel(data, runtime), stale, data));
}

export function renderView(snapshot, root, runtime, view = "overview") {
  if (view === "ledger") return renderLedger(snapshot, root, runtime);
  if (view === "sources") return renderSources(snapshot, root, runtime);
  if (view === "subsystems") return renderSubsystems(snapshot, root, runtime);
  if (view === "memories" || view === "sessions") {
    const { envelope, data, stale } = normalizeSnapshot(snapshot);
    if (envelope.kind === "error") return renderOverview(snapshot, root, runtime);
    root.innerHTML = stripAllEyebrows(renderSimpleView(dashboardModel(data, runtime), view, stale, data));
    return;
  }
  return renderOverview(snapshot, root, runtime);
}
