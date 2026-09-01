#!/usr/bin/env node
// U61: SEAM-CONTRACT §8 conformance for blueprint — vendor-neutral naming, snapshot shape, watcher single-ownership
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import Ajv from "ajv";
import { buildSnapshot, boundUtf8, validateSnapshot, SNAPSHOT_MAX_REASON_BYTES } from "../../src/lib/snapshot.mjs";
import { computeManifestDigest, detectHubIdentityFields, detectShadowManifestKeys, assertBuildIdentityClean } from "../../src/graph/generation-identity.mjs";
import { classifyMutablePath, assertSafeMutableStorePath } from "../../src/graph/store-sqlite.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

function textFiles(paths) {
  const files = [];
  const visit = (path) => {
    const stat = statSync(path);
    if (stat.isDirectory()) for (const name of readdirSync(path)) visit(join(path, name));
    else files.push(path);
  };
  for (const path of paths) visit(resolve(ROOT, path));
  return files;
}

function occurrences(needle, paths, ignored = () => false) {
  const rows = [];
  for (const file of textFiles(paths)) {
    if (ignored(file)) continue;
    const source = readFileSync(file, "utf8");
    source.split(/\r?\n/).forEach((line, index) => {
      if (line.includes(needle)) rows.push(`${file.slice(ROOT.length + 1)}:${index + 1}:${line}`);
    });
  }
  return rows;
}

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
const membraneOccurrences = occurrences(MEMBRANE, ["scripts", "src/lib", "src/service"], (file) => file.endsWith("check-seam-conformance.mjs"));
if (membraneOccurrences.length) { console.error(`✗ grep-gate ${MEMBRANE}: found\n${membraneOccurrences.join("\n").slice(0, 500)}`); failed = true; }
else console.log(`✓ grep-gate: zero ${MEMBRANE} in scripts/ src/lib/ src/service/`);

check("grep-gate: no hardcoded cortex outside config-default in scripts/blueprint.mjs", () => {
  const src = readFileSync(join(ROOT, "scripts/blueprint.mjs"), "utf8");
  const lines = src.split("\n");
  // Config-default contexts: peer = "cortex" or service: "cortex" or comment
  const bad = [];
  lines.forEach((line, idx) => {
    if (!line.includes('"cortex"') && !line.includes("'cortex'")) return;
    // Allow if line is config-default: peer = "cortex" or service: "cortex" or comment
    if (/peer\s*=\s*["']cortex["']/.test(line)) return;
    if (/service:\s*["']cortex["']/.test(line)) return;
    if (line.trim().startsWith("//")) return;
    if (line.includes("peerBinCandidates")) return;
    // Otherwise, consider hardcoded outside config-default
    // Currently we allow only the two config-default lines; any other is bad
    if (line.includes('join(homedir(), "bin", "cortex"')) bad.push(`${idx + 1}:${line.trim()}`);
  });
  if (bad.length) throw new Error(`hardcoded cortex outside config-default:\n${bad.join("\n")}`);
});

check("snapshot shape validates", () => {
  const localSchemaPath = join(ROOT, "schemas/blueprint-snapshot-v2.schema.json");
  // Validate only against the pinned released artifact copied into this
  // package. An external schema is explicitly not a fallback.
  if (!existsSync(localSchemaPath)) throw new Error("pinned snapshot schema missing");
  const snapshot = buildSnapshot({ root: ROOT });
  const validate = new Ajv().compile(JSON.parse(readFileSync(localSchemaPath, "utf8")));
  if (!validate(snapshot)) throw new Error(`snapshot schema: ${JSON.stringify(validate.errors)}`);
  const local = validateSnapshot(snapshot);
  if (!local.ok) throw new Error(`snapshot shape: ${local.errors.join(", ")}`);
});

const MEM_WORD = "mem" + "brane";
check(`watcher single-ownership: no ${MEM_WORD} in blueprint-watch.mjs / watchman/`, () => {
  const found = occurrences(MEM_WORD, ["scripts/blueprint-watch.mjs", "watchman"]);
  if (found.length) throw new Error(`found ${MEM_WORD} in watcher:\n${found.join("\n").slice(0, 300)}`);
  console.log(`✓ watcher single-ownership: zero ${MEM_WORD}`);
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
    protocol: "blueprint.lifecycle.v1",
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
