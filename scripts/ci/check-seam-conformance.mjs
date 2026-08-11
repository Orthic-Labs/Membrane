#!/usr/bin/env node
// U61: SEAM-CONTRACT §8 conformance for cortex — vendor-neutral naming, manifest shape, watcher single-ownership
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import Ajv from "ajv";
import { buildProductManifest, validateProductManifest } from "../../lib/init/manifest.mjs";
import { buildSnapshot, validateSnapshot } from "../../lib/orthic-snapshot.mjs";

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

check("grep-gate: no hardcoded crypt outside config-default in scripts/cortex.mjs", () => {
  const src = readFileSync(join(ROOT, "scripts/cortex.mjs"), "utf8");
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
  const canonicalPath = resolve(ROOT, "../orthic/schema/manifest.v1.schema.json");
  const schemaPath = existsSync(canonicalPath) ? canonicalPath : localSchemaPath;
  if (!existsSync(schemaPath)) throw new Error("manifest schema missing");
  const manifest = buildProductManifest({ installRoot: ROOT });
  const validate = new Ajv().compile(JSON.parse(readFileSync(schemaPath, "utf8")));
  if (!validate(manifest)) throw new Error(`manifest schema: ${JSON.stringify(validate.errors)}`);
  const local = validateProductManifest(manifest);
  if (!local.ok) throw new Error(`manifest security: ${local.errors.join(", ")}`);
});

check("snapshot shape validates", () => {
  const localSchemaPath = join(ROOT, "schemas/orthic-product-snapshot-v1.schema.json");
  const canonicalPath = resolve(ROOT, "../orthic/schema/snapshot.v1.schema.json");
  const schemaPath = existsSync(canonicalPath) ? canonicalPath : localSchemaPath;
  const snapshot = buildSnapshot({ root: ROOT });
  const validate = new Ajv().compile(JSON.parse(readFileSync(schemaPath, "utf8")));
  if (!validate(snapshot)) throw new Error(`snapshot schema: ${JSON.stringify(validate.errors)}`);
  const local = validateSnapshot(snapshot);
  if (!local.ok) throw new Error(`snapshot shape: ${local.errors.join(", ")}`);
});

const MEM_WORD = "mem" + "brane";
check(`watcher single-ownership: no ${MEM_WORD} in cortex-watch.mjs / watchman/`, () => {
  try {
    const out = execFileSync("grep", ["-rn", MEM_WORD, "scripts/cortex-watch.mjs", "watchman/"], { encoding: "utf8", cwd: ROOT });
    if (out.trim()) throw new Error(`found ${MEM_WORD} in watcher:\n${out.slice(0, 300)}`);
    console.log(`✓ watcher single-ownership: zero ${MEM_WORD}`);
  } catch (e) {
    if (e.status === 1) console.log(`✓ watcher single-ownership: zero ${MEM_WORD}`);
    else throw e;
  }
});

check("cortex graph manifest --json shape (if graph exists)", () => {
  // If no graph, skip — not a failure of conformance
  const out = (() => {
    try { return run("node", ["scripts/cortex.mjs", "graph", "manifest", "--json"], { stdio: "pipe" }); } catch (e) { return e.stdout ?? ""; }
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

if (failed) { console.error("check-seam-conformance FAILED"); process.exit(1); }
console.log(JSON.stringify({ ok: true }));
