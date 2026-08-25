import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const BLUEPRINT = path.resolve(HERE, "..");
const CLI = path.join(BLUEPRINT, "scripts/blueprint.mjs");
const FIXTURE = path.join(BLUEPRINT, "evals/fixture-repos/typescript-commerce");

// Audit projections must be buildable into an explicit run-owned output dir.
// The fixture is deliberately copied WITHOUT git so the walker sees every
// file — the exact environment where a non-`.agent*` output directory used
// to poison its own source hash and fail typed graph_stale forever.
function copyFixture(tag) {
  const dir = path.join(os.tmpdir(), `blueprint-audit-projection-${process.pid}-${Date.now()}-${tag}`);
  fs.cpSync(FIXTURE, dir, { recursive: true });
  return dir;
}

function run(args, cwd) {
  return spawnSync(process.execPath, [CLI, ...args], { cwd, encoding: "utf8", maxBuffer: 16 * 1024 * 1024 });
}

test("build into explicit run-owned output projects that exact graph fresh", () => {
  const repo = copyFixture("fresh");
  try {
    const OUT = ".audit-run/projection";
    const build = run(["graph", "build", "--out", OUT], repo);
    assert.equal(build.status, 0, build.stderr || build.stdout);

    const status = run(["graph", "status", "--out", OUT, "--json"], repo);
    assert.equal(status.status, 0, status.stderr || status.stdout);
    assert.equal(JSON.parse(status.stdout).state, "fresh");

    const projection = run(["graph", "audit-projection", "--out", OUT], repo);
    assert.equal(projection.status, 0, projection.stderr || projection.stdout);
    const packet = JSON.parse(projection.stdout);
    assert.equal(packet.schema, "membrane.blueprint-packet.v1");
    assert.equal(packet.status, "ready");
    assert.equal(packet.state, "ready");
    assert.match(packet.generationId, /^xxh128:/);
    assert.match(packet.manifestDigest, /^sha256:/);

    // The projection describes THIS output's generation, not another
    // checkout's or `.agent`'s: query the store envelope behind it directly.
    const manifest = run(["graph", "manifest", "--out", OUT], repo);
    assert.equal(manifest.status, 0, manifest.stderr || manifest.stdout);
    assert.equal(JSON.parse(manifest.stdout).generationId, packet.generationId);

    // The run-owned output never becomes its own input.
    const outPrefix = `${OUT}/`;
    assert.ok(!packet.files.some((file) => file === outPrefix.replace(/\/$/, "") || file.startsWith(outPrefix)));
    assert.deepEqual(packet.files, [...packet.files].sort());
    assert.equal(packet.fileCount, packet.files.length);
    assert.ok(packet.files.includes("src/service.ts"));
    assert.ok(packet.sourceFileCount >= 5);
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("refresh is idempotent and pinned projection survives concurrent drift", () => {
  const repo = copyFixture("pin");
  try {
    const OUT = ".audit-run/pin";
    assert.equal(run(["graph", "build", "--out", OUT], repo).status, 0);
    // Refresh in place: unchanged tree stays fresh (no self-inflicted drift).
    assert.equal(run(["graph", "build", "--out", OUT], repo).status, 0);

    const first = JSON.parse(run(["graph", "audit-projection", "--out", OUT], repo).stdout);
    const generationId = first.generationId;

    // Concurrent work elsewhere in the shared checkout after the caller's
    // build: unpinned projection fails typed; pinned still projects exactly
    // the generation the caller built.
    fs.writeFileSync(path.join(repo, "src/drift.ts"), "export const drifted = true;\n");

    const stale = run(["graph", "audit-projection", "--out", OUT], repo);
    assert.notEqual(stale.status, 0);
    assert.match(stale.stderr || "", /"code":\s*"graph_stale"/);

    const pinned = run(["graph", "audit-projection", "--out", OUT, "--expected-generation", generationId], repo);
    assert.equal(pinned.status, 0, pinned.stderr || pinned.stdout);
    const packet = JSON.parse(pinned.stdout);
    assert.equal(packet.generationId, generationId);
    assert.equal(packet.pinnedGeneration, generationId);
    assert.ok(!packet.files.includes("src/drift.ts"));
    assert.ok(packet.files.includes("src/service.ts"));
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("audit-projection failures stay typed", () => {
  const repo = copyFixture("typed");
  try {
    const OUT = ".audit-run/typed";
    const missing = run(["graph", "audit-projection", "--out", OUT], repo);
    assert.notEqual(missing.status, 0);
    assert.match(missing.stderr || "", /"code":\s*"graph_missing"/);

    assert.equal(run(["graph", "build", "--out", OUT], repo).status, 0);
    const wrongPin = "xxh128:" + "0".repeat(32);
    const mismatch = run(["graph", "audit-projection", "--out", OUT, "--expected-generation", wrongPin], repo);
    assert.notEqual(mismatch.status, 0);
    const errorEnvelope = JSON.parse(mismatch.stderr).error;
    assert.equal(errorEnvelope.code, "graph_generation_changed");
    assert.equal(errorEnvelope.expectedGeneration, wrongPin);
    assert.match(errorEnvelope.observedGeneration, /^xxh128:/);
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});
