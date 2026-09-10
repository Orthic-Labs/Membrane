#!/usr/bin/env node
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const seedText = process.env.MEMBRANE_RANDOM_SEED ?? "1597463007";
const seed = Number(seedText);
if (!Number.isSafeInteger(seed) || seed < 0 || seed > 0xffffffff) {
  throw new Error("MEMBRANE_RANDOM_SEED must be an unsigned 32-bit integer");
}

// Observable-event construction & validation are native membrane-mcp code.
// Keep this CI entrypoint as a small Rust-focused runner so legacy host JS is
// not loaded merely to exercise its former randomized suite.
const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const command = process.platform === "win32" ? "rightkit.cmd" : "cargo";
const args = process.platform === "win32"
  ? ["cargo", "test", "--manifest-path", "engine/Cargo.toml", "--target", "x86_64-pc-windows-msvc", "-p", "membrane-mcp", "--locked", "--test", "parity_host_observable_event"]
  : ["test", "--manifest-path", "engine/Cargo.toml", "-p", "membrane-mcp", "--locked", "--test", "parity_host_observable_event"];
const result = spawnSync(command, args, { cwd: repoRoot, stdio: "inherit", windowsHide: true, shell: process.platform === "win32" });
assert.equal(result.status, 0, `native membrane-mcp observable-event tests failed (status ${result.status})`);
console.log(`native observable-event suite OK: seed=${seed}`);
