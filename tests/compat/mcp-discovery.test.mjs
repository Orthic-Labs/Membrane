import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const request = JSON.stringify({ jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "compat", version: "1" } } });
const golden = { protocolVersion: "2025-03-26", name: "membrane" };

function invoke(command, args, cwd) {
  return new Promise((resolvePromise, reject) => {
    const child = spawn(command, args, { cwd, stdio: ["pipe", "pipe", "pipe"] });
    let output = "";
    child.stdout.on("data", (chunk) => { output += chunk; });
    child.on("error", reject);
    child.on("close", (code) => code === 0 ? resolvePromise(JSON.parse(output.trim().split("\n")[0])) : reject(new Error(`exit ${code}`)));
    child.stdin.end(`${request}\n`);
  });
}

const native = await invoke("cargo", ["run", "-p", "membrane", "--", "stdio-mcp"], resolve(root, "engine"));
assert.equal(native.result.protocolVersion, golden.protocolVersion);
assert.equal(native.result.serverInfo.name, golden.name);
