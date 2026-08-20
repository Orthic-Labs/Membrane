import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const CLIENT = join(dirname(fileURLToPath(import.meta.url)), "client.mjs");
const LEVELS = { cursor: "L1", windsurf: "L1", generic_mcp: "L0" };

// MBR-207: four mutually exclusive adapter lifecycle states. The Hub and CLI
// both label clients with one of these so they cannot conflate "installed"
// with "active" or "delivering" without the classifier disagreeing out loud.
export const STATUSES = Object.freeze({
  installed: "installed",
  active: "active",
  delivering: "delivering",
  stale: "stale",
});

const STATUS_VALUES = new Set(Object.values(STATUSES));
const SCHEMA_VERSION = 1;
const SCHEMA = "membrane.adapter-heartbeat.v1";
const DEFAULT_STALENESS_MS = 60_000;

export function adapterManifest(name) {
  if (!LEVELS[name]) throw new Error("unsupported_adapter");
  return { schema: "membrane.adapter-shim.v1", adapter_id: name, max_honest_level: LEVELS[name], injection: "membrane_context", tool_receipts: false, response_gate: false };
}

export function requestContext(name, request, { root = process.cwd(), client = CLIENT } = {}) {
  const manifest = adapterManifest(name);
  const result = spawnSync(process.execPath, [client, "--input", "-"], { cwd: root, input: `${JSON.stringify(request)}\n`, encoding: "utf8", timeout: 1500, windowsHide: true });
  if (result.error || result.status !== 0) return { ...manifest, state: "degraded", reason: result.error?.code === "ETIMEDOUT" ? "context_timeout" : "context_unavailable" };
  try {
    const payload = JSON.parse(result.stdout.trim());
    return { ...manifest, state: payload.ok && payload.packet ? "context_enforced" : "advisory", packet: payload.packet || null, omissions: payload.degradationReason && payload.degradationReason !== "none" ? [payload.degradationReason] : [] };
  } catch {
    return { ...manifest, state: "degraded", reason: "malformed_context_response" };
  }
}

// MBR-207 pure status classifier. Mirrors Membrane resident heartbeat:
// stale wins when the lastSeen gap is larger than `stalenessMs`; a non-zero
// receipt.count wins over a self-declared `installed`; a heartbeat without
// receipts can never stay `installed` and is coerced to `active`.
export function classifyStatus(
  { status, lastSeen, receipts } = {},
  { now = Date.now(), stalenessMs = DEFAULT_STALENESS_MS } = {},
) {
  const lastSeenMs = typeof lastSeen === "number" ? lastSeen : Number(lastSeen);
  if (Number.isFinite(lastSeenMs) && now - lastSeenMs > stalenessMs) {
    return STATUSES.stale;
  }
  const list = Array.isArray(receipts) ? receipts : [];
  if (list.some((receipt) => Number(receipt?.count) > 0)) {
    return STATUSES.delivering;
  }
  if (status && STATUS_VALUES.has(status) && status !== STATUSES.stale) {
    // No receipts ⇒ installed is incoherent; coerce to active.
    return status === STATUSES.installed ? STATUSES.active : status;
  }
  return STATUSES.installed;
}

// Deterministic synthetic snapshot for a `clientId` when no real adapter
// has phoned home yet. The status arg is the adapter's self-declared
// (or pre-heartbeat) state, so an unknown client with no receipts is
// reported as `installed` rather than coerced to `active` — mirrors the
// supervisor's "no record ⇒ Installed" rule.
export function adapterSnapshot(
  clientId,
  { now = Date.now(), stalenessMs = DEFAULT_STALENESS_MS, status = STATUSES.installed, receipts = [], lastSeen = null, adapterVersion = "0.0.0" } = {},
) {
  if (typeof clientId !== "string" || clientId.length === 0) {
    throw new TypeError("adapterSnapshot requires a non-empty clientId");
  }
  const observedLastSeen = lastSeen == null ? now : Number(lastSeen);
  const installedStyle = (receipts || []).length === 0 && status === STATUSES.installed;
  const computed = installedStyle
    ? STATUSES.installed
    : classifyStatus(
        { status, lastSeen: observedLastSeen, receipts },
        { now, stalenessMs },
      );
  return {
    schema: SCHEMA,
    schemaVersion: SCHEMA_VERSION,
    clientId,
    adapterVersion,
    status: computed,
    lastSeen: observedLastSeen,
    mechanismReceipts: Array.isArray(receipts) ? receipts : [],
  };
}

// MBR-207 acceptance gate for the JS surface. Mirrors the supervisor's
// `HeartbeatTable::detect_conflation`; a snapshot reported as both installed
// and delivering is exactly the conflation the Hub/CLI must refuse.
export function assertNoConflation(snapshot) {
  if (!snapshot || typeof snapshot !== "object") {
    throw new TypeError("assertNoConflation requires a heartbeat snapshot");
  }
  const installed = snapshot.status === STATUSES.installed;
  const delivering = snapshot.status === STATUSES.delivering
    || (Array.isArray(snapshot.mechanismReceipts)
      && snapshot.mechanismReceipts.some((receipt) => Number(receipt?.count) > 0));
  if (installed && delivering) {
    const err = new Error(
      `heartbeat conflation for client ${snapshot.clientId}: installed and delivering coexist`,
    );
    err.code = "heartbeat-conflation";
    err.clientId = snapshot.clientId;
    throw err;
  }
}
