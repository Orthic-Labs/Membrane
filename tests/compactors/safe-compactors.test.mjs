import test from "node:test"; import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs"; import { tmpdir } from "node:os";
import { join } from "node:path"; import { spawnSync } from "node:child_process";

const engineRoot = join(process.cwd(), "engine");
const manifest = join(engineRoot, "Cargo.toml");
const canonicalBinary = join(engineRoot, "target", "debug", process.platform === "win32" ? "membrane-compact.exe" : "membrane-compact");

// Walks the engine workspace's own sources (skipping build output and non-build-affecting
// "tests"/"examples"/"benches" targets) for the newest mtime, so an already-built
// membrane-compact binary is reused instead of rebuilt when nothing relevant changed.
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

// Resolves the membrane-compact binary to exercise. Reuses an already-fresh build when possible
// (the common case, and the only path that works when the guarded `cargo` on this host requires
// RIGHT_RELEASE_CACHE_ROOT -- see docs/rules/paid-compute.md). Otherwise attempts a real build and
// locates the produced binary via cargo's own `--message-format=json` artifact report (robust to
// the guard redirecting CARGO_TARGET_DIR into a shared cache). Deliberately does NOT fall back to
// a stale existing binary on build failure: an outdated membrane-compact can have materially
// different (and here, verified weaker) redaction behavior than what this test asserts, so a
// stale substitute could silently mask a real regression. Returns null when nothing fresh is
// available, which causes the real test to be skipped with a clear reason instead of crashing the
// whole file.
function resolveBinary() {
  if (process.env.MEMBRANE_COMPACT_TEST_BIN) return process.env.MEMBRANE_COMPACT_TEST_BIN;
  if (existsSync(canonicalBinary) && statSync(canonicalBinary).mtimeMs >= newestEngineSourceMtimeMs(engineRoot)) return canonicalBinary;
  const build = spawnSync("cargo", ["build", "--quiet", "--message-format=json", "--manifest-path", manifest, "-p", "membrane", "--bin", "membrane-compact"], { encoding: "utf8" });
  if (build.status !== 0) return null;
  const artifact = (build.stdout || "").trim().split("\n").filter(Boolean)
    .map((line) => { try { return JSON.parse(line); } catch { return null; } })
    .reverse().find((message) => message && message.reason === "compiler-artifact" && message.executable);
  if (artifact?.executable && existsSync(artifact.executable)) return artifact.executable;
  return existsSync(canonicalBinary) ? canonicalBinary : null;
}

const binary = resolveBinary();

if (!binary) {
  test("compactors retain raw, redact summary, & fail closed", { skip: "membrane-compact binary unavailable: cargo build failed and no prebuilt binary exists (set RIGHT_RELEASE_CACHE_ROOT on macOS, or pre-build engine/target/debug/membrane-compact)" }, () => {});
} else {
  const invoke = (args) => spawnSync(binary, args, { encoding: "utf8" });
  const compact = (root, kind, script) => invoke(["run", kind, "--raw-root", root, "--max-lines", "1", "--", process.execPath, "-e", script]);
  test("compactors retain raw, redact summary, & fail closed", () => { const root = mkdtempSync(join(tmpdir(), "membrane-compact-fail-"));
    for (const [index, kind] of ["git", "tests", "build", "package", "containers", "logs"].entries()) {
      const secret = ["ghp_abcdefghijklmnopqrstuvwxyz123456", "AKIA1234567890ABCDEF", "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.signature", "Bearer opaque-value"][index % 4], raw = `ERROR ${kind} ${secret}\nall details\n`, run = compact(root, kind, `process.stdout.write(${JSON.stringify(raw)}); process.exit(7)`);
      assert.equal(run.status, 7, run.stderr); const result = JSON.parse(run.stdout);
      assert.equal(result.commandSucceeded, false); assert.equal(result.signalLineCount, 1); assert.equal(result.omittedSignalLineCount, 0);
      assert.equal(result.summary.split("\n").filter(Boolean).length, 1);
      assert.match(result.summary, /REDACTED/); assert.doesNotMatch(result.summary, new RegExp(secret.split(" ")[0])); assert.ok(!result.rawPointer.startsWith("/")); assert.equal(readFileSync(join(root, result.rawPointer), "utf8"), raw);
    } const getRoot = mkdtempSync(join(tmpdir(), "membrane-compact-get-")), result = JSON.parse(compact(getRoot, "tests", "console.log('ERROR exact');").stdout);
    assert.equal(invoke(["get", result.rawDigest, "--raw-root", getRoot]).stdout, "ERROR exact\n"); assert.equal(invoke(["get", "not-a-digest", "--raw-root", getRoot]).status, 2); writeFileSync(join(getRoot, result.rawPointer), "tampered\n"); const get = invoke(["get", result.rawDigest, "--raw-root", getRoot]); assert.equal(get.status, 2); assert.match(get.stderr, /hash mismatch/);
    const flagsRoot = mkdtempSync(join(tmpdir(), "membrane-compact-flags-")); const misordered = invoke(["run", "tests", "--max-lines", "1", "--raw-root", flagsRoot, "--", "missing"]); assert.equal(misordered.status, 2);
    const afterSeparator = invoke(["run", "tests", "--raw-root", flagsRoot, "--", "missing", "--max-lines", "0"]); assert.equal(afterSeparator.status, 2); assert.match(afterSeparator.stderr, /execute missing/);
    for (const args of [["run", "tests", "--raw-root", flagsRoot, "--unknown", "x", "--", "missing"], ["run", "tests", "--raw-root", flagsRoot, "--raw-root", flagsRoot, "--", "missing"]]) assert.equal(invoke(args).status, 2);
    const crowded = JSON.parse(compact(flagsRoot, "tests", "console.log('ERROR one\\nERROR two')").stdout); assert.equal(crowded.summary.trim(), "ERROR one"); assert.equal(crowded.omittedSignalLineCount, 1);
    const overflow = compact(flagsRoot, "logs", "process.stdout.write('x'.repeat(16*1024*1024+1))"); assert.equal(overflow.status, 2); assert.match(overflow.stderr, /command_output_limit_exceeded/);
  });
}
