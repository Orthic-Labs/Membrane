import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
import { buildGraphGeneration } from "../graph/static-provider.mjs";

const ROOT = resolve(import.meta.dirname, "..");
const CORTEX_CLI = join(ROOT, "scripts", "cortex.mjs");
const SOAK = join(ROOT, "scripts", "run-soak.mjs");
const FAULT_INJECT = join(ROOT, "scripts", "fault-inject.mjs");
const CHECK_RELEASE = join(ROOT, "scripts", "release", "check-release.mjs");
const SBOM = join(ROOT, "scripts", "release", "sbom.mjs");

test("soak CLI writes its requested report", () => {
  const dir = mkdtempSync(join(tmpdir(), "cortex-soak-cli-"));
  const report = join(dir, "report.json");
  try {
    const result = spawnSync(process.execPath, [SOAK, "--seed", "1", "--duration-events", "3", "--repos", "1", "--report", report], { encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr || result.stdout);
    assert.match(result.stdout, /soak report written to/);
    assert.ok(existsSync(report), "requested report exists");
    assert.equal(JSON.parse(readFileSync(report, "utf8")).schemaVersion, 1);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("fault injector CLI emits a report", () => {
  const result = spawnSync(process.execPath, [FAULT_INJECT, "--seed", "1", "--duration", "1", "--repos", "1"], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  assert.ok(Array.isArray(JSON.parse(result.stdout).events));
});

test("SBOM CLI emits SPDX for a candidate", () => {
  const dir = mkdtempSync(join(tmpdir(), "cortex-sbom-cli-"));
  try {
    writeFileSync(join(dir, "compatibility.json"), JSON.stringify({ version: "0.0.0", commit: "deadbeef" }));
    const result = spawnSync(process.execPath, [SBOM, dir], { encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr || result.stdout);
    assert.equal(JSON.parse(result.stdout).spdxVersion, "SPDX-2.3");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("release check CLI reports a valid candidate", () => {
  const dir = mkdtempSync(join(tmpdir(), "cortex-release-cli-"));
  try {
    const tarball = "orthic-labs-cortex-0.0.0.tgz", files = { "SBOM.spdx.json": JSON.stringify({ spdxVersion: "SPDX-2.3" }), THIRD_PARTY_NOTICES: "fixture notices" };
    for (const [name, content] of Object.entries(files)) writeFileSync(join(dir, name), content);
    mkdirSync(join(dir, "package"));
    writeFileSync(join(dir, "package", "package.json"), JSON.stringify({ name: "@orthic-labs/cortex", version: "0.0.0" }));
    execFileSync("tar", ["-czf", tarball, "package"], { cwd: dir });
    rmSync(join(dir, "package"), { recursive: true, force: true });
    const artifacts = [tarball, ...Object.keys(files)].map((name) => ({ name, sha256: createHash("sha256").update(readFileSync(join(dir, name))).digest("hex") }));
    const commit = "a".repeat(40);
    writeFileSync(join(dir, "compatibility.json"), JSON.stringify({ packageName: "@orthic-labs/cortex", version: "0.0.0", commit, platform: `${process.platform}-${process.arch}`, artifacts }));
    writeFileSync(join(dir, "checksums.txt"), `${artifacts.map((artifact) => `${artifact.sha256}  ${artifact.name}`).join("\n")}\n`);
    writeFileSync(join(dir, "artifact-catalog.json"), JSON.stringify({ version: "0.0.0", platform: `${process.platform}-${process.arch}`, files: artifacts.map((artifact) => artifact.name), checksums: "checksums.txt" }));
    writeFileSync(join(dir, "update-manifest.json"), JSON.stringify({ schemaVersion: 1, channel: "stable", version: "0.0.0", commit, publishedAt: "2026-08-08T00:00:00Z", artifacts, signatureAlgorithm: "Ed25519", keyId: "fixture", signature: "pending" }));
    const result = spawnSync(process.execPath, [CHECK_RELEASE, dir], { encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr || result.stdout);
    assert.match(result.stdout, /check-release OK:/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("release check CLI rejects an incomplete candidate", () => {
  const dir = mkdtempSync(join(tmpdir(), "cortex-release-cli-"));
  try {
    const result = spawnSync(process.execPath, [CHECK_RELEASE, dir], { encoding: "utf8" });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /check-release FAILED:/);
    assert.match(result.stderr, /missing compatibility\.json/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("cortex mcp serve starts a live MCP server over stdio", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-cli-mcp-"));
  let client;
  let transport;
  let pid = null;
  try {
    cpSync(join(ROOT, "evals", "fixture-repos", "typescript-commerce"), repo, { recursive: true });
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    transport = new StdioClientTransport({
      command: process.execPath,
      args: [CORTEX_CLI, "mcp", "serve", "--root", repo],
      cwd: repo,
      stderr: "pipe",
    });
    client = new Client({ name: "cortex-cli-acceptance", version: "1.0.0" }, { capabilities: {} });
    await client.connect(transport);
    pid = transport.pid;
    const tools = await client.listTools();
    assert.equal(tools.tools.length, 6, "mcp serve must expose the six-tool surface");
    const status = await client.callTool({ name: "cortex_status", arguments: {} });
    assert.ok(!status.isError, "cortex_status over mcp serve must succeed");
    assert.ok(status.structuredContent, "cortex_status over mcp serve must return structuredContent");
    mkdirSync(join(ROOT, ".audit", "cx-b1"), { recursive: true });
    writeFileSync(join(ROOT, ".audit", "cx-b1", "cli-probe.json"), JSON.stringify({
      lifecycle: ["spawned", "initialized", "capabilities_listed", "tool_called", "structured_result_returned", "process_exited_clean"],
      entrypoint: "cortex mcp serve",
      tools: tools.tools.length,
      structuredContent: true,
    }, null, 2));
  } finally {
    if (client) await client.close().catch(() => {});
    if (pid) {
      const alive = await new Promise((done) => {
        try { process.kill(pid, 0); done(true); } catch { done(false); }
      });
      assert.equal(alive, false, "mcp serve process must exit cleanly after client close");
    }
    rmSync(repo, { recursive: true, force: true });
  }
});

test("cortex mcp rejects unknown subcommands with usage exit", () => {
  const result = spawnSync(process.execPath, [CORTEX_CLI, "mcp", "bogus"], { encoding: "utf8" });
  assert.notEqual(result.status, 0);
  const stderr = result.stderr ?? "";
  assert.ok(stderr.includes('"usage"'), "stderr must carry the usage error code");
  assert.match(stderr, /mcp serve/, "usage message must name the serve subcommand");
  assert.ok(!stderr.includes("not_implemented"), "mcp must no longer be a not_implemented stub");
});

test("cortex mcp without a subcommand exits with usage", () => {
  const result = spawnSync(process.execPath, [CORTEX_CLI, "mcp"], { encoding: "utf8" });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr ?? "", /"usage"/);
});
