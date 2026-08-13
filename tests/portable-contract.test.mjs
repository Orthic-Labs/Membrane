// G1 portable-contract and bootstrap regression tests.
//
// Covers the dispatch G1 acceptance criteria for:
//   - `.audit/` never appearing in graph nodes
//   - bootstrap-from-tracked behavior with missing/ready/stale/corrupt states
//   - bootstrap rejects manifests that bake in absolute machine paths
//   - collision-safe IDs when duplicate file/symbol slugs would otherwise clash
//   - doctor states (missing/stale/degraded/ready) with machine-readable reasons
//   - tracked .agent/ portable contract works without machine-specific paths

import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

import {
  buildGraphGeneration,
  scanSourcesPublic as scanSources,
  sourceHashPublic as sourceHash,
} from "../graph/static-provider.mjs";
import {
  AGENT_DIR_NAME,
  MANIFEST_NAME,
  bootstrapFromTracked,
} from "../graph/bootstrap.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const CORTEX = path.resolve(HERE, "..");
const FIXTURE = path.join(CORTEX, "evals/fixture-repos/typescript-commerce");

function copyFixture() {
  const dir = path.join(os.tmpdir(), `cortex-portable-${process.pid}-${Date.now()}`);
  fs.cpSync(FIXTURE, dir, { recursive: true });
  return dir;
}

function writeJson(filePath, value) {
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, `${JSON.stringify(value, null, 2)}\n`);
}

test("discovery excludes .audit/ trees so they cannot become source nodes", () => {
  const repo = copyFixture();
  try {
    const auditDir = path.join(repo, ".audit", "session-1");
    fs.mkdirSync(auditDir, { recursive: true });
    fs.writeFileSync(path.join(auditDir, "index.json"), '{"noise":1}');
    fs.writeFileSync(path.join(auditDir, "graph.json"), '{"more":2}');

    const sources = scanSources(repo);
    const auditedPaths = sources.files
      .map((file) => file.path)
      .filter((p) => p.includes(".audit"));
    assert.equal(auditedPaths.length, 0, `.audit/ leaked into discovery: ${auditedPaths.join(", ")}`);
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("discovery excludes cache, worktree metadata, and build output directories", () => {
  const repo = copyFixture();
  try {
    for (const dir of [
      ".cache",
      ".pytest_cache",
      "node_modules",
      "dist",
      "out",
      "__pycache__",
      ".next",
      ".parcel-cache",
      ".worktrees",
    ]) {
      const fullDir = path.join(repo, dir);
      fs.mkdirSync(fullDir, { recursive: true });
      fs.writeFileSync(path.join(fullDir, "marker.txt"), "noise");
    }

    const sources = scanSources(repo);
    const leaked = sources.files
      .map((file) => file.path)
      .filter((p) =>
        [
          ".cache",
          ".pytest_cache",
          "node_modules",
          "dist",
          "out",
          "__pycache__",
          ".next",
          ".parcel-cache",
          ".worktrees",
        ].some((needle) => p.split(path.sep).includes(needle))
      );
    assert.equal(leaked.length, 0, `machine-local dirs leaked into discovery: ${leaked.join(", ")}`);
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("two same-slug files receive different stable IDs (regression fixture)", () => {
  const repo = copyFixture();
  try {
    // Create two distinct README.md files at different paths to confirm IDs differ.
    const aDir = path.join(repo, "packages/a");
    const bDir = path.join(repo, "packages/b");
    fs.mkdirSync(aDir, { recursive: true });
    fs.mkdirSync(bDir, { recursive: true });
    fs.writeFileSync(path.join(aDir, "README.md"), "A");
    fs.writeFileSync(path.join(bDir, "README.md"), "B");

    const sources = scanSources(repo);
    const readmePaths = sources.files
      .filter((file) => file.path.endsWith("README.md"))
      .map((file) => file.path)
      .sort();
    assert.equal(readmePaths.length, 2, "two README.md files expected");

    const generation = buildGraphGeneration(repo);
    const readmeIds = generation.nodes
      .filter((node) => readmePaths.includes(node.path ?? node.evidence?.[0]?.path))
      .map((node) => node.id);
    // Both nodes exist; their IDs differ by file path, so no collision.
    const uniqueIds = new Set(readmeIds);
    assert.equal(uniqueIds.size, readmeIds.length, "duplicate IDs across same-slug files");

    const generation2 = buildGraphGeneration(repo);
    const readmeIds2 = generation2.nodes
      .filter((node) => readmePaths.includes(node.path ?? node.evidence?.[0]?.path))
      .map((node) => node.id)
      .sort();
    assert.deepEqual(
      readmeIds2,
      readmeIds.sort(),
      "two rebuilds from identical inputs must produce equivalent node IDs",
    );
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("bootstrap returns missing when no tracked portable contract exists", () => {
  const repo = copyFixture();
  try {
    const result = bootstrapFromTracked(repo);
    assert.equal(result.state, "missing");
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("bootstrap returns ready when the tracked manifest matches the current source tree", () => {
  const repo = copyFixture();
  try {
    const sources = scanSources(repo);
    const hash = sourceHash(sources.files);
    const manifest = {
      schemaVersion: 1,
      provider: "cortex-static",
      generatedAt: "2026-07-12T17:00:00Z",
      complete: true,
      generation: {
        id: "test-bootstrap-ready",
        toolVersions: { cortex: "0.4.2" },
        supportedEdgeTypes: ["CALLS", "IMPORTS"],
        supportedLanguages: ["typescript"],
      },
      repo: { rootName: "typescript-commerce", sourceHash: hash, fileCount: sources.files.length },
      frozen: true,
    };
    writeJson(path.join(repo, AGENT_DIR_NAME, MANIFEST_NAME), manifest);

    const result = bootstrapFromTracked(repo);
    assert.equal(result.state, "ready");
    assert.equal(result.descriptor.frozen, true);
    assert.equal(result.descriptor.contentHash, hash);
    assert.equal(result.descriptor.id, "test-bootstrap-ready");
    assert.deepEqual(result.descriptor.toolVersions, { cortex: "0.4.2" });
    assert.deepEqual(result.descriptor.supportedEdgeTypes, ["CALLS", "IMPORTS"]);
    assert.deepEqual(result.descriptor.supportedLanguages, ["typescript"]);
    // The tracked descriptor must carry NO absolute machine paths.
    const serialised = JSON.stringify(result.descriptor);
    assert.ok(!/D:[\\/]/.test(serialised), "ready descriptor must not carry a Windows drive letter");
    assert.ok(!/\/Users\//.test(serialised), "ready descriptor must not carry a Mac home prefix");
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("bootstrap returns stale when the tracked manifest does not match the source tree", () => {
  const repo = copyFixture();
  try {
    const manifest = {
      schemaVersion: 1,
      generationId: "test-bootstrap-stale",
      provider: "cortex-static",
      generatedAt: "2026-07-12T17:00:00Z",
      complete: true,
      repo: { rootName: "typescript-commerce", sourceHash: "xxh128:" + "0".repeat(32), fileCount: 0 },
      frozen: true,
    };
    writeJson(path.join(repo, AGENT_DIR_NAME, MANIFEST_NAME), manifest);

    const result = bootstrapFromTracked(repo);
    assert.equal(result.state, "stale");
    assert.ok(result.recordedHash);
    assert.ok(result.currentHash);
    assert.notEqual(result.recordedHash, result.currentHash);
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("bootstrap rejects manifests that bake in absolute machine paths", () => {
  const repo = copyFixture();
  try {
    const manifest = {
      schemaVersion: 1,
      generationId: "test-bootstrap-absolute-path",
      provider: "cortex-static",
      generatedAt: "2026-07-12T17:00:00Z",
      complete: true,
      // Bake an absolute Windows path into the manifest.
      repo: {
        rootName: "typescript-commerce",
        sourceHash: sourceHash(scanSources(repo).files),
        fileCount: 0,
        absolutePath: "D:/CodexWorktrees/Claude/crypt-main",
      },
      frozen: true,
    };
    writeJson(path.join(repo, AGENT_DIR_NAME, MANIFEST_NAME), manifest);

    const result = bootstrapFromTracked(repo);
    assert.equal(result.state, "corrupt");
    assert.ok(
      result.errors.some((e) => e.includes("absolute machine path")),
      "expected absolute-path rejection, got: " + JSON.stringify(result.errors),
    );
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("bootstrap rejects manifests with unsupported schemaVersion", () => {
  const repo = copyFixture();
  try {
    const manifest = {
      schemaVersion: 99,
      generationId: "test-bootstrap-bad-version",
      provider: "cortex-static",
      generatedAt: "2026-07-12T17:00:00Z",
      complete: true,
      repo: { rootName: "typescript-commerce", sourceHash: "xxh128:" + "0".repeat(32) },
    };
    writeJson(path.join(repo, AGENT_DIR_NAME, MANIFEST_NAME), manifest);
    const result = bootstrapFromTracked(repo);
    assert.equal(result.state, "corrupt");
    assert.ok(result.errors.some((e) => e.includes("schemaVersion")));
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("two rebuilds from identical inputs produce equivalent manifests and hashes", () => {
  const repo = copyFixture();
  try {
    const first = buildGraphGeneration(repo);
    const second = buildGraphGeneration(repo);
    assert.equal(first.manifest.generationId, second.manifest.generationId);
    assert.equal(first.manifest.repo.sourceHash, second.manifest.repo.sourceHash);

    const ids1 = first.nodes.map((n) => n.id).sort();
    const ids2 = second.nodes.map((n) => n.id).sort();
    assert.deepEqual(ids1, ids2);
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("graphStatus detects dirty overlays when files have been modified post-build", () => {
  const repo = copyFixture();
  try {
    spawnBuild(repo, ".agent");
    assert.equal(
      JSON.parse(spawnDoc(repo, ".agent")).capabilities?.dirtyOverlayFileCount ?? 0,
      0,
      "no dirty overlays immediately after build",
    );
    // Mutate one tracked file in the checkout.
    const serviceTs = path.join(repo, "src/service.ts");
    fs.writeFileSync(serviceTs, `${fs.readFileSync(serviceTs, "utf8")}\n// overlay marker\n`);

    const payload = JSON.parse(spawnDoc(repo, ".agent"));
    assert.ok(
      payload.graphCapabilities?.dirtyOverlayFileCount >= 1,
      `expected >=1 dirty overlay, got ${JSON.stringify(payload.graphCapabilities)}`,
    );
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("doctor reports degraded with machine-readable reasons for unsupported languages", () => {
  const repo = copyFixture();
  try {
    // Drop unsupported Crystal source into the checkout BEFORE the first build so the
    // recorded sourceHash already includes the unsupported extension.
    const crystalDir = path.join(repo, "crystal-src");
    fs.mkdirSync(crystalDir, { recursive: true });
    fs.writeFileSync(path.join(crystalDir, "thing.cr"), "def hello\n  \"world\"\nend\n");
    spawnBuild(repo, ".agent");

    const payload = JSON.parse(spawnDoc(repo, ".agent"));
    assert.equal(payload.state, "degraded", JSON.stringify(payload, null, 2));
    assert.ok(Array.isArray(payload.reasons), "doctor must include a `reasons` array");
    assert.ok(
      payload.reasons.some((r) => r.code === "unsupported_languages"),
      `expected an unsupported_languages reason code; got: ${JSON.stringify(payload.reasons)}`,
    );
    assert.ok(payload.capabilities, "doctor must include capabilities");
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("doctor is JSON-parseable and reports ready state for a clean fixture", () => {
  const repo = copyFixture();
  try {
    spawnBuild(repo, ".agent");
    const payload = JSON.parse(spawnDoc(repo, ".agent"));
    assert.equal(payload.state, "ready");
    assert.ok(Array.isArray(payload.reasons));
    assert.ok(payload.capabilities);
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

function spawnBuild(repo, outDir) {
  const cli = path.resolve(CORTEX, "scripts/cortex.mjs");
  const result = spawnSync(process.execPath, [cli, "build", "--out", outDir], {
    cwd: repo,
    encoding: "utf8",
  });
  if (result.status !== 0) {
    throw new Error(`build exited ${result.status}: ${result.stderr || result.stdout}`);
  }
  return result.stdout;
}

function spawnDoc(repo, outDir) {
  // Use the CLI so we exercise the doctor payload shape exactly as a caller would.
  const cli = path.resolve(CORTEX, "scripts/cortex.mjs");
  const result = spawnSync(
    process.execPath,
    [cli, "doctor", "--out", outDir],
    { cwd: repo, encoding: "utf8" },
  );
  if (result.status !== 0 && result.status !== 1 && result.status !== 2) {
    throw new Error(`doctor exited ${result.status}: ${result.stderr || result.stdout}`);
  }
  return result.stdout;
}
