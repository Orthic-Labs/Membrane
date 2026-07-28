import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { fileURLToPath } from "node:url";

const client = fileURLToPath(new URL("./client.mjs", import.meta.url));
const sourceGeneration = `sha256:${"a".repeat(64)}`;
const largeText = "x".repeat(128 * 1024);
const response = JSON.stringify({
  providerStatus: "ready",
  fallbackMode: "none",
  degradationReason: "none",
  sourceGeneration,
  packet: { blocks: [{ text: largeText }] },
  receipts: [],
});

const server = createServer((request, reply) => {
  request.resume();
  reply.writeHead(200, {
    "content-type": "application/json",
    "content-length": Buffer.byteLength(response),
  });
  reply.end(response);
});

await new Promise((resolve, reject) => {
  server.once("error", reject);
  server.listen(0, "127.0.0.1", resolve);
});

try {
  const { port } = server.address();
  const child = spawn(process.execPath, [client, "--input", "-"], {
    stdio: ["pipe", "pipe", "pipe"],
    windowsHide: true,
    env: {
      ...process.env,
      MEMRIGHT_PORT: String(port),
      MEMRIGHT_API_TOKEN: "test-token",
      WORKSPACE_ROOT: process.cwd(),
    },
  });
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (chunk) => { stdout += chunk; });
  child.stderr.on("data", (chunk) => { stderr += chunk; });
  child.stdin.end(JSON.stringify({ task: "large response", repo: process.cwd() }));
  const code = await new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("close", resolve);
  });

  assert.equal(code, 0, stderr);
  let parsed;
  try {
    parsed = JSON.parse(stdout);
  } catch {
    assert.fail(`client emitted truncated JSON (${Buffer.byteLength(stdout)} bytes)`);
  }
  assert.equal(parsed.packet.blocks[0].text.length, largeText.length);
} finally {
  await new Promise((resolve) => server.close(resolve));
}
