#!/usr/bin/env node
// D09: clean-package test. Verifies the npm tarball contains only intended
// runtime files and works outside the monorepo (help, status, MCP handshake).

import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { npmCliArgs } from "./release/npm-cli.mjs";
import { verifyMcpInitialize } from "./release/mcp-client-smoke.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const pkg = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8"));

const requiredBins = ["blueprint", "blueprint-watch", "blueprint-mcp", "blueprint-install"];
for (const name of requiredBins) {
  if (!pkg.bin?.[name]) throw new Error(`package missing bin ${name}`);
}

function fail(message) {
  console.error(`test-package: ${message}`);
  process.exit(1);
}

// 1. Dry-run pack: file list must be a subset of the explicit `files` allowlist.
const dryRun = spawnSync(process.execPath, npmCliArgs(["pack", "--json", "--dry-run"]), { cwd: ROOT, encoding: "utf8" });
if (dryRun.status !== 0) fail(`npm pack --dry-run failed: ${dryRun.stderr}`);
const packResult = JSON.parse(dryRun.stdout);
const entry = packResult.find((item) => item.filename);
if (!entry) fail("no tarball entry in pack output");
const allowlist = new Set(pkg.files ?? []);
const unexpected = entry.files
  .map((f) => f.path)
  .map((path) => path.replace(/^package\//, ""))
  .filter((path) => {
    // package.json is always present; everything else must fall under an
    // allowlisted file or directory prefix (files entries may end with /).
    if (path === "package.json") return false;
    return ![...allowlist].some((allowed) => path === allowed || path.startsWith(allowed.endsWith("/") ? allowed : `${allowed}/`));
  });
if (unexpected.length) fail(`tarball contains files outside the allowlist: ${unexpected.join(", ")}`);

// 2. Extract a real tarball into a temp dir and install production deps.
const temp = mkdtempSync(join(tmpdir(), "blueprint-test-package-"));
try {
  execFileSync(process.execPath, npmCliArgs(["pack", "--pack-destination", temp]), { cwd: ROOT, stdio: "ignore" });
  const tarball = join(temp, entry.filename);
  if (!existsSync(tarball)) fail(`tarball not produced: ${tarball}`);
  const extractDir = join(temp, "extract");
  // Reference the archive by relative basename with cwd set to the archive
  // directory: MSYS GNU tar on Windows reads drive-letter paths as remote hosts.
  execFileSync("tar", ["-xzf", entry.filename], { cwd: temp, stdio: "ignore" });
  const packageDir = join(temp, "package");
  if (!existsSync(packageDir)) fail("tarball did not extract a package/ dir");

  // 3. Install production dependencies inside the extracted package.
  writeFileSync(join(packageDir, ".npmrc"), "package-lock=false\n");
  const install = spawnSync(process.execPath, npmCliArgs(["install", "--omit=dev", "--no-audit", "--no-fund"]), { cwd: packageDir, encoding: "utf8", timeout: 180000 });
  if (install.status !== 0) fail(`prod install failed: ${install.stderr || install.stdout}`);

  // 4. Help, status, and MCP handshake from the extracted package.
  const help = spawnSync(process.execPath, [join(packageDir, "scripts", "blueprint.mjs"), "--help"], { cwd: packageDir, encoding: "utf8" });
  if (help.status !== 0) fail(`help failed: ${help.stderr}`);
  if (!/Blueprint — repository truth and evidence map/.test(help.stdout)) fail("help is not branded Blueprint");

  try {
    await verifyMcpInitialize({ script: join(packageDir, "scripts", "blueprint-mcp.mjs"), root: packageDir });
  } catch (error) {
    fail(`MCP handshake failed: ${error.message}`);
  }

  console.log(`test-package OK: ${entry.filename} (${entry.files.length} files), prod install + help + MCP handshake clean`);
} finally {
  rmSync(temp, { recursive: true, force: true });
}
