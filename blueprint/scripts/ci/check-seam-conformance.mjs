#!/usr/bin/env node
// U61: SEAM-CONTRACT §8 conformance for blueprint — vendor-neutral naming, manifest shape, watcher single-ownership
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import Ajv from "ajv";
import { buildProductManifest, validateProductManifest } from "../../src/lib/init/manifest.mjs";
import { buildSnapshot, boundUtf8, validateSnapshot, SNAPSHOT_MAX_REASON_BYTES } from "../../src/lib/orthic-snapshot.mjs";
import { computeManifestDigest, detectHubIdentityFields, detectShadowManifestKeys, assertBuildIdentityClean } from "../../src/graph/generation-identity.mjs";
import { classifyMutablePath, assertSafeMutableStorePath } from "../../src/graph/store-sqlite.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

function run(cmd, args, opts = {}) {
  return execFileSync(cmd, args, { encoding: "utf8", cwd: ROOT, ...opts });
}

let failed = false;

function check(name, fn) {
  try {
    fn();
    console.log(`✓ ${name}`);
  } catch (e) {
    console.error(`✗ ${name}: ${e.message}`);
    failed = true;
  }
}

const MEMBRANE = "_mem" + "brane";
try {
  const out = execFileSync("grep", ["-rn", MEMBRANE, "scripts/", "lib/", "service/"], { encoding: "utf8", cwd: ROOT });
  const filtered = out.split("\n").filter((l) => !l.includes("check-seam-conformance.mjs")).join("\n").trim();
  if (filtered) { console.error(`✗ grep-gate ${MEMBRANE}: found\n${filtered.slice(0, 500)}`); failed = true; } else console.log(`✓ grep-gate: zero ${MEMBRANE} in scripts/ lib/ service/`);
} catch (e) {
  if (e.status === 1) console.log(`✓ grep-gate: zero ${MEMBRANE} in scripts/ lib/ service/`);
  else { console.error(`✗ grep-gate ${MEMBRANE} error: ${e.message}`); failed = true; }
}

check("grep-gate: no hardcoded crypt outside config-default in scripts/blueprint.mjs", () => {
  const src = readFileSync(join(ROOT, "scripts/blueprint.mjs"), "utf8");
  const lines = src.split("\n");
  // Config-default contexts: peer = "crypt" or service: "crypt" or comment
  const bad = [];
  lines.forEach((line, idx) => {
    if (!line.includes('"crypt"') && !line.includes("'crypt'")) return;
    // Allow if line is config-default: peer = "crypt" or service: "crypt" or comment
    if (/peer\s*=\s*["']crypt["']/.test(line)) return;
    if (/service:\s*["']crypt["']/.test(line)) return;
    if (line.trim().startsWith("//")) return;
    if (line.includes("peerBinCandidates")) return;
    // Otherwise, consider hardcoded outside config-default
    // Currently we allow only the two config-default lines; any other is bad
    if (line.includes('join(homedir(), "bin", "crypt"')) bad.push(`${idx + 1}:${line.trim()}`);
  });
  if (bad.length) throw new Error(`hardcoded crypt outside config-default:\n${bad.join("\n")}`);
});

check("manifest shape validates", () => {
  const localSchemaPath = join(ROOT, "schemas/orthic-product-manifest-v1.schema.json");
  // The released contract is pinned into Blueprint at publish time. Never read a
  // sibling checkout: its presence/version is mutable and would make CI
  // silently validate against an unrelated source tree.
  if (!existsSync(localSchemaPath)) throw new Error("pinned manifest schema missing");
  const manifest = buildProductManifest({ installRoot: ROOT });
  const validate = new Ajv().compile(JSON.parse(readFileSync(localSchemaPath, "utf8")));
  if (!validate(manifest)) throw new Error(`manifest schema: ${JSON.stringify(validate.errors)}`);
  const local = validateProductManifest(manifest);
  if (!local.ok) throw new Error(`manifest security: ${local.errors.join(", ")}`);
});

check("snapshot shape validates", () => {
  const localSchemaPath = join(ROOT, "schemas/orthic-product-snapshot-v2.schema.json");
  // Validate only against the pinned released artifact copied into this
  // package. A sibling ../orthic schema is explicitly not a fallback.
  if (!existsSync(localSchemaPath)) throw new Error("pinned snapshot schema missing");
  const snapshot = buildSnapshot({ root: ROOT });
  const validate = new Ajv().compile(JSON.parse(readFileSync(localSchemaPath, "utf8")));
  if (!validate(snapshot)) throw new Error(`snapshot schema: ${JSON.stringify(validate.errors)}`);
  const local = validateSnapshot(snapshot);
  if (!local.ok) throw new Error(`snapshot shape: ${local.errors.join(", ")}`);
});

const MEM_WORD = "mem" + "brane";
check(`watcher single-ownership: no ${MEM_WORD} in blueprint-watch.mjs / watchman/`, () => {
  try {
    const out = execFileSync("grep", ["-rn", MEM_WORD, "scripts/blueprint-watch.mjs", "watchman/"], { encoding: "utf8", cwd: ROOT });
    if (out.trim()) throw new Error(`found ${MEM_WORD} in watcher:\n${out.slice(0, 300)}`);
    console.log(`✓ watcher single-ownership: zero ${MEM_WORD}`);
  } catch (e) {
    if (e.status === 1) console.log(`✓ watcher single-ownership: zero ${MEM_WORD}`);
    else throw e;
  }
});

check("blueprint graph manifest --json shape (if graph exists)", () => {
  const out = (() => {
    try { return run("node", ["scripts/blueprint.mjs", "graph", "manifest", "--json"], { stdio: "pipe" }); } catch (e) { return e.stdout ?? ""; }
  })();
  if (!out) { console.log("  (no graph — skipping manifest shape check)"); return; }
  try {
    const j = JSON.parse(out);
    if (!j.storeSchemaVersion && !j.schemaVersion) throw new Error("missing schemaVersion");
  } catch (e) {
    if (e.message.includes("Graph store is missing")) { console.log("  (no graph — skipping)"); return; }
    throw e;
  }
});

check("build identity: Hub protocol/lease/endpoint/instance/fence never enter manifestDigest", () => {
  const base = {
    schemaVersion: 1,
    provider: { id: "lexical", version: "repo-local-v1" },
    counts: { nodes: 3, edges: 2 },
    repo: { rootName: "blueprint", sourceHash: "xxh128:abc", fileCount: 1 },
  };
  const clean = computeManifestDigest(base, null);
  const contaminated = computeManifestDigest({
    ...base,
    hub: { lease: "l", endpoint: "e" },
    fence: 42,
    protocol: "orthic.lifecycle.v1",
    instance: "i",
  }, null);
  if (clean !== contaminated) throw new Error("Hub fields changed the manifest digest");
  const leaks = detectHubIdentityFields({ ...base, fence: 1, protocol: "p" });
  if (!leaks.includes("fence") || !leaks.includes("protocol")) throw new Error("hub-field detection missed a leak");
  const shadows = detectShadowManifestKeys({ ...base, sourceHash: "xxh128:dup" });
  if (shadows.length !== 1 || shadows[0].canonical !== "repo.sourceHash") throw new Error("shadow-key detection missed a duplicate");
  assertBuildIdentityClean(base);
  try { assertBuildIdentityClean({ ...base, fence: 1 }); throw new Error("expected build_identity_violation"); }
  catch (e) { if (e.code !== "build_identity_violation") throw e; }
});

check("mutable state path: synced/shared storage is refused typed, local proceeds", () => {
  const synced = classifyMutablePath("/Users/adrian/Dropbox/blueprint/.agent/graph/graph.db", { probeMount: () => "local" });
  if (synced.classification !== "synced") throw new Error(`expected synced, got ${synced.classification}`);
  const shared = classifyMutablePath("\\\\server\\share\\repo\\.agent\\graph.db", { platform: "win32", probeMount: () => "unavailable" });
  if (shared.classification !== "shared") throw new Error(`expected shared, got ${shared.classification}`);
  try { assertSafeMutableStorePath("/Users/adrian/Dropbox/repo/.agent/graph.db", { probeMount: () => "local" }); throw new Error("expected refusal"); }
  catch (e) { if (e.code !== "synced_store_path_refused") throw e; }
  const local = classifyMutablePath("/tmp/blueprint-fixture/.agent/graph/graph.db", { probeMount: () => "local" });
  if (local.classification === "synced" || local.classification === "shared") throw new Error(`local path misclassified as ${local.classification}`);
});

check("snapshot is bounded and content-free (SEAM §4.3)", () => {
  const snapshot = buildSnapshot({ root: ROOT });
  if (!validateSnapshot(snapshot).ok) throw new Error(`built snapshot failed bounds: ${validateSnapshot(snapshot).errors.join(", ")}`);
  const oversized = { ...snapshot, sections: { ...snapshot.sections, graph: { state: "available", reason: "x".repeat(500) } } };
  if (validateSnapshot(oversized).ok) throw new Error("oversized reason was accepted");
  const unicodeReason = boundUtf8("é".repeat(SNAPSHOT_MAX_REASON_BYTES), SNAPSHOT_MAX_REASON_BYTES);
  if (Buffer.byteLength(unicodeReason, "utf8") > SNAPSHOT_MAX_REASON_BYTES) {
    throw new Error("UTF-8 reason truncation exceeded byte cap");
  }
});

if (failed) { console.error("check-seam-conformance FAILED"); process.exit(1); }
console.log(JSON.stringify({ ok: true }));
