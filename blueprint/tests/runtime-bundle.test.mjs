// D14: portable runtime bundle contract — self-contained layout, launchers
// resolve bundle-relative paths, and the staged app runs without system Node
// on PATH (simulated by pointing PATH at /usr/bin:/bin).

import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

import { stageRuntime } from "../scripts/release/stage-runtime.mjs";

const ROOT = join(import.meta.dirname, "..");
const LAUNCHERS = join(ROOT, "release", "launchers");
const runtimeName = process.platform === "win32" ? "lib/node.exe" : "lib/node";

function stagedBundle() {
  const out = mkdtempSync(join(tmpdir(), "blueprint-runtime-test-"));
  const result = stageRuntime({ out });
  return { out, result };
}

test("stageRuntime produces the S-12 layout", () => {
  const { out, result } = stagedBundle();
  try {
    for (const path of ["bin/blueprint", "bin/blueprint.cmd", "bin/blueprint-mcp", runtimeName, "app/package/scripts/blueprint.mjs", "app/schemas", "LICENSE", "README.txt"]) {
      assert.ok(existsSync(join(out, path)), `missing ${path}`);
    }
    assert.equal(result.version, "0.2.0");
    assert.ok(result.layout.includes("app/package/node_modules"));
    assert.equal(result.layout.includes("app/node_modules"), false);
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
});

test("stageRuntime rejects a nonempty output without modifying it", () => {
  const out = mkdtempSync(join(tmpdir(), "blueprint-runtime-test-"));
  const sentinel = join(out, "sentinel.txt");
  try {
    writeFileSync(sentinel, "preserve");
    assert.throws(() => stageRuntime({ out }), /empty directory/);
    assert.equal(readFileSync(sentinel, "utf8"), "preserve");
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
});

test("launchers are present and executable", () => {
  const { out } = stagedBundle();
  try {
    for (const name of ["blueprint", "blueprint.cmd", "blueprint.ps1", "blueprint-mcp"]) {
      assert.ok(existsSync(join(LAUNCHERS, name)), `missing launcher ${name}`);
    }
    if (process.platform !== "win32") {
      for (const name of ["blueprint", "blueprint-mcp"]) {
        const mode = statSync(join(out, "bin", name)).mode;
        assert.ok(mode & 0o111, `staged ${name} not executable`);
      }
    }
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
});

test("bundled launcher runs help without system Node on PATH", () => {
  if (process.platform === "win32") {
    const { out } = stagedBundle();
    try {
      const result = spawnSync(process.env.ComSpec ?? "cmd.exe", ["/d", "/c", `call "${join(out, "bin", "blueprint.cmd")}" --help`], { env: { ...process.env, PATH: "" }, encoding: "utf8", windowsVerbatimArguments: true });
      assert.equal(result.status, 0, result.stderr || result.stdout);
      assert.match(result.stdout, /Blueprint — repository truth and evidence map/);
    } finally { rmSync(out, { recursive: true, force: true }); }
    return;
  }
  const { out } = stagedBundle();
  try {
    const result = spawnSync(join(out, "bin", "blueprint"), ["--help"], { env: { ...process.env, PATH: "/usr/bin:/bin" }, encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr || result.stdout);
    assert.match(result.stdout, /Blueprint — repository truth and evidence map/);
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
});

test("bundled app has grammars staged from the production install", () => {
  const { out } = stagedBundle();
  try {
    const grammarDir = join(out, "app", "grammars");
    assert.ok(existsSync(grammarDir), "grammars dir missing");
    const count = readdirSync(grammarDir).filter((name) => name.endsWith(".wasm")).length;
    assert.ok(count > 0, "no WASM grammars staged");
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
});
