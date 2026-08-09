#!/usr/bin/env node
import { mkdtemp, realpath, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { serveStdio } from "@modelcontextprotocol/server/stdio";
import { buildServer } from "./server.mjs";

const root = await realpath(resolve(fileURLToPath(new URL("..", import.meta.url))));
const registryDir = await mkdtemp(join(tmpdir(), "membrane-rightax-"));
const registry = join(registryDir, "registry.json");
await writeFile(registry, JSON.stringify({ schema_version: 1, bindings: { [root]: { repository_id: "conformance-probe", scope_id: "conformance-probe", provider_config: {}, grant_policy: { level: "write-proposed", durability: "advisory" } } } }), "utf8");
process.env.MEMBRANE_PROJECT_REGISTRY = registry;
process.env.MEMBRANE_DURABILITY_MODE = "advisory";
const shutdown = new AbortController();
const stdio = serveStdio(() => buildServer({ shutdownSignal: shutdown.signal }), { onerror: (error) => process.stderr.write(`membrane-rightax: ${error.message}\n`) });
process.stdin.once("end", () => {
  shutdown.abort(new Error("stdio_closed"));
  void stdio.close().finally(() => rm(registryDir, { recursive: true, force: true }));
});
