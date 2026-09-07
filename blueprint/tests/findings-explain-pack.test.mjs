import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawn, spawnSync } from "node:child_process";
import test from "node:test";

import { createFindingsService } from "../src/lib/findings/service.mjs";
import { createDaemonServer } from "../src/service/server.mjs";
import { temporaryDaemonEndpoint } from "../src/service/paths.mjs";

const FILES = {
  "src/target.ts": "export const present = 1;\n",
  "src/user.ts": "import { missing } from './target.js';\nexport const value = missing;\n",
};

function service() {
  return createFindingsService({
    sealedGeneration: () => ({ generationId: "gen-findings", repoId: "repo-findings", manifestDigest: "sha256:m", baseCommit: null }),
    freshnessOverlay: () => ({ available: true, stable: true, limitExceeded: false, entries: [], reason: null }),
    scanRepository: () => Object.entries(FILES).map(([path, text]) => ({ path, text })),
  });
}

test("findings.explain binds rule reasoning to source/hash evidence", async () => {
  const api = service();
  const listed = await api["findings.get"]({ repoRoot: "/repo" });
  assert.equal(listed.findings.length, 1);
  const finding = listed.findings[0];
  const explained = await api["findings.explain"]({ repoRoot: "/repo", fingerprint: finding.fingerprint });
  assert.equal(explained.kind, "findings.explain");
  assert.equal(explained.generationId, "gen-findings");
  assert.equal(explained.finding.fingerprint, finding.fingerprint);
  assert.equal(explained.reasoning.ruleName, "import-binding-not-exported");
  assert.ok(explained.reasoning.description);
  assert.ok(explained.reasoning.message.includes("missing"));
  assert.deepEqual(explained.evidence.map((entry) => entry.path), ["src/user.ts", "src/target.ts"]);
  assert.ok(explained.evidence.every((entry) => typeof entry.contentHash === "string" && entry.contentHash.startsWith("sha256:")));
});

test("findings.explain fails closed for a finding outside the served generation", async () => {
  await assert.rejects(service()["findings.explain"]({ repoRoot: "/repo", fingerprint: "not-here" }), { code: "finding_not_found" });
});

test("findings.evidence_pack includes only selected findings and is generation-bound", async () => {
  const api = service();
  const listed = await api["findings.get"]({ repoRoot: "/repo" });
  const fingerprint = listed.findings[0].fingerprint;
  const result = await api["findings.evidence_pack"]({ repoRoot: "/repo", fingerprints: [fingerprint] });
  assert.equal(result.kind, "findings.evidence_pack");
  assert.equal(result.generationId, "gen-findings");
  assert.equal(result.pack.repoId, "repo-findings");
  assert.equal(result.pack.generationId, "gen-findings");
  assert.deepEqual(result.pack.results.map((entry) => entry.id), [fingerprint]);
  assert.ok(result.pack.results[0].evidence.every((entry) => entry.contentHash));
  assert.match(result.pack.packDigest, /^[0-9a-f]{64}$/);
});

test("findings.evidence_pack requires an explicit bounded selection", async () => {
  const api = service();
  await assert.rejects(api["findings.evidence_pack"]({ repoRoot: "/repo", fingerprints: [] }), { code: "finding_selection_empty" });
  await assert.rejects(api["findings.evidence_pack"]({ repoRoot: "/repo", fingerprints: Array.from({ length: 101 }, (_, i) => `f${i}`) }), { code: "finding_selection_too_large" });
});

// ---------------------------------------------------------------------------
// End-to-end: the real `blueprint findings explain` / `blueprint findings
// evidence-pack` CLI subcommands, over the real resident daemon protocol —
// this is the actual production routing path BPT-051/BPT-052 close, not the
// findings service library called in-process.
// ---------------------------------------------------------------------------

const ROOT = join(import.meta.dirname, "..");
const CLI = join(ROOT, "scripts/blueprint.mjs");

// The daemon under test lives in this same process (createDaemonServer), so
// CLI invocations MUST be spawned asynchronously — a synchronous spawnSync
// call would block this process's event loop and the in-process daemon could
// never answer the socket, deadlocking the test.
function runCli(args, { cwd, env }) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [CLI, ...args], { cwd, env, encoding: "utf8" });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.once("error", reject);
    child.once("close", (status) => resolve({ status, stdout, stderr }));
  });
}

function buildFindingRepo() {
  const repo = mkdtempSync(join(tmpdir(), "blueprint-findings-cli-"));
  mkdirSync(join(repo, "src"));
  writeFileSync(join(repo, "src/target.ts"), "export const present = 1;\n");
  writeFileSync(join(repo, "src/user.ts"), "import { missing } from './target.js';\nexport const value = missing;\n");
  spawnSync("git", ["init"], { cwd: repo });
  spawnSync("git", ["add", "-A"], { cwd: repo });
  spawnSync("git", ["-c", "user.email=test@example.com", "-c", "user.name=test", "commit", "-m", "init"], { cwd: repo });
  const build = spawnSync(process.execPath, [CLI, "graph", "build", "--out", ".agent"], { cwd: repo, encoding: "utf8" });
  assert.equal(build.status, 0, build.stderr || build.stdout);
  return repo;
}

async function withDaemon(fn) {
  const endpoint = temporaryDaemonEndpoint("blueprint-findings-cli");
  const daemon = createDaemonServer({ endpoint, service: { status: async () => ({ generationId: "g" }) } });
  await daemon.listen();
  const env = { ...process.env, BLUEPRINT_DAEMON_ENDPOINT: endpoint, NODE_NO_WARNINGS: "1" };
  try {
    await fn(env);
  } finally {
    await daemon.close().catch(() => {});
  }
}

test("CLI: `blueprint findings explain` returns rule reasoning and source-bound evidence for a real finding", async () => {
  const repo = buildFindingRepo();
  try {
    await withDaemon(async (env) => {
      const get = await runCli(["findings", "--json"], { cwd: repo, env });
      assert.equal(get.status, 2, get.stderr || get.stdout); // findings present => exit 2
      const listed = JSON.parse(get.stdout);
      const fingerprint = listed.findings[0].fingerprint;
      assert.ok(fingerprint);

      const explain = await runCli(["findings", "explain", "--fingerprint", fingerprint], { cwd: repo, env });
      assert.equal(explain.status, 0, explain.stderr || explain.stdout);
      const payload = JSON.parse(explain.stdout);
      assert.equal(payload.schemaVersion, 1);
      assert.equal(payload.kind, "findings.explain");
      assert.equal(payload.finding.fingerprint, fingerprint);
      assert.equal(payload.reasoning.ruleName, "import-binding-not-exported");
      assert.ok(payload.reasoning.message.includes("missing"));
      assert.deepEqual(payload.evidence.map((entry) => entry.path), ["src/user.ts", "src/target.ts"]);
      assert.ok(payload.evidence.every((entry) => typeof entry.contentHash === "string" && entry.contentHash.startsWith("sha256:")));
    });
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("CLI: `blueprint findings explain` fails closed for a fingerprint outside the served generation", async () => {
  const repo = buildFindingRepo();
  try {
    await withDaemon(async (env) => {
      const explain = await runCli(["findings", "explain", "--fingerprint", "not-a-real-fingerprint"], { cwd: repo, env });
      assert.notEqual(explain.status, 0);
      const payload = JSON.parse(explain.stderr);
      assert.equal(payload.error.code, "finding_not_found");
    });
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("CLI: `blueprint findings evidence-pack` produces a pack bound to the generation containing only the selected findings", async () => {
  const repo = buildFindingRepo();
  try {
    await withDaemon(async (env) => {
      const get = await runCli(["findings", "--json"], { cwd: repo, env });
      const fingerprint = JSON.parse(get.stdout).findings[0].fingerprint;

      const pack = await runCli(["findings", "evidence-pack", "--fingerprints", fingerprint], { cwd: repo, env });
      assert.equal(pack.status, 0, pack.stderr || pack.stdout);
      const payload = JSON.parse(pack.stdout);
      assert.equal(payload.schemaVersion, 1);
      assert.equal(payload.kind, "findings.evidence_pack");
      assert.ok(payload.generationId);
      assert.deepEqual(payload.pack.results.map((entry) => entry.id), [fingerprint]);
      assert.ok(payload.pack.results[0].evidence.every((entry) => entry.contentHash));
    });
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("CLI: `blueprint findings evidence-pack` refuses when no explicit selection is given", async () => {
  const repo = buildFindingRepo();
  try {
    await withDaemon(async (env) => {
      const pack = await runCli(["findings", "evidence-pack"], { cwd: repo, env });
      assert.notEqual(pack.status, 0);
      const payload = JSON.parse(pack.stderr);
      assert.equal(payload.error.code, "finding_selection_empty");
    });
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});
