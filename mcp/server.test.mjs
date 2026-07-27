import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

const server = fileURLToPath(new URL("./server.mjs", import.meta.url));
const child = spawn(process.execPath, [server], { stdio: ["pipe", "pipe", "pipe"], windowsHide: true });
let output = "";
child.stdout.on("data", (chunk) => { output += chunk; });
child.stdin.end([
  JSON.stringify({ jsonrpc: "2.0", id: 1, method: "initialize", params: {} }),
  JSON.stringify({ jsonrpc: "2.0", id: 2, method: "tools/list", params: {} }),
  JSON.stringify({ jsonrpc: "2.0", id: 3, method: "resources/read", params: { uri: "membrane://protocol/v1" } }),
].join("\n") + "\n");
await new Promise((resolve, reject) => { child.once("error", reject); child.once("close", resolve); });
const rows = output.trim().split("\n").map(JSON.parse);
assert.match(rows[0].result.instructions, /federated context/i);
const tools = rows[1].result.tools.map((tool) => tool.name).sort();
assert.deepEqual(tools, ["membrane_checkpoint_load", "membrane_checkpoint_save", "membrane_context", "membrane_feedback", "membrane_knowledge_propose", "membrane_source_read"]);
assert.match(rows[2].result.contents[0].text, /not raw recall/i);
