#!/usr/bin/env node
// Idempotent local MCP registration. It prints intended changes by default;
// --apply writes only user-owned JSON config files.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const apply = process.argv.includes("--apply");
const command = process.env.MEMRIGHT_MCP_COMMAND || process.execPath;
const server = path.resolve(new URL("./server.mjs", import.meta.url).pathname);
const entry = { command, args: [server], env: { MEMRIGHT_PORT: process.env.MEMRIGHT_PORT || "38123" } };
const targets = [
  path.join(os.homedir(), ".claude", "mcp.json"),
  path.join(os.homedir(), ".codex", "mcp.json"),
];

for (const target of targets) {
  let config = {};
  try { config = JSON.parse(fs.readFileSync(target, "utf8")); } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
  config.mcpServers ??= {};
  const next = { ...config, mcpServers: { ...config.mcpServers, memright: entry } };
  process.stdout.write(`${JSON.stringify({ target, apply, changed: JSON.stringify(config) !== JSON.stringify(next) })}\n`);
  if (apply) {
    fs.mkdirSync(path.dirname(target), { recursive: true });
    const temp = `${target}.tmp-${process.pid}`;
    fs.writeFileSync(temp, `${JSON.stringify(next, null, 2)}\n`, "utf8");
    fs.renameSync(temp, target);
  }
}
