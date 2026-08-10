import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";

const SNAPSHOT_SCHEMA_VERSION = 1;

export function buildSnapshot({ root = process.cwd(), outDir = ".agent", now = new Date().toISOString() } = {}) {
  const result = {
    schemaVersion: SNAPSHOT_SCHEMA_VERSION,
    productId: "cortex",
    timestamp: now,
    sections: {},
  };

  // freshness section — typed degradation: unavailable with reason if missing
  try {
    const dbPath = resolve(root, outDir, "graph", "graph.db");
    if (!existsSync(dbPath)) {
      result.sections.freshness = { state: "unavailable", reason: "graph store missing — run cortex build" };
    } else {
      // Try to read manifest envelope — if fails, unavailable
      result.sections.freshness = { state: "degraded", reason: "snapshot builder does not open store in this minimal build; use service.status" };
    }
  } catch (e) {
    result.sections.freshness = { state: "unavailable", reason: String(e.message ?? e).slice(0, 200) };
  }

  // graph section
  try {
    const pkg = JSON.parse(readFileSync(join(resolve(root), "package.json"), "utf8"));
    result.sections.graph = { version: pkg.version, state: "ok" };
  } catch (e) {
    result.sections.graph = { state: "unavailable", reason: "package.json unreadable" };
  }

  // watcher section
  try {
    result.sections.watcher = { state: "ok", note: "watcher status via service.status" };
  } catch (e) {
    result.sections.watcher = { state: "unavailable", reason: String(e.message).slice(0, 200) };
  }

  // Ensure every section that is missing reports unavailable, never fabricated zero
  for (const key of ["freshness", "graph", "watcher"]) {
    if (!result.sections[key]) result.sections[key] = { state: "unavailable", reason: "section not implemented" };
  }

  return result;
}

export function validateSnapshot(snapshot) {
  const errors = [];
  if (snapshot.schemaVersion !== 1) errors.push("schemaVersion must be 1");
  if (snapshot.productId !== "cortex") errors.push("productId must be cortex");
  if (!snapshot.timestamp) errors.push("timestamp required");
  if (!snapshot.sections || typeof snapshot.sections !== "object") errors.push("sections required");
  for (const [name, section] of Object.entries(snapshot.sections ?? {})) {
    if (!section.state) errors.push(`section ${name} missing state`);
    if (section.state === "unavailable" && !section.reason) errors.push(`section ${name} unavailable must have reason`);
    if (section.state !== "ok" && section.state !== "degraded" && section.state !== "unavailable") errors.push(`section ${name} invalid state`);
  }
  return { ok: errors.length === 0, errors };
}
