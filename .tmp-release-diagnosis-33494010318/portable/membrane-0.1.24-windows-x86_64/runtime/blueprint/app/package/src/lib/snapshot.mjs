import http from "node:http";
import { timingSafeEqual, randomBytes } from "node:crypto";
import { graphStatus } from "../graph/static-provider.mjs";
import { serviceStatus } from "../service/status.mjs";

// The suite contract is blueprint.snapshot.v2. v1 is not selected implicitly;
// callers needing historical data must pass an explicit versioned adapter.
export const SNAPSHOT_SCHEMA_VERSION = 2;

// SEAM-CONTRACT §4.3: bounded, content-free payload. These caps fix section
// count, item count, string bytes, and total bytes so a snapshot can never
// carry source text, secrets, arbitrary maps, or an unbounded item list.
export const SNAPSHOT_MAX_SECTIONS = 16;
export const SNAPSHOT_MAX_ITEMS_PER_SECTION = 1000;
export const SNAPSHOT_MAX_REASON_BYTES = 200;
export const SNAPSHOT_MAX_EVIDENCE_BYTES = 512;
export const SNAPSHOT_MAX_TOTAL_BYTES = 64 * 1024;

// Truncate a UTF-8 string to at most `maxBytes` without splitting a multi-byte
// code point; the ellipsis signals truncation rather than silent loss.
export function boundUtf8(value, maxBytes) {
  const text = String(value ?? "");
  if (Buffer.byteLength(text, "utf8") <= maxBytes) return text;
  // Reserve the UTF-8 width of the truncation marker before slicing. Without
  // this reservation a bounded field could exceed its contract by 3 bytes.
  const marker = "…";
  const markerBytes = Buffer.byteLength(marker, "utf8");
  const contentLimit = Math.max(0, maxBytes - markerBytes);
  let end = Math.min(text.length, contentLimit);
  while (end > 0 && Buffer.byteLength(text.slice(0, end), "utf8") > contentLimit) end -= 1;
  return `${text.slice(0, end)}${marker}`;
}

function boundItems(items, maxItems = SNAPSHOT_MAX_ITEMS_PER_SECTION) {
  return (Array.isArray(items) ? items : []).slice(0, maxItems);
}

function section(state, reason, extra = {}) {
  const result = { state, reason: boundUtf8(reason, SNAPSHOT_MAX_REASON_BYTES), ...extra };
  if (result.items !== undefined) result.items = boundItems(result.items);
  if (result.evidence !== undefined) result.evidence = boundUtf8(result.evidence, SNAPSHOT_MAX_EVIDENCE_BYTES);
  if (result.resolver !== undefined) result.resolver = boundUtf8(result.resolver, SNAPSHOT_MAX_EVIDENCE_BYTES);
  return result;
}

function graphSection(root, outDir) {
  try {
    const status = graphStatus(root, outDir);
    if (status.state === "fresh") return section("available", "graph_fresh", { evidence: status.manifest?.generationId ?? "generation_present" });
    if (status.state === "stale") return section("degraded", "graph_stale", { evidence: status.manifest?.generationId ?? "generation_present" });
    if (status.state === "indeterminate") return section("degraded", "graph_scan_indeterminate");
    if (status.state === "incomplete") return section("degraded", "graph_generation_incomplete");
    if (status.state === "corrupt") return section("degraded", "graph_store_corrupt");
    return section("unavailable", "graph_store_missing");
  } catch (error) {
    return section("unavailable", "graph_status_unreadable", { evidence: String(error?.message ?? error).slice(0, 200) });
  }
}

function watcherSection() {
  try {
    const status = serviceStatus();
    const enrolledCount = Array.isArray(status.enrolledRepos) ? status.enrolledRepos.length : 0;
    return status.running
      ? section("available", "watcher_running", { items: [{ enrolledCount }] })
      : section("unavailable", "watcher_not_running", { items: [{ enrolledCount }] });
  } catch (error) {
    return section("unavailable", "watcher_status_unreadable", { evidence: String(error?.message ?? error).slice(0, 200) });
  }
}

export function buildSnapshot({ root = process.cwd(), outDir = ".agent", now = Date.now() } = {}) {
  const observedAtUnixMs = now instanceof Date ? now.getTime() : Number(now);
  return {
    schemaVersion: SNAPSHOT_SCHEMA_VERSION,
    productId: "blueprint",
    observedAtUnixMs: Number.isFinite(observedAtUnixMs) && observedAtUnixMs >= 0 ? Math.trunc(observedAtUnixMs) : Date.now(),
    sections: {
      graph: graphSection(root, outDir),
      watcher: watcherSection(),
    },
  };
}

function headerValue(request, header) {
  const value = request.headers[header.toLowerCase()];
  if (typeof value !== "string") return "";
  return header.toLowerCase() === "authorization" ? value.replace(/^Bearer\s+/i, "") : value;
}

function constantTimeEqual(left, right) {
  const a = Buffer.from(left);
  const b = Buffer.from(right);
  return a.length === b.length && timingSafeEqual(a, b);
}

export async function startSnapshotServer({ root = process.cwd(), outDir = ".agent", host = "127.0.0.1", port = 0, authHeader = "Authorization", authToken = randomBytes(32).toString("base64url") } = {}) {
  if (host !== "127.0.0.1") throw new Error("snapshot server must bind to 127.0.0.1");
  const server = http.createServer((request, response) => {
    const send = (status, payload) => {
      const body = JSON.stringify(payload);
      response.writeHead(status, { "content-type": "application/json; charset=utf-8", "content-length": Buffer.byteLength(body), "cache-control": "no-store", "x-content-type-options": "nosniff" });
      response.end(body);
    };
    if (request.method !== "GET") return send(405, { error: { code: "method_not_allowed" } });
    if (request.url !== "/snapshot") return send(404, { error: { code: "route_not_found" } });
    if (!constantTimeEqual(headerValue(request, authHeader), authToken)) return send(401, { error: { code: "unauthorized" } });
    return send(200, buildSnapshot({ root, outDir }));
  });
  await new Promise((resolveServer, reject) => {
    server.once("error", reject);
    server.listen(port, host, resolveServer);
  });
  const address = server.address();
  return Object.freeze({
    host,
    port: address.port,
    authHeader,
    authToken,
    close: () => new Promise((resolveClose, reject) => server.close((error) => error ? reject(error) : resolveClose())),
  });
}

export function validateSnapshot(snapshot) {
  const errors = [];
  if (snapshot.schemaVersion !== SNAPSHOT_SCHEMA_VERSION) errors.push(`schemaVersion must be ${SNAPSHOT_SCHEMA_VERSION}`);
  if (snapshot.productId !== "blueprint") errors.push("productId must be blueprint");
  if (!Number.isInteger(snapshot.observedAtUnixMs) || snapshot.observedAtUnixMs < 0) errors.push("observedAtUnixMs required");
  if (!snapshot.sections || typeof snapshot.sections !== "object") errors.push("sections required");
  const sectionEntries = Object.entries(snapshot.sections ?? {});
  if (sectionEntries.length > SNAPSHOT_MAX_SECTIONS) errors.push(`sections exceed ${SNAPSHOT_MAX_SECTIONS}`);
  for (const [name, value] of sectionEntries) {
    if (!value.state) errors.push(`section ${name} missing state`);
    if (!value.reason) errors.push(`section ${name} missing reason`);
    if (!["available", "degraded", "unavailable"].includes(value.state)) errors.push(`section ${name} invalid state`);
    if (Array.isArray(value.items) && value.items.length > SNAPSHOT_MAX_ITEMS_PER_SECTION) errors.push(`section ${name} items exceed ${SNAPSHOT_MAX_ITEMS_PER_SECTION}`);
    if (value.reason && Buffer.byteLength(String(value.reason), "utf8") > SNAPSHOT_MAX_REASON_BYTES) errors.push(`section ${name} reason exceeds ${SNAPSHOT_MAX_REASON_BYTES} bytes`);
    if (value.evidence && Buffer.byteLength(String(value.evidence), "utf8") > SNAPSHOT_MAX_EVIDENCE_BYTES) errors.push(`section ${name} evidence exceeds ${SNAPSHOT_MAX_EVIDENCE_BYTES} bytes`);
  }
  try {
    const bytes = Buffer.byteLength(JSON.stringify(snapshot), "utf8");
    if (bytes > SNAPSHOT_MAX_TOTAL_BYTES) errors.push(`snapshot total bytes ${bytes} exceed ${SNAPSHOT_MAX_TOTAL_BYTES}`);
  } catch {
    errors.push("snapshot is not JSON-serializable");
  }
  return { ok: errors.length === 0, errors };
}
