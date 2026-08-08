import assert from "node:assert/strict";
import { cp, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { verifyServerJson } from "../../scripts/release/registry/verify-server-json.mjs";
import { computeToolContractCoverage } from "../../scripts/release/registry/tool-contract-coverage.mjs";

const source = await readFile(new URL("../../server.json", import.meta.url), "utf8");

async function fixture(change) {
  const directory = await mkdtemp(join(tmpdir(), "membrane-mcp-registry-server-json-"));
  await cp(new URL("../../npm", import.meta.url), join(directory, "npm"), { recursive: true });
  const server = JSON.parse(source);
  change(server);
  await writeFile(join(directory, "server.json"), JSON.stringify(server));
  return directory;
}

test("real server.json's tools field matches the live TOOLS + OPERATIONS derivation exactly", async () => {
  const expected = computeToolContractCoverage();
  const server = JSON.parse(source);
  assert.deepEqual(server.tools, expected);
});

test("unpublished checkout fails closed even though tool coverage is correct (honest pre-release state)", async () => {
  await assert.rejects(
    () => verifyServerJson({ directory: new URL("../..", import.meta.url).pathname }),
    /unpublished/,
  );
});

test("with requirePublished:false, the real repo's identity and tool coverage both verify", async () => {
  const result = await verifyServerJson({ directory: new URL("../..", import.meta.url).pathname, requirePublished: false });
  assert.equal(result.name, "io.github.orthic-labs/membrane");
  assert.ok(Array.isArray(result.tools));
  assert.equal(result.tools.length, computeToolContractCoverage().length);
});

test("a missing tools array is rejected", async () => {
  const directory = await fixture((server) => {
    delete server.tools;
  });
  await assert.rejects(() => verifyServerJson({ directory, requirePublished: false }), /missing a `tools` array/);
});

test("a tool list missing a real tool (understating the surface) is rejected", async () => {
  const directory = await fixture((server) => {
    server.tools = server.tools.filter((tool) => tool.name !== "membrane_cortex");
  });
  await assert.rejects(() => verifyServerJson({ directory, requirePublished: false }), /declares 9 tool\(s\); the live server \+ operations registry has 10/);
});

test("silently upgrading the known gap to operations_registry coverage is rejected", async () => {
  const directory = await fixture((server) => {
    const cortex = server.tools.find((tool) => tool.name === "membrane_cortex");
    cortex.contractCoverage = "operations_registry";
    delete cortex.gapReason;
  });
  await assert.rejects(
    () => verifyServerJson({ directory, requirePublished: false }),
    /tool "membrane_cortex" must declare contractCoverage "gap"/,
  );
});

test("a gap entry without a gapReason is rejected", async () => {
  const directory = await fixture((server) => {
    const cortex = server.tools.find((tool) => tool.name === "membrane_cortex");
    delete cortex.gapReason;
  });
  await assert.rejects(() => verifyServerJson({ directory, requirePublished: false }), /must carry a non-empty gapReason/);
});

test("a contract-covered tool carrying a stray gapReason is rejected", async () => {
  const directory = await fixture((server) => {
    const context = server.tools.find((tool) => tool.name === "membrane_context");
    context.gapReason = "not actually a gap";
  });
  await assert.rejects(() => verifyServerJson({ directory, requirePublished: false }), /must not carry a gapReason/);
});

test("an invented tool name not in the live server is rejected", async () => {
  const directory = await fixture((server) => {
    server.tools[0] = { name: "membrane_fabricated_tool", contractCoverage: "operations_registry" };
  });
  await assert.rejects(() => verifyServerJson({ directory, requirePublished: false }), /tool\[0\] must be/);
});
