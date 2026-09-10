#!/usr/bin/env node
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const ingressSource = readFileSync(resolve(repoRoot, "engine", "crates", "membrane-mcp", "src", "host_observable_ingress.rs"), "utf8");
const eventSource = readFileSync(resolve(repoRoot, "engine", "crates", "membrane-mcp", "src", "host_observable_event.rs"), "utf8");

assert.doesNotMatch(ingressSource, /std::net|Tcp(?:Listener|Stream)|node:(?:net|http|https)|\bfetch\s*\(|\bspawn(?:Sync)?\s*\(/);
assert.doesNotMatch(eventSource, /node:(?:net|http|https)|\bfetch\s*\(|\bspawn(?:Sync)?\s*\(/);

const command = process.platform === "win32" ? "rightkit.cmd" : "cargo";
const cargoPrefix = process.platform === "win32" ? ["cargo"] : [];
for (const target of ["parity_host_observable_ingress", "parity_host_observable_ingress_default_path"]) {
  const args = [...cargoPrefix, "test", "--manifest-path", "engine/Cargo.toml", ...(process.platform === "win32" ? ["--target", "x86_64-pc-windows-msvc"] : []), "-p", "membrane-mcp", "--locked", "--test", target];
  const result = spawnSync(command, args, { cwd: repoRoot, stdio: "inherit", windowsHide: true, shell: process.platform === "win32" });
  assert.equal(result.status, 0, `native membrane-mcp ${target} failed (status ${result.status})`);
}

console.log("network boundary OK: native ingress stays local, loopback-only, and file-backed");
