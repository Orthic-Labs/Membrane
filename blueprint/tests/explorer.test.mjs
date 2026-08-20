import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { startExplorerServer } from "../src/lib/http-server.mjs";
import { serveExplorerAsset } from "../src/lib/explorer/static.mjs";

test("explorer shell boots without exposing API", async () => {
  const service = { status: async () => ({ state: "fresh" }), architecture: async () => ({ nodes: [] }) };
  const server = await startExplorerServer({ service, repoRoot: process.cwd(), serveAsset: serveExplorerAsset });
  try {
    const base = server.url.split("#")[0].replace(/\/$/, "");
    const shell = await fetch(`${base}/`);
    assert.equal(shell.status, 200);
    assert.match(await shell.text(), /Blueprint Explorer/);
    assert.equal((await fetch(`${base}/api/status`)).status, 401);
    const token = server.url.split("#token=")[1];
    assert.equal((await fetch(`${base}/api/status`, { headers: { authorization: `Bearer ${token}` } })).status, 200);
  } finally { await server.close(); }
});

test("standalone explorer never launches a token-bearing browser child", () => {
  const source = readFileSync(new URL("../src/lib/explorer/index.mjs", import.meta.url), "utf8");
  assert.doesNotMatch(source, /spawn\(|execFile|xdg-open|\/c.*, *start/);
});
