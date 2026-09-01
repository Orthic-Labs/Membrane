// Runtime capability checks.
//
// `engines.node` in package.json pins the supported range, but engines is
// advisory — pnpm warns, then runs anyway on an older runtime. A user who
// runs `blueprint build` on Node 20 would see the WASM hash-wasm dependency
// load OK but `node:sqlite` (DatabaseSync) throw ERR_UNKNOWN_BUILTIN_MODULE
// with no actionable message. Emitting `unsupported_node_runtime` on startup
// gives the failure a code, a codex-style breadcrumb, and a clear remediation
// ("upgrade to Node >=22.22.3"). The capability record is also emitted as a
// structured field so a downstream consumer can decide to degrade rather
// than fail.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));

// Parse the supported range out of package.json. Done at import time so the
// comparison is against the version the module was bundled with, not a
// hardcoded literal that can desync if the engine floor moves.
let SUPPORTED_RANGE = ">=22.22.3";
try {
  const pkgPath = resolve(__dirname, "..", "..", "package.json");
  const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
  if (pkg?.engines?.node) SUPPORTED_RANGE = pkg.engines.node;
} catch {
  // Best-effort: fall back to the in-source default so a malformed
  // package.json still produces a useful error.
}

function parseMajorMinor(version) {
  const match = String(version ?? "").match(/^v?(\d+)\.(\d+)(?:\.(\d+))?/);
  if (!match) return null;
  return { major: Number(match[1]), minor: Number(match[2]), patch: Number(match[3] ?? 0) };
}

// Minimal semver floor parser: supports `>=X.Y.Z`, `>=X.Y`, `>=X` and ignores
// pre-release tags. A real semver lib is overkill for a single hard floor.
function supportsCurrent(version, range) {
  const parsed = parseMajorMinor(version);
  if (!parsed) return false;
  const match = String(range ?? "").match(/>=\s*(\d+)(?:\.(\d+))?(?:\.(\d+))?/);
  if (!match) return false;
  const minMajor = Number(match[1]);
  const minMinor = Number(match[2] ?? 0);
  const minPatch = Number(match[3] ?? 0);
  if (parsed.major !== minMajor) return parsed.major > minMajor;
  if (parsed.minor !== minMinor) return parsed.minor > minMinor;
  return parsed.patch >= minPatch;
}

const NODE_VERSION = process.version;
const SUPPORTED = supportsCurrent(NODE_VERSION, SUPPORTED_RANGE);

export const RUNTIME_CAPABILITIES = Object.freeze({
  schemaVersion: 1,
  nodeVersion: NODE_VERSION,
  supportedRange: SUPPORTED_RANGE,
  supported: SUPPORTED,
  // Capability flags — kept as booleans so a consumer can degrade rather
  // than crash on a missing feature. Adding a flag here is a contract.
  capabilities: Object.freeze({
    nodeSqlite: SUPPORTED, // `node:sqlite` ships in 22.5+; stable floor is 22.22.3.
    topLevelAwait: true,    // Node 14.8+; harmless to assert.
    sqliteWAL: true,        // Node 22+ exposes the WAL busy_timeout knob.
  }),
});

/**
 * Returns `{ ok: true }` when the runtime meets the floor, otherwise
 * `{ ok: false, error: { code: "unsupported_node_runtime", ... } }`.
 * Callers can choose to print the structured payload and exit non-zero, or
 * proceed with degraded capability.
 */
export function checkRuntime() {
  if (SUPPORTED) return { ok: true, capabilities: RUNTIME_CAPABILITIES };
  return {
    ok: false,
    capabilities: RUNTIME_CAPABILITIES,
    error: {
      code: "unsupported_node_runtime",
      message: `Node ${NODE_VERSION} is below the supported floor (${SUPPORTED_RANGE}). Upgrade Node and re-run.`,
      nodeVersion: NODE_VERSION,
      supportedRange: SUPPORTED_RANGE,
      remediation: `install Node ${SUPPORTED_RANGE} (Node 22 LTS or 24+ recommended) and re-run the same command`,
    },
  };
}

/**
 * Convenience wrapper: checks the runtime and exits the process with a
 * structured JSON payload when it does not meet the floor. Designed for the
 * top of every entry script so the failure surfaces before any work begins.
 */
export function requireRuntime() {
  const result = checkRuntime();
  if (!result.ok) {
    process.stderr.write(`${JSON.stringify({ schemaVersion: 1, error: result.error })}\n`);
    process.exit(2);
  }
  return result;
}
