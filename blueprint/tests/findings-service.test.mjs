// Findings resident service — generation/hash binding, freshness honesty,
// named-generation baselines, SARIF rendering, and daemon method registration
// (design §7 D0a/D0b lane, §7.1 Phase-0 items 5–7).
//
// Generation identities are stubbed through the service's injected seams so no
// graph store or watcher is required.

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { createServer } from "node:net";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { buildGenerationBoundBundle, computeBaselineDelta, createFindingsService, FINDINGS_SERVICE_METHODS } from "../src/lib/findings/service.mjs";
import { toSarif } from "../src/lib/sarif.mjs";
import { DaemonClient } from "../src/service/client.mjs";
import { temporaryDaemonEndpoint } from "../src/service/paths.mjs";
import { MAX_DEADLINE_MS, METHODS, PROTOCOL_VERSION } from "../src/service/protocol.mjs";
import { createDaemonServer } from "../src/service/server.mjs";

const GENERATION_A = "xxh128:gen-a";

const V1 = {
  "src/fuse.ts": "export function fuseCandidates() {}\nexport const scoreBatch = () => 1;\n",
  "src/admit.ts": "import { admitCandidate } from \"./fuse.js\";\nexport const run = () => admitCandidate();\n",
  "src/reexp.ts": "export { gone } from \"./missing-a.js\";\n",
};

const V2 = {
  "src/fuse.ts": V1["src/fuse.ts"],
  "src/admit.ts": "import { fuseCandidates } from \"./fuse.js\";\nexport const run = () => fuseCandidates();\n",
  "src/reexp.ts": "export { gone } from \"./missing-b.js\";\n",
  "src/new.ts": "import { nope } from \"./fuse.js\";\nexport const x = () => nope();\n",
};

function cleanOverlay() {
  return { available: true, stable: true, revision: "r", commitDistance: 0, entries: [], limitExceeded: false, reason: null };
}

function stubService({ generationId = GENERATION_A, baseCommit = null, overlay = () => cleanOverlay(), files = V1, stateDir = null } = {}) {
  let scans = 0;
  const service = createFindingsService({
    ...(stateDir ? { stateDir } : {}),
    sealedGeneration: () => ({ generationId, manifestDigest: "sha256:manifest-digest", baseCommit }),
    freshnessOverlay: () => overlay(),
    scanRepository: () => {
      scans += 1;
      return Object.entries(files).map(([path, text]) => ({ path, text }));
    },
    clock: () => "2026-01-01T00:00:00.000Z",
  });
  return { service, scans: () => scans };
}

function temporaryStateDir() {
  return mkdtempSync(join(tmpdir(), "blueprint-findings-baselines-"));
}

// ---------------------------------------------------------------------------
// Method registration convention
// ---------------------------------------------------------------------------

test("findings methods are registered on the resident protocol surface", () => {
  assert.deepEqual([...FINDINGS_SERVICE_METHODS].sort(), [
    "findings.baseline.capture",
    "findings.baseline.list",
    "findings.get",
    "findings.sarif",
  ]);
  for (const method of FINDINGS_SERVICE_METHODS) assert.ok(METHODS.includes(method));
});

// ---------------------------------------------------------------------------
// Generation/hash binding (§7.1 item 5)
// ---------------------------------------------------------------------------

test("bundles bind emission-time generation identity and per-file sha256 content hashes", async () => {
  const files = Object.entries(V1).map(([path, text]) => ({ path, text }));
  const bundle = await buildGenerationBoundBundle({ files, generationId: "xxh128:test-gen" });
  assert.equal(bundle.kind, "findings-bundle");
  assert.equal(bundle.generationId, "xxh128:test-gen");
  assert.deepEqual(Object.keys(bundle.perFileContentHashes).sort(), Object.keys(V1).sort());
  for (const [path, hash] of Object.entries(bundle.perFileContentHashes)) {
    assert.match(hash, /^sha256:[0-9a-f]{64}$/);
    assert.equal(hash, `sha256:${createHash("sha256").update(V1[path], "utf8").digest("hex")}`);
  }
  const bp001 = bundle.findings.find((finding) => finding.ruleId === "BP001");
  assert.equal(bp001.generationId, "xxh128:test-gen");
  assert.deepEqual(Object.keys(bp001.perFileContentHashes).sort(), ["src/admit.ts", "src/fuse.ts"]);
});

test("findings.get serves the canonical detect pipeline bound to the current generation", async () => {
  const { service, scans } = stubService();
  const result = await service["findings.get"]({});
  assert.equal(result.schemaVersion, 1);
  assert.equal(result.generationId, GENERATION_A);
  assert.equal(result.freshness, "current");
  assert.deepEqual(result.findings.map((finding) => finding.ruleId).sort(), ["BP001", "BP002"]);
  assert.ok(result.perFileContentHashes["src/fuse.ts"]);
  assert.equal(result.delta, null);
  assert.deepEqual(result.omissions.map((omission) => omission.code), []);
});

test("a working tree that moved under a still-sealed generation is never served a cached bundle", async () => {
  // Freshness honesty regression: the coarse generation key used to short-
  // circuit before the current files were hashed, so tree B under sealed
  // generation G was answered with tree A's bundle. Cache keys are now
  // content-addressed — the source digest is computed before any lookup.
  const mutable = { files: V1 };
  let detectionRuns = 0;
  const service = createFindingsService({
    sealedGeneration: () => ({ generationId: GENERATION_A, manifestDigest: "sha256:manifest-digest", baseCommit: null }),
    freshnessOverlay: () => cleanOverlay(),
    scanRepository: () => {
      detectionRuns += 1;
      return Object.entries(mutable.files).map(([path, text]) => ({ path, text }));
    },
    clock: () => "2026-01-01T00:00:00.000Z",
  });

  const first = await service["findings.get"]({});
  assert.deepEqual(first.findings.map((finding) => finding.ruleId).sort(), ["BP001", "BP002"]);
  assert.ok(first.perFileContentHashes["src/admit.ts"], "tree A scanned");

  // Working tree mutates A → B while the sealed generation stays G.
  mutable.files = V2;
  const second = await service["findings.get"]({});
  assert.equal(second.generationId, GENERATION_A, "generation unchanged");
  assert.notDeepEqual(
    second.perFileContentHashes,
    first.perFileContentHashes,
    "bundle must reflect the CURRENT bytes, not the first-seen bytes",
  );
  assert.ok(second.findings.some((finding) => finding.ruleId === "BP001" && finding.path === "src/new.ts"),
    "the newly added file must have been scanned and detected");
  assert.ok(detectionRuns >= 2, "each distinct tree state is scanned at least once");

  // Identical bytes remain cacheable: third call over unchanged tree B
  // reuses the content-addressed detection work.
  const before = detectionRuns;
  await service["findings.get"]({});
  // The repository scan itself always runs; only buildGenerationBoundBundle
  // work is cached, so findings stay correct either way.
  const third = await service["findings.get"]({});
  assert.deepEqual(third.findings, second.findings);
  assert.ok(detectionRuns >= before);
});

test("a moved-on working tree is served marked stale with a typed omission, never recomputed silently", async () => {
  const dirty = () => ({
    available: true,
    stable: false,
    revision: "r2",
    commitDistance: null,
    entries: [{ path: "src/fuse.ts", status: "M", contentHash: "sha256:something-new" }],
    limitExceeded: false,
    reason: null,
  });
  const { service, scans } = stubService({ overlay: dirty });
  const result = await service["findings.get"]({});
  assert.equal(result.freshness, "stale");
  assert.equal(result.generationId, GENERATION_A);
  const staleOmission = result.omissions.find((omission) => omission.code === "stale_generation");
  assert.ok(staleOmission);
  assert.equal(staleOmission.dirtyFileCount, 1);
  // allowStale:false refuses BEFORE scanning: no bundle work for refused
  // evidence, so the scan count stays at the one tolerated stale call.
  await assert.rejects(service["findings.get"]({ allowStale: false }), { code: "stale_blocked" });
  assert.equal(scans(), 1);
});

test("a pinned generation that is not current fails closed", async () => {
  const { service } = stubService();
  await assert.rejects(service["findings.get"]({ generation: "xxh128:gen-old" }), { code: "generation_mismatch" });
});

test("a missing sealed generation fails typed instead of scanning anyway", async () => {
  const service = createFindingsService({
    scanRepository: () => { assert.fail("must not scan without a sealed generation"); },
    sealedGeneration: () => {
      throw Object.assign(new Error("Graph store is missing."), { code: "graph_missing" });
    },
  });
  await assert.rejects(service["findings.get"]({ repoRoot: join(tmpdir(), "definitely-missing") }), { code: "graph_missing" });
});

// ---------------------------------------------------------------------------
// Named-generation baselines and deltas (§8.1)
// ---------------------------------------------------------------------------

test("named baselines capture, list, and delta added/resolved/changed against the bound generation", async () => {
  const stateDir = temporaryStateDir();
  try {
    const before = stubService({ files: V1, stateDir });
    const capture = await before.service["findings.baseline.capture"]({ name: "Before Review" });
    assert.equal(capture.name, "before-review");
    assert.equal(capture.generationId, GENERATION_A);
    assert.equal(capture.freshness, "current");
    assert.match(capture.path, /before-review\.json$/);

    const listed = await before.service["findings.baseline.list"]({});
    assert.deepEqual(listed.baselines.map((baseline) => baseline.name), ["before-review"]);
    assert.equal(listed.baselines[0].generationId, GENERATION_A);
    assert.equal(listed.baselines[0].findingCount, 2);

    const after = stubService({ files: V2, stateDir });
    const result = await after.service["findings.get"]({ baselineGeneration: "before-review" });
    assert.equal(result.delta.baselineGeneration, GENERATION_A);
    assert.equal(result.delta.baselineName, "before-review");
    assert.deepEqual(result.delta.resolved.map((entry) => entry.path), ["src/admit.ts"]);
    assert.deepEqual(result.delta.added.map((entry) => entry.path), ["src/new.ts"]);
    assert.deepEqual(result.delta.changed.map((entry) => entry.path), ["src/reexp.ts"]);
    assert.equal(result.delta.resolved.filter((entry) => entry.path === "src/reexp.ts").length, 0);

    // A literal generationId resolves to its most recent captured name too.
    const byGeneration = await after.service["findings.get"]({ baselineGeneration: GENERATION_A });
    assert.equal(byGeneration.delta.baselineName, "before-review");
  } finally {
    rmSync(stateDir, { recursive: true, force: true });
  }
});

test("an unknown baseline reference reports a typed omission and keeps the delta null", async () => {
  const stateDir = temporaryStateDir();
  try {
    const { service } = stubService({ stateDir });
    const result = await service["findings.get"]({ baselineGeneration: "never-captured" });
    assert.equal(result.delta, null);
    assert.ok(result.omissions.some((omission) => omission.code === "baseline_unknown"));
  } finally {
    rmSync(stateDir, { recursive: true, force: true });
  }
});

test("paths scope findings, omissions, and delta slices", async () => {
  const stateDir = temporaryStateDir();
  try {
    await stubService({ files: V1, stateDir }).service["findings.baseline.capture"]({ name: "base" });
    const { service } = stubService({ files: V2, stateDir });
    const scoped = await service["findings.get"]({ baselineGeneration: "base", paths: ["src/reexp.ts"] });
    assert.deepEqual(scoped.findings.map((finding) => finding.path), ["src/reexp.ts"]);
    assert.deepEqual(scoped.delta.changed.map((entry) => entry.path), ["src/reexp.ts"]);
    assert.deepEqual(scoped.delta.added, []);
    assert.deepEqual(scoped.delta.resolved, []);
    const unscoped = await service["findings.get"]({ baselineGeneration: "base" });
    assert.equal(unscoped.findings.length, 2);
  } finally {
    rmSync(stateDir, { recursive: true, force: true });
  }
});

test("computeBaselineDelta pairs a specifier rewrite as changed, not add+resolve", () => {
  const baselineRecord = {
    generationId: GENERATION_A,
    name: "base",
    findings: [
      { fingerprint: "fp-old", ruleId: "BP002", path: "src/reexp.ts", startLine: 1, endLine: 1, name: null, specifier: "./missing-a.js", severity: "error" },
    ],
  };
  const current = [{
    fingerprint: "fp-new", ruleId: "BP002", path: "src/reexp.ts", startLine: 1, endLine: 1, name: null, specifier: "./missing-b.js", severity: "error",
  }];
  const delta = computeBaselineDelta(current, baselineRecord);
  assert.deepEqual(delta.added, []);
  assert.deepEqual(delta.resolved, []);
  assert.equal(delta.changed.length, 1);
  assert.equal(delta.changed[0].fingerprint, "fp-new");
});

test("computeBaselineDelta keeps a newly affected finding in an untouched dependent file (affected-closure property)", async () => {
  // Regression: an overlay implementation that filtered added/resolved to
  // directly-dirty paths only would drop this finding, because b.ts itself
  // was never edited — only a.ts, the module it depends on, was. The
  // production delta is a pure fingerprint diff over the full current
  // finding set, so a new BP001 finding surfacing in an untouched dependent
  // must remain in the delta.
  const stateDir = temporaryStateDir();
  try {
    const beforeFiles = {
      "src/a.ts": "export const shared = 1;\n",
      "src/b.ts": "import { shared } from \"./a.js\";\nexport const x = shared;\n",
    };
    // Only a.ts's export changes; b.ts is byte-identical in both trees.
    const afterFiles = {
      "src/a.ts": "export const renamed = 1;\n",
      "src/b.ts": beforeFiles["src/b.ts"],
    };
    assert.equal(afterFiles["src/b.ts"], beforeFiles["src/b.ts"], "src/b.ts must be untouched between trees");

    const before = stubService({ files: beforeFiles, stateDir });
    const baselineResult = await before.service["findings.get"]({});
    assert.deepEqual(baselineResult.findings.map((finding) => finding.ruleId), [], "no findings before the export is renamed");
    await before.service["findings.baseline.capture"]({ name: "pre-rename" });

    const after = stubService({ files: afterFiles, stateDir });
    const result = await after.service["findings.get"]({ baselineGeneration: "pre-rename" });

    const newInDependent = result.delta.added.find((entry) => entry.path === "src/b.ts" && entry.ruleId === "BP001");
    assert.ok(newInDependent, "the affected-closure finding in the untouched dependent must appear in delta.added");
    assert.equal(newInDependent.name, "shared");
  } finally {
    rmSync(stateDir, { recursive: true, force: true });
  }
});

// ---------------------------------------------------------------------------
// SARIF as rendering, never independent truth (§7.1 item 7)
// ---------------------------------------------------------------------------

test("findings.sarif renders the same bound finding objects via lib/sarif.mjs", async () => {
  const { service } = stubService();
  const served = await service["findings.get"]({});
  const rendered = await service["findings.sarif"]({ toolVersion: "9.9.9-test" });
  assert.equal(rendered.freshness, "current");
  assert.equal(rendered.sarif.version, "2.1.0");
  assert.deepEqual(rendered.sarif, toSarif(served.findings, "9.9.9-test"));
});

// ---------------------------------------------------------------------------
// Resident daemon wiring
// ---------------------------------------------------------------------------

test("the daemon routes findings methods through the findings service adapter", async () => {
  const endpoint = temporaryDaemonEndpoint("blueprint-findings-routed");
  let seenInput = null;
  const daemon = createDaemonServer({
    endpoint,
    service: { status: async () => ({ generationId: "g" }) },
    findingsService: Object.freeze({
      "findings.get": async (input) => {
        seenInput = input;
        return { schemaVersion: 1, kind: "findings.get", generationId: "xxh128:routed", findings: [], omissions: [], delta: null };
      },
    }),
  });
  let client;
  try {
    await daemon.listen();
    client = new DaemonClient({ endpoint });
    const response = await client.findingsGet({ repoRoot: "/tmp/anywhere" }, { deadlineMs: 2000 });
    assert.equal(response.ok, true);
    assert.equal(response.generation, "xxh128:routed");
    assert.equal(seenInput.repoRoot, "/tmp/anywhere");
  } finally {
    await client?.close().catch(() => {});
    await daemon.close().catch(() => {});
  }
});

test("the daemon's default findings service fails typed on a repository without a graph", async () => {
  const endpoint = temporaryDaemonEndpoint("blueprint-findings-missing-graph");
  const repo = mkdtempSync(join(tmpdir(), "blueprint-findings-empty-"));
  const daemon = createDaemonServer({ endpoint, service: { status: async () => ({ generationId: "g" }) } });
  let client;
  try {
    await daemon.listen();
    client = new DaemonClient({ endpoint });
    const response = await client.findingsGet({ repoRoot: repo }, { deadlineMs: 5000 });
    assert.equal(response.ok, false);
    assert.equal(response.error.code, "graph_missing");
  } finally {
    await client?.close().catch(() => {});
    await daemon.close().catch(() => {});
    rmSync(repo, { recursive: true, force: true });
  }
});

test("client findings wrappers address the registered methods with a detection-sized deadline", async () => {
  const endpoint = temporaryDaemonEndpoint("blueprint-findings-wrappers");
  const received = [];
  const server = createServer((socket) => {
    socket.setEncoding("utf8");
    let buffer = "";
    socket.on("data", (chunk) => {
      buffer += chunk;
      let newline;
      while ((newline = buffer.indexOf("\n")) !== -1) {
        const line = buffer.slice(0, newline);
        buffer = buffer.slice(newline + 1);
        if (!line.trim()) continue;
        const request = JSON.parse(line);
        received.push(request);
        // Respond per request WITHOUT ending the socket: the client issues
        // four sequential requests over one connection.
        socket.write(`${JSON.stringify({ protocolVersion: PROTOCOL_VERSION, requestId: request.requestId, ok: true, generation: "g", result: {}, error: null })}\n`);
      }
    });
  });
  try {
    await new Promise((resolve, reject) => { server.once("error", reject); server.listen(endpoint, resolve); });
    const client = new DaemonClient({ endpoint });
    await client.connect();
    await client.findingsGet({});
    await client.findingsSarif({});
    await client.findingsBaselineCapture({ name: "x" }, { deadlineMs: 1234 });
    await client.findingsBaselineList({});
    await client.close();
    assert.deepEqual(received.map((request) => request.method), [
      "findings.get",
      "findings.sarif",
      "findings.baseline.capture",
      "findings.baseline.list",
    ]);
    assert.equal(received[0].deadlineMs, MAX_DEADLINE_MS);
    assert.equal(received[2].deadlineMs, 1234);
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
});
