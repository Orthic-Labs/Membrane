// D07: MCP root confinement — the server root is fixed at process start and
// tool inputs cannot select an arbitrary local directory.

import assert from "node:assert/strict";
import { cpSync, mkdtempSync, rmSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { tmpdir } from "node:os";
import test from "node:test";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
import { buildGraphGeneration } from "../graph/static-provider.mjs";

const ROOT = join(import.meta.dirname, "..");
const SERVER = join(ROOT, "scripts/cortex-mcp.mjs");
const FIXTURE = join(ROOT, "evals/fixture-repos/typescript-commerce");

test("tool inputs cannot select an arbitrary repoRoot", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-mcp-confine-"));
  const elsewhere = mkdtempSync(join(tmpdir(), "cortex-mcp-elsewhere-"));
  cpSync(FIXTURE, repo, { recursive: true });
  buildGraphGeneration(repo, { outDir: ".agent", persist: true });
  const transport = new StdioClientTransport({ command: process.execPath, args: [SERVER, "--root", repo], cwd: repo, stderr: "pipe" });
  const client = new Client({ name: "cortex-test", version: "1.0.0" }, { capabilities: {} });
  try {
    await client.connect(transport);
    await assert.rejects(
      client.callTool({ name: "cortex_status", arguments: { repoRoot: elsewhere } }),
      (error) => error?.code === -32602,
    );
  } finally {
    await client.close().catch(() => {});
    rmSync(repo, { recursive: true, force: true });
    rmSync(elsewhere, { recursive: true, force: true });
  }
});

test("server requires a root at startup", () => {
  const result = spawnSync(process.execPath, [SERVER], { encoding: "utf8", timeout: 5000 });
  assert.notEqual(result.status, 0, "server without --root must exit non-zero");
});
