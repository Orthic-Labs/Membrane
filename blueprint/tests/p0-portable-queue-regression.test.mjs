import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { bootstrapFromTracked } from "../src/graph/bootstrap.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const CORTEX = path.resolve(HERE, "..");
const CLI = path.join(CORTEX, "scripts/cortex.mjs");
const FIXTURE = path.join(CORTEX, "evals/fixture-repos/typescript-commerce");

function copyFixture(prefix) {
  const repo = fs.mkdtempSync(path.join(os.tmpdir(), `cortex-${prefix}-`));
  fs.cpSync(FIXTURE, repo, { recursive: true });
  return repo;
}

function build(repo) {
  const result = spawnSync(process.execPath, [CLI, "build", "--out", ".agent"], {
    cwd: repo,
    encoding: "utf8",
  });
  assert.equal(result.status, 0, result.stderr || result.stdout);
}

test("build-produced portable manifest bootstraps its graph generation", () => {
  const repo = copyFixture("p0-bootstrap");
  try {
    build(repo);
    const manifestPath = path.join(repo, ".agent", "manifest.json");
    const graphPath = path.join(repo, ".agent", "graph", "graph.db");
    const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));

    assert.equal(manifest.artifacts.graph, ".agent/graph/graph.db");
    assert.ok(fs.existsSync(graphPath), "build must produce the graph SQLite artifact");

    const bootstrap = bootstrapFromTracked(repo);
    assert.equal(bootstrap.state, "ready", JSON.stringify(bootstrap));
    assert.equal(bootstrap.descriptor.contentHash, manifest.generation.contentHash);
    assert.equal(bootstrap.descriptor.contentHash, manifest.repo.sourceHash);
    assert.equal(bootstrap.descriptor.id, manifest.generation.id);
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("queue anchors prioritize central task-linked code over a larger isolated file", () => {
  const repo = copyFixture("p0-queue");
  try {
    fs.appendFileSync(path.join(repo, "docs", "ARCHITECTURE.md"), "\nOrder behavior is implemented by `src/service.ts`.\n");
    // `src/service.ts` is document-linked and sits between routes and storage in
    // the graph. The oversized file is intentionally isolated from that flow.
    fs.writeFileSync(path.join(repo, "src", "zzz-large-isolated.ts"), `// ${"x".repeat(80_000)}\nexport const isolated = true;\n`);
    fs.writeFileSync(path.join(repo, "src", "dependency-a.ts"), "export function dependencyA() { return 1; }\n");
    fs.writeFileSync(path.join(repo, "src", "dependency-b.ts"), "export function dependencyB() { return 2; }\n");
    fs.writeFileSync(
      path.join(repo, "src", "hub.ts"),
      "export class Hub {\n  run() {\n    return dependencyA() + dependencyB();\n  }\n}\n",
    );
    for (let index = 0; index < 14; index += 1) {
      fs.writeFileSync(
        path.join(repo, "src", `decoy-${String(index).padStart(2, "0")}.ts`),
        `// ${"d".repeat(2_000)}\nexport const decoy${index} = ${index};\n`,
      );
    }

    build(repo);
    const queue = JSON.parse(fs.readFileSync(path.join(repo, ".agent", "queue.json"), "utf8"));

    assert.ok(queue.anchors.includes("src/service.ts"), "central task-linked service must be an anchor");
    assert.ok(queue.anchors.includes("src/hub.ts"), "nested symbol call edges must count toward file centrality");
    assert.ok(!queue.anchors.includes("src/zzz-large-isolated.ts"), "size alone must not select an isolated anchor");
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});
