import assert from "node:assert/strict";
import test from "node:test";
import { existsSync, readdirSync, statSync } from "node:fs";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const engineRoot = join(root, "engine");
const manifest = join(engineRoot, "Cargo.toml");
const canonicalBinary = join(engineRoot, "target", "debug", process.platform === "win32" ? "membrane.exe" : "membrane");
const request = JSON.stringify({ jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "compat", version: "1" } } });
const golden = { protocolVersion: "2025-03-26", name: "membrane" };

// Walks the engine workspace's own sources (skipping build output and non-build-affecting
// "tests"/"examples"/"benches" targets) for the newest mtime, so an already-built `membrane`
// binary is reused instead of rebuilt when nothing relevant changed.
function newestEngineSourceMtimeMs(dir) {
  let newest = 0;
  const stack = [dir];
  while (stack.length) {
    const current = stack.pop();
    let entries;
    try { entries = readdirSync(current, { withFileTypes: true }); } catch { continue; }
    for (const entry of entries) {
      if (["target", "tests", "examples", "benches"].includes(entry.name) || entry.name.startsWith(".")) continue;
      const full = join(current, entry.name);
      if (entry.isDirectory()) { stack.push(full); continue; }
      if (!entry.name.endsWith(".rs") && entry.name !== "Cargo.toml" && entry.name !== "Cargo.lock") continue;
      const mtime = statSync(full).mtimeMs;
      if (mtime > newest) newest = mtime;
    }
  }
  return newest;
}

function invoke(command, args, cwd) {
  return new Promise((resolvePromise, reject) => {
    const child = spawn(command, args, { cwd, stdio: ["pipe", "pipe", "pipe"] });
    let output = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => { output += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.on("error", reject);
    child.on("close", (code) => code === 0 ? resolvePromise(JSON.parse(output.trim().split("\n")[0])) : reject(new Error(`exit ${code}: ${stderr.trim()}`)));
    child.stdin.end(`${request}\n`);
  });
}

// Resolves how to run the native `membrane stdio-mcp` roundtrip: reuse an already-fresh build
// when possible (the common case, and the only path that works when the guarded `cargo` on this
// host requires RIGHT_RELEASE_CACHE_ROOT -- see docs/rules/paid-compute.md); otherwise fall back
// to `cargo run`, which also picks up a source change the freshness check might miss.
async function nativeStdioMcp() {
  if (process.env.MEMBRANE_TEST_BIN) return invoke(process.env.MEMBRANE_TEST_BIN, ["stdio-mcp"], root);
  if (existsSync(canonicalBinary) && statSync(canonicalBinary).mtimeMs >= newestEngineSourceMtimeMs(engineRoot)) {
    return invoke(canonicalBinary, ["stdio-mcp"], root);
  }
  // The "membrane" package declares three [[bin]] targets (membrane, cli-parity,
  // membrane-compact) with no default-run, so `cargo run -p membrane` alone is ambiguous --
  // `--bin membrane` is required to select the right one.
  return invoke("cargo", ["run", "--quiet", "-p", "membrane", "--bin", "membrane", "--", "stdio-mcp"], engineRoot);
}

test("native stdio-mcp responds with the golden membrane server identity", async (t) => {
  let native;
  try {
    native = await nativeStdioMcp();
  } catch (error) {
    t.skip(`native membrane binary unavailable: ${error.message} (set RIGHT_RELEASE_CACHE_ROOT on macOS, or pre-build engine/target/debug/membrane)`);
    return;
  }
  assert.equal(native.result.protocolVersion, golden.protocolVersion);
  assert.equal(native.result.serverInfo.name, golden.name);
});
