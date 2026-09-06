import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { buildGraphGeneration } from "../src/graph/static-provider.mjs";

test("production graph build runs structural/framework/portable/convention layers", () => {
  const root = mkdtempSync(join(tmpdir(), "blueprint-intelligence-build-"));
  try {
    mkdirSync(join(root, "src"), { recursive: true });
    writeFileSync(join(root, "src", "base.ts"), "export class Base { run() {} }\n");
    writeFileSync(join(root, "src", "child.ts"), `import { Base } from './base.js';\nexport class Child extends Base { run() {} }\nconst endpoint = process.env.API_URL;\n`);
    writeFileSync(join(root, "src", "tools.ts"), `export function ping() {}\nmcp.tool("ping", ping);\n`);
    const generation = buildGraphGeneration(root);
    const providers = generation.augmentation?.providers;
    assert.ok(providers?.structuralIntelligence);
    assert.ok(providers?.frameworkIntelligence);
    assert.ok(providers?.portableIdentity);
    assert.equal(providers?.conventions?.policyAuthority, false);
    assert.ok(generation.nodes.some((node) => node.labels?.includes("ConfigKey") && node.name === "API_URL"));
    assert.ok(generation.nodes.some((node) => node.labels?.includes("ToolContract") && node.name === "ping"));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
