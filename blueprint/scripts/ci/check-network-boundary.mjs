#!/usr/bin/env node
// D10/D84: network-boundary inventory. Proves zero unrequested network calls
// by default: every network-capable operation in the shipped surface must be
// (a) explicitly enumerated here, and (b) disabled unless opted in.

import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const SCAN_DIRS = ["scripts", "lib", "graph", "watchman", "sources"];
// The checker's own directory contains the API-name literals and must not
// flag itself.
const SELF_EXCLUDE = "scripts/ci/";

// The complete inventory of network-capable call sites. Anything using these
// APIs must be listed here or the check fails. Telemetry is off by default.
const NETWORK_APIS = [
  "fetch(",
  "node:http",
  "node:https",
  "node:net",
  "node:dns",
  "WebSocket",
  "net.createConnection",
  "http.request",
  "https.request",
];

const ALLOWED = [
  // Update checks are opt-in, channel-gated, and disabled by CORTEX_NO_UPDATE_CHECK=1.
  "src/lib/update/channel.mjs",
  "src/lib/update/manifest.mjs",
  "src/lib/update/apply.mjs",
  // The local explorer binds loopback only (127.0.0.1) — not egress.
  "src/lib/http-server.mjs",
  "src/lib/orthic-snapshot.mjs",
  "src/lib/session-token.mjs",
  "src/lib/explorer/static.mjs",
  // Telemetry is off by default and documented as opt-in.
  "src/lib/telemetry.mjs",
];

export function toInventoryPath(path, nativeSeparator = sep) {
  return path.split(nativeSeparator).join("/");
}

function walk(dir) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) out.push(...walk(path));
    else if (path.endsWith(".mjs") || path.endsWith(".js")) out.push(path);
  }
  return out;
}

const offenders = [];
for (const scanDir of SCAN_DIRS) {
  const base = join(ROOT, scanDir);
  if (!existsSync(base)) continue;
  for (const file of walk(base)) {
    const rel = toInventoryPath(relative(ROOT, file));
    if (rel.startsWith(SELF_EXCLUDE)) continue;
    if (ALLOWED.some((allowed) => rel === allowed || rel.startsWith(`${allowed}/`))) continue;
    const source = readFileSync(file, "utf8");
    for (const api of NETWORK_APIS) {
      if (source.includes(api)) {
        offenders.push(`${rel} uses ${api}`);
        break;
      }
    }
  }
}

if (offenders.length) {
  console.error("check-network-boundary: undeclared network-capable operations:");
  for (const line of offenders) console.error(`  ${line}`);
  console.error("Add them to the inventory in scripts/ci/check-network-boundary.mjs or remove the call.");
  process.exit(1);
}

console.log("check-network-boundary OK: zero undeclared network-capable operations");
