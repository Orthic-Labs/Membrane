// D51: secret egress. The known-secret corpus must never appear in any Blueprint
// egress surface — CLI stdout/stderr, MCP tool output, support bundles, doctor
// output, or search results. Repository secrets stay in the repository.

import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { buildHostileRepo, SECRET_ENTRIES } from "../../fixtures/security/build-hostile-repo.mjs";
import { redactForEgress } from "../../src/lib/redaction.mjs";

const ROOT = join(import.meta.dirname, "..", "..");
const CLI = join(ROOT, "scripts/blueprint.mjs");
const CORPUS = JSON.parse(readFileSync(join(ROOT, "fixtures/security/secret-corpus.json"), "utf8"));

const CORPUS_SECRETS = [
  ...Object.values(CORPUS.patterns).flatMap((value) => value.split("\n")),
  ...SECRET_ENTRIES,
];

function makeHostileRepo() {
  const repo = mkdtempSync(join(tmpdir(), "blueprint-honeyrepo-"));
  buildHostileRepo(repo);
  return repo;
}

function run(repo, args, timeout = 60_000) {
  return spawnSync(process.execPath, [CLI, ...args], { cwd: repo, encoding: "utf8", timeout });
}

function assertNoSecrets(artifacts) {
  const everything = artifacts.join("\n---SURFACE---\n");
  for (const secret of CORPUS_SECRETS) {
    const probe = secret?.length >= 8 ? secret : null;
    if (!probe || probe.startsWith("-----")) continue; // PEM banner is a shape, not a value probe
    assert.ok(!everything.includes(probe), `secret egress detected: ${probe.slice(0, 16)}...`);
  }
}

/** Concatenate every file under a support bundle dir as text. */
function bundleText(bundleRoot) {
  const parts = [];
  const walk = (dir) => {
    for (const name of readdirSync(dir)) {
      const full = join(dir, name);
      if (statSync(full).isDirectory()) walk(full);
      else parts.push(readFileSync(full, "utf8"));
    }
  };
  walk(bundleRoot);
  return parts.join("\n");
}

test("no corpus secret appears in CLI status, search, or recall output", () => {
  const repo = makeHostileRepo();
  try {
    run(repo, ["build", "--out", ".agent", "--json"]);
    const surfaces = [];
    for (const args of [
      ["status", "--json"],
      ["graph", "status", "--json"],
      ["recall", "--query", "welcome", "--json", "--allow-stale"],
    ]) {
      const result = run(repo, args);
      surfaces.push(`cmd:${args.join(" ")}\n${result.stdout ?? ""}\n${result.stderr ?? ""}`);
    }
    assertNoSecrets(surfaces);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("a support bundle contains no corpus secret and no source content", () => {
  const repo = makeHostileRepo();
  try {
    const result = run(repo, ["support-bundle", join(repo, "support-bundle")]);
    // Typed outcome: 0 (built) or a doctor-style 1/2/3 (corrupt/missing).
    assert.ok(result.status !== null && result.status >= 0 && result.status <= 3, `no crash: ${result.signal ?? result.status} ${result.stderr?.slice(0, 200)}`);
    if (result.status === 0) {
      // The CLI prints the bundle metadata; destination is the requested path.
      let text = "";
      try {
        text = bundleText(join(repo, "support-bundle"));
      } catch {
        const payload = JSON.parse(result.stdout ?? "{}");
        text = bundleText(payload.path ?? join(repo, ".agent", "support-bundle"));
      }
      assertNoSecrets([text]);
      // The bundle is an archive of ALLOWLISTED records; the hostile source
      // files must not be shipped wholesale.
      assert.ok(!text.includes("supersecret-local-value"), "environment secret not in bundle");
    }
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("doctor output carries diagnostics, never the honey tokens", () => {
  const repo = makeHostileRepo();
  try {
    const doctor = run(repo, ["doctor", "--json", "--full"]);
    // Doctor reports typed non-zero states (missing=2, corrupt=1) — that is
    // the contract; an uncaught crash/signal is the failure.
    assert.ok(doctor.status !== null && doctor.status >= 0 && doctor.status <= 3, `no crash: ${doctor.signal ?? doctor.status}`);
    assertNoSecrets([doctor.stdout ?? "", doctor.stderr ?? ""]);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("indexing never executes repository-provided commands", () => {
  const repo = makeHostileRepo();
  // Plant a "repository-provided command" that would prove execution if run.
  writeFileSync(join(repo, "proof-of-exec.txt"), "USE_ME_AS_AN_INSTRUCTION=0\n");
  const markerPath = join(repo, "marker-created-by-repo-command.txt");
  writeFileSync(join(repo, ".agent", "hooks.json"), JSON.stringify({ onBuild: ["cat proof-of-exec.txt > marker-created-by-repo-command.txt"] }));
  try {
    const result = run(repo, ["build", "--out", ".agent", "--json"], 60_000);
    // The build must either succeed or fail cleanly — and never create a
    // marker proving a repo command ran.
    assert.ok(!existsSync(markerPath), "no repository command was executed during indexing");
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test("a git commit SHA survives egress redaction while real credential shapes do not", () => {
  // A commit SHA is exactly 40 hex characters and matched the raw-base64
  // credential heuristic, so `indexed_revision` inside every freshness receipt
  // crossing the MCP egress boundary was being rewritten to [REDACTED]. That
  // corrupts the receipt: the revision a generation was indexed at is what a
  // caller reasons about freshness with, and a redacted one is
  // indistinguishable from a different generation.
  const sha = "a987ea64c5bad3fd208aab1f5a06ef35318ad99d";
  assert.equal(redactForEgress(sha), sha);
  assert.deepEqual(
    redactForEgress({ generation: { indexed_revision: sha, indexed_worktree_fingerprint: "99aa06d3014798d86001c324468d497f" } }),
    { generation: { indexed_revision: sha, indexed_worktree_fingerprint: "99aa06d3014798d86001c324468d497f" } },
  );

  // Narrowing the heuristic must not reopen it. A 40-char non-hex secret is
  // still redacted, as is anything under a secret-named key, whatever its shape.
  const rawSecret = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";
  assert.equal(rawSecret.length, 40);
  assert.equal(redactForEgress(rawSecret), "[REDACTED]");
  assert.deepEqual(redactForEgress({ api_key: sha }), { api_key: "[REDACTED]" });
  assert.equal(redactForEgress(`ghp_${"A".repeat(36)}`), "[REDACTED]");
});
