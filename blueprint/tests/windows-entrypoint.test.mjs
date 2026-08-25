import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const CMD_PATH = path.join(ROOT, "scripts", "blueprint.cmd");
const FIXTURE = path.join(ROOT, "evals", "fixture-repos", "typescript-commerce");
const CLI = path.join(ROOT, "scripts", "blueprint.mjs");

test("windows entrypoint wrapper is ASCII/newline-safe with a normalized invocation", () => {
  const bytes = fs.readFileSync(CMD_PATH);
  assert.ok(!bytes.subarray(0, 3).equals(Buffer.from([0xef, 0xbb, 0xbf])), "wrapper has no UTF-8 BOM");
  assert.ok(
    [...bytes].every((byte) => byte === 0x09 || byte === 0x0a || byte === 0x0d || (byte >= 0x20 && byte <= 0x7e)),
    "wrapper is pure ASCII",
  );
  const text = bytes.toString("utf8");
  assert.ok(!/(?<!\r)\r(?!\n)/.test(text), "wrapper has no stray carriage returns");
  assert.ok(text.endsWith("\n"), "file ends with a newline");

  // Normalized script-dir resolution: %~dp0 always ends in a backslash, so the
  // wrapper strips it before rejoining, keeping one canonical separator even
  // for install roots with spaces. Arguments pass through untouched (%*) and
  // node's exit code propagates through endlocal via %ERRORLEVEL%.
  assert.ok(text.includes('%~dp0'), "anchors on the wrapper's own directory");
  assert.ok(
    text.includes('if "%SCRIPT_DIR:~-1%"=="\\" set "SCRIPT_DIR=%SCRIPT_DIR:~0,-1%"'),
    "strips the trailing backslash from %~dp0 before joining",
  );
  assert.ok(/node\s+"%SCRIPT_DIR%\\blueprint\.mjs"\s+%*/.test(text), "invokes blueprint.mjs by normalized quoted path with all args");
  assert.ok(/endlocal & exit \/b %ERRORLEVEL%/.test(text), "propagates the node exit code");
});

test("direct .cmd invocation creates expected artifacts", { skip: process.platform !== "win32" }, () => {
  const repo = path.join(os.tmpdir(), `blueprint-cmd-entry-${process.pid}-${Date.now()}`);
  fs.cpSync(FIXTURE, repo, { recursive: true });
  try {
    const OUT = ".cmd-run-out";
    const build = spawnSync("cmd.exe", ["/d", "/c", CMD_PATH, "graph", "build", "--out", OUT], { cwd: repo, encoding: "utf8" });
    assert.equal(build.status, 0, build.stderr || build.stdout);
    assert.ok(fs.existsSync(path.join(repo, OUT, "graph", "graph.db")), "graph store written under explicit output");
    assert.ok(fs.existsSync(path.join(repo, OUT, "config.json")), "output config written under explicit output");

    const projection = spawnSync("cmd.exe", ["/d", "/c", CMD_PATH, "graph", "audit-projection", "--out", OUT], { cwd: repo, encoding: "utf8" });
    assert.equal(projection.status, 0, projection.stderr || projection.stdout);
    const packet = JSON.parse(projection.stdout);
    assert.equal(packet.schema, "membrane.blueprint-packet.v1");
    assert.equal(packet.state, "ready");

    // Exit codes propagate: a typed projection rejection surfaces non-zero.
    fs.writeFileSync(path.join(repo, "drift.js"), "export const drift = 1;\n");
    const stale = spawnSync("cmd.exe", ["/d", "/c", CMD_PATH, "graph", "audit-projection", "--expected-generation", "xxh128:00000000000000000000000000000000", "--out", OUT], { cwd: repo, encoding: "utf8" });
    assert.notEqual(stale.status, 0);
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});
