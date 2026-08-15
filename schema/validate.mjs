#!/usr/bin/env node
// Orthic contract validator — Node mirror of src-tauri/src/manifest_validate.rs
// and schema/manifest.v2 + snapshot.v2 + lifecycle.v1 bounds.
//
// Owned by OR-CONTRACTS (schema/**). This is the "Node schema test" the
// dispatch acceptance names: it validates the released JSON-schema fixtures
// and recomputes the bundle digest. It deliberately depends on no third-party
// validator package so it runs anywhere Node 26 runs.
//
// CLI:
//   node schema/validate.mjs --check-fixtures    # validate every schema/fixtures/*.json
//   node schema/validate.mjs --check-bundle      # recompute & compare schema/bundle.json digest
import { createHash } from "node:crypto";
import { readFileSync, readdirSync } from "node:fs";
import { resolve, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const FIXTURES_DIR = join(here, "fixtures");

export const SNAPSHOT_BOUNDS = {
  minSections: 1, maxSections: 16,
  maxItemsPerSection: 1000, maxItemFields: 8, maxItemStringBytes: 512,
  maxLabel: 128, maxKind: 64, maxReason: 200, maxEvidence: 512, maxResolver: 512,
  maxTotalBytes: 65_536,
};
const ALLOWED_ITEM_FIELDS = new Set(["label","kind","count","severity","evidence","resolver","observedAtUnixMs","stale"]);

const isSha256 = (v) => typeof v === "string" && /^sha256:[0-9a-f]{64}$/.test(v);
const isHex64 = (v) => typeof v === "string" && /^[0-9a-f]{64}$/.test(v);
const isLoopback = (h) => h === "127.0.0.1" || h === "::1" || h === "localhost";

// ---- hubCompatRange grammar (mirrors evaluate_hub_compat_range) ----
function parseVersion(text) {
  text = String(text).trim();
  if (!text) return null;
  const parts = text.split(".");
  if (parts.length > 3) return null;
  const out = [0, 0, 0];
  for (let i = 0; i < parts.length; i++) {
    if (parts[i] === "" || !/^\d+$/.test(parts[i])) return null;
    out[i] = Number(parts[i]);
  }
  return out;
}
const cmp = (a, b) => a[0] - b[0] || a[1] - b[1] || a[2] - b[2];
export function evaluateHubCompatRange(range, hubVersion) {
  range = String(range).trim();
  if (range === "" || range === "*") return true;
  const hub = parseVersion(hubVersion);
  if (!hub) return false;
  let any = false;
  for (const raw of range.split(/[,\s]+/)) {
    const token = raw.trim();
    if (!token) continue;
    any = true;
    let op = "", rest = token;
    for (const candidate of [">=", "<=", "==", ">", "<", "="]) {
      if (token.startsWith(candidate)) { op = candidate; rest = token.slice(candidate.length); break; }
    }
    if (!op) return false; // bare version / unsupported future grammar
    const v = parseVersion(rest);
    if (!v) return false;
    const ok = op === ">=" ? cmp(hub, v) >= 0
      : op === "<=" ? cmp(hub, v) <= 0
      : op === ">" ? cmp(hub, v) > 0
      : op === "<" ? cmp(hub, v) < 0
      : cmp(hub, v) === 0;
    if (!ok) return false;
  }
  return any;
}

export function validateManifestV2(value, { hubVersion = "0.1.11" } = {}) {
  if (!value || typeof value !== "object") return "manifest_schema_invalid";
  if (value.schemaVersion !== 2) return "manifest_schema_invalid";
  const known = new Set(["schemaVersion","productId","displayName","productVersion","hubCompatRange","installRoot","serviceStart","serviceStop","icon","artifactDigest"]);
  for (const k of Object.keys(value)) if (!known.has(k)) return "manifest_schema_invalid";
  for (const req of ["productId","displayName","productVersion","hubCompatRange","installRoot","serviceStart","icon","artifactDigest"]) {
    if (value[req] === undefined) return "manifest_schema_invalid";
  }
  if (value.productId !== "cortex" && value.productId !== "membrane") return "manifest_schema_invalid";
  if (typeof value.hubCompatRange !== "string" || value.hubCompatRange === "") return "manifest_schema_invalid";
  if (!evaluateHubCompatRange(value.hubCompatRange, hubVersion)) return "manifest_hub_range_incompatible";
  if (!Array.isArray(value.serviceStart) || value.serviceStart.length === 0) return "manifest_schema_invalid";
  for (const s of value.serviceStart) if (typeof s !== "string") return "manifest_schema_invalid";
  if (!Array.isArray(value.serviceStop)) return "manifest_schema_invalid";
  if (!isSha256(value.artifactDigest)) return "manifest_artifact_digest_invalid";
  return null;
}

const SECTION_STATES = new Set(["available", "degraded", "unavailable"]);
const ITEM_SEVERITIES = new Set(["info", "warning", "error", "critical"]);
function isPrimitive(v) { return v === null || typeof v === "string" || typeof v === "number" || typeof v === "boolean"; }

export function validateSnapshotV2(value) {
  if (!value || typeof value !== "object") return "snapshot_schema_unsupported";
  if (value.schemaVersion !== 2) return "snapshot_schema_unsupported";
  const known = new Set(["schemaVersion","productId","observedAtUnixMs","sections","stale","cacheAgeMs"]);
  for (const k of Object.keys(value)) if (!known.has(k)) return "snapshot_schema_invalid";
  if (value.productId !== "cortex" && value.productId !== "membrane") return "snapshot_schema_invalid";
  // Total payload cap matches the live runtime (hub_runtime MAX_SNAPSHOT_BYTES = 65536).
  const total = Buffer.byteLength(JSON.stringify(value), "utf8");
  if (total > SNAPSHOT_BOUNDS.maxTotalBytes) return "snapshot_too_large";
  const sections = value.sections;
  if (!sections || typeof sections !== "object") return "snapshot_schema_invalid";
  const names = Object.keys(sections);
  if (names.length < SNAPSHOT_BOUNDS.minSections || names.length > SNAPSHOT_BOUNDS.maxSections) return "snapshot_sections_out_of_bounds";
  for (const name of names) {
    const sec = sections[name];
    if (!sec || typeof sec !== "object") return "snapshot_schema_invalid";
    for (const k of Object.keys(sec)) if (!["state","reason","items","evidence","resolver","observedAtUnixMs"].includes(k)) return "snapshot_schema_invalid";
    if (!SECTION_STATES.has(sec.state)) return "snapshot_schema_invalid";
    if (typeof sec.reason !== "string" || sec.reason === "" || sec.reason.length > SNAPSHOT_BOUNDS.maxReason) return "snapshot_section_string_too_long";
    if (sec.items !== undefined) {
      if (!Array.isArray(sec.items) || sec.items.length > SNAPSHOT_BOUNDS.maxItemsPerSection) return "snapshot_items_out_of_bounds";
      for (const item of sec.items) {
        // Items are closed bounded evidence-handle objects; arbitrary maps are forbidden.
        if (!item || typeof item !== "object" || Array.isArray(item)) return "snapshot_item_not_object";
        const keys = Object.keys(item);
        if (keys.length > SNAPSHOT_BOUNDS.maxItemFields) return "snapshot_item_properties_out_of_bounds";
        for (const k of keys) {
          if (!ALLOWED_ITEM_FIELDS.has(k)) return "snapshot_item_field_unknown";
        }
        if (typeof item.label !== "string" || item.label === "" || item.label.length > SNAPSHOT_BOUNDS.maxLabel) return "snapshot_item_string_too_long";
        if (item.kind !== undefined && (typeof item.kind !== "string" || item.kind.length > SNAPSHOT_BOUNDS.maxKind)) return "snapshot_item_string_too_long";
        if (item.count !== undefined && (typeof item.count !== "number" || item.count < 0)) return "snapshot_item_value_not_primitive";
        if (item.severity !== undefined && !ITEM_SEVERITIES.has(item.severity)) return "snapshot_item_value_not_primitive";
        if (item.evidence !== undefined && (typeof item.evidence !== "string" || item.evidence.length > SNAPSHOT_BOUNDS.maxEvidence)) return "snapshot_item_string_too_long";
        if (item.resolver !== undefined && (typeof item.resolver !== "string" || item.resolver.length > SNAPSHOT_BOUNDS.maxResolver)) return "snapshot_item_string_too_long";
        if (item.observedAtUnixMs !== undefined && (!Number.isInteger(item.observedAtUnixMs) || item.observedAtUnixMs < 0)) return "snapshot_item_value_not_primitive";
        if (item.stale !== undefined && typeof item.stale !== "boolean") return "snapshot_item_value_not_primitive";
        // Content-free: no nested objects/arrays as values (beyond the named handled fields above).
        for (const [k, v] of Object.entries(item)) {
          if (!["label","kind","count","severity","evidence","resolver","observedAtUnixMs","stale"].includes(k)) return "snapshot_item_field_unknown";
          if (v !== null && typeof v === "object") return "snapshot_item_value_not_primitive";
        }
      }
    }
    if (sec.evidence !== undefined && (typeof sec.evidence !== "string" || sec.evidence.length > SNAPSHOT_BOUNDS.maxEvidence)) return "snapshot_section_string_too_long";
    if (sec.resolver !== undefined && (typeof sec.resolver !== "string" || sec.resolver.length > SNAPSHOT_BOUNDS.maxResolver)) return "snapshot_section_string_too_long";
  }
  return null;
}

const LIFECYCLE_COMMANDS = new Set(["drain","stop","update_handoff","ownership_loss"]);
const REGISTER_STATES = new Set(["starting","ready","degraded","incompatible","failed"]);
export function validateLifecycleFrame(frame) {
  if (!frame || typeof frame !== "object") return "lifecycle_frame_invalid";
  switch (frame.kind) {
    case "hello": {
      if (frame.lifecycleVersion !== 1) return "lifecycle_version_unsupported";
      for (const req of ["installationId","productId","instanceId","fence","artifactDigest","declaredDataRoot","secret"]) if (frame[req] === undefined) return "hello_field_missing";
      if (frame.productId !== "cortex" && frame.productId !== "membrane") return "hello_product_invalid";
      if (!Number.isInteger(frame.fence) || frame.fence < 1) return "hello_fence_invalid";
      if (!isSha256(frame.artifactDigest)) return "hello_digest_invalid";
      if (!isHex64(frame.secret)) return "hello_secret_invalid";
      return null;
    }
    case "register": {
      if (!REGISTER_STATES.has(frame.state)) return "register_state_invalid";
      if (!Number.isInteger(frame.fence) || frame.fence < 1) return "register_fence_invalid";
      if (frame.state === "ready") {
        if (!frame.endpoint || !isLoopback(frame.endpoint.host) || !Number.isInteger(frame.endpoint.port) || frame.endpoint.port < 1) return "endpoint_not_loopback";
        if (!frame.capability || typeof frame.capability !== "string") return "ready_requires_endpoint_and_capability";
      }
      if (frame.endpoint && !isLoopback(frame.endpoint.host)) return "endpoint_not_loopback";
      return null;
    }
    case "command":
      if (!LIFECYCLE_COMMANDS.has(frame.command)) return "unknown_lifecycle_command";
      if (!Number.isInteger(frame.fence) || frame.fence < 1) return "command_fence_invalid";
      return null;
    case "ack":
      if (!LIFECYCLE_COMMANDS.has(frame.command)) return "unknown_lifecycle_command";
      if (!Number.isInteger(frame.fence) || frame.fence < 1) return "ack_fence_invalid";
      return null;
    default:
      return "unknown_lifecycle_frame";
  }
}

// Materialize an adversarial fixture's "instance"/"frame" applying _prefixed
// construction hints (e.g. oversized strings) so the fixtures file stays
// human-readable while still exercising byte caps.
function materializeAdversarial(caseObj, kind) {
  const target = kind === "lifecycle" ? caseObj.frame : caseObj.instance;
  const out = structuredClone(target);
  if (kind === "snapshot") {
    if (caseObj._labelLength) out.sections.x.items[0].label = "X".repeat(caseObj._labelLength);
    if (caseObj._reasonLength) out.sections.x.reason = "X".repeat(caseObj._reasonLength);
    if (caseObj._evidenceLength) out.sections.x.items[0].evidence = "E".repeat(caseObj._evidenceLength);
  }
  return out;
}

function loadJson(rel) { return JSON.parse(readFileSync(resolve(here, rel), "utf8")); }

export function checkFixtures() {
  const errors = [];
  const manifestValid = loadJson("fixtures/manifest-valid.json");
  if (validateManifestV2(manifestValid.instance)) errors.push("manifest-valid fixture rejected");
  const manifestAdv = loadJson("fixtures/manifest-adversarial.json");
  for (const c of manifestAdv.cases) {
    const err = validateManifestV2(c.instance, { hubVersion: c.hubVersion });
    if (c.expect === "accept") { if (err) errors.push(`manifest ${c.id}: expected accept got ${err}`); }
    else if (!err) errors.push(`manifest ${c.id}: expected reject, accepted`);
  }
  const snapshotValid = loadJson("fixtures/snapshot-valid.json");
  if (validateSnapshotV2(snapshotValid.instance)) errors.push("snapshot-valid fixture rejected");
  const snapshotAdv = loadJson("fixtures/snapshot-adversarial.json");
  for (const c of snapshotAdv.cases) {
    const inst = materializeAdversarial(c, "snapshot");
    const err = validateSnapshotV2(inst);
    if (c.expect === "accept") { if (err) errors.push(`snapshot ${c.id}: expected accept got ${err}`); }
    else if (!err) errors.push(`snapshot ${c.id}: expected reject, accepted`);
  }
  const lifecycleValid = loadJson("fixtures/lifecycle-valid.json");
  for (const f of lifecycleValid.frames) if (validateLifecycleFrame(f)) errors.push(`lifecycle valid frame ${f.kind} rejected`);
  const lifecycleAdv = loadJson("fixtures/lifecycle-adversarial.json");
  for (const c of lifecycleAdv.cases) {
    const err = validateLifecycleFrame(c.frame);
    if (c.expect === "accept") { if (err) errors.push(`lifecycle ${c.id}: expected accept got ${err}`); }
    else if (!err) errors.push(`lifecycle ${c.id}: expected reject, accepted`);
  }
  return errors;
}

const BUNDLE_FILES = [
  "manifest.v2.schema.json", "manifest.v2.ts",
  "snapshot.v2.schema.json", "snapshot.v2.ts",
  "lifecycle.v1.schema.json", "lifecycle.v1.ts",
  "manifest.v1.schema.json", "manifest.v1.ts", "snapshot.v1.schema.json", "snapshot.v1.ts",
  "MIGRATION.md", "validate.mjs", "schema.test.mjs",
];

export function computeBundleDigest() {
  const hasher = createHash("sha256");
  const files = [];
  for (const rel of readdirSync(FIXTURES_DIR).filter((f) => f.endsWith(".json"))) files.push(`fixtures/${rel}`);
  for (const f of [...BUNDLE_FILES, ...files.sort()]) hasher.update(`${f}\0`);
  for (const f of [...BUNDLE_FILES, ...files.sort()]) hasher.update(readFileSync(join(here, f)));
  hasher.update("\0canonical-orthic-contract-bundle-v1");
  return "sha256:" + hasher.digest("hex");
}

export function checkBundle() {
  const errors = [];
  const bundle = loadJson("bundle.json");
  const expected = computeBundleDigest();
  if (bundle.bundleVersion !== 1) errors.push("bundle.bundleVersion must be 1");
  if (bundle.digest !== expected) errors.push(`bundle digest mismatch: recorded ${bundle.digest} recomputed ${expected}`);
  if (!bundle.unsupportedFutureRefusal) errors.push("bundle must declare unsupportedFutureRefusal");
  if (!Array.isArray(bundle.migration) || bundle.migration.length === 0) errors.push("bundle must record migration rule");
  return errors;
}

async function main() {
  const arg = process.argv[2];
  if (arg === "--check-fixtures") {
    const errors = checkFixtures();
    if (errors.length) { for (const e of errors) console.error(`fixtures FAIL: ${e}`); process.exit(1); }
    console.log("fixtures OK: manifest + snapshot + lifecycle valid/adversarial all conform"); return;
  }
  if (arg === "--check-bundle") {
    const errors = checkBundle();
    if (errors.length) { for (const e of errors) console.error(`bundle FAIL: ${e}`); process.exit(1); }
    console.log("bundle OK: digest verified"); return;
  }
  console.error("usage: node schema/validate.mjs [--check-fixtures | --check-bundle]");
  process.exit(2);
}
const invoked = import.meta.url === `file://${process.argv[1]}`;
if (invoked) main();