// BPT-042 / BPT-044: cross-adapter parity evidence.
//
// BPT-042 claims the same application semantics are served through five
// adapters that all sit on createBlueprintApplicationService
// (src/lib/application/service.mjs:168): the Hub IPC daemon
// (src/service/server.mjs), the bounded one-shot path (embedded service /
// SDK direct-call fallback), the CLI (scripts/blueprint.mjs + scripts/cli),
// the JS SDK (src/sdk/client.mjs), and the native MCP server
// (scripts/blueprint-mcp.mjs). BPT-044 claims a stable, identical
// {code, retryable, summary, nextOperation} error taxonomy (sourced from
// src/lib/application/errors.mjs) across all of them.
//
// This file drives ONE fixture repo and the SAME logical requests through
// all five adapters and diffs the results. Non-deterministic transport
// fields (timings, pids, receipt/session ids, socket paths) are stripped
// explicitly and visibly before comparison -- nothing is blanket-deleted.
//
// Where an adapter genuinely cannot express or reproduce a given request,
// that is called out in the test itself rather than silently dropped from
// the parity claim (see the "CLI cannot pin a generation" notes below).
//
// Where adapters genuinely diverge, the assertion is written to the
// INTENDED contract (identical taxonomy) so a failure here is the executable
// record of the gap -- it is not weakened to pass.

import assert from "node:assert/strict";
import { cpSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

import { buildGraphGeneration } from "../src/graph/static-provider.mjs";
import { createBlueprintApplicationService } from "../src/lib/application/service.mjs";
import { RootRegistry } from "../src/lib/application/root-registry.mjs";
import { createDaemonServer } from "../src/service/server.mjs";
import { temporaryDaemonEndpoint } from "../src/service/paths.mjs";
import { BlueprintClient, EmbeddedBlueprintClient } from "../src/sdk/index.mjs";

const ROOT = join(import.meta.dirname, "..");
const CLI = join(ROOT, "scripts/blueprint.mjs");
const WATCH = join(ROOT, "scripts/blueprint-watch.mjs");
const MCP_SERVER = join(ROOT, "scripts/blueprint-mcp.mjs");
const FIXTURE = join(ROOT, "evals/fixture-repos/typescript-commerce");

function enroll(repo) {
  const result = spawnSync(process.execPath, [WATCH, "enroll", repo], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr || result.stdout);
}

function unenroll(repo) {
  const result = spawnSync(process.execPath, [WATCH, "unenroll", repo], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr || result.stdout);
}

function buildFixtureRepo(prefix) {
  const repo = mkdtempSync(join(tmpdir(), prefix));
  cpSync(FIXTURE, repo, { recursive: true });
  buildGraphGeneration(repo, { outDir: ".agent", persist: true });
  enroll(repo);
  return repo;
}

function runCli(repo, args) {
  return spawnSync(process.execPath, [CLI, ...args], { cwd: repo, encoding: "utf8" });
}

function parseCliJsonStderr(result) {
  const stderr = String(result.stderr).trim();
  const start = stderr.lastIndexOf("\n{");
  const jsonText = start >= 0 ? stderr.slice(start + 1) : stderr;
  return JSON.parse(jsonText);
}

async function startMcpClient(repo) {
  const transport = new StdioClientTransport({
    command: process.execPath,
    args: [MCP_SERVER, "--root", repo],
    cwd: repo,
    stderr: "pipe",
  });
  const client = new Client({ name: "blueprint-adapter-parity", version: "1.0.0" }, { capabilities: {} });
  await client.connect(transport);
  return { client, transport };
}

function mcpPayload(response) {
  const text = response.content.find((block) => block.type === "text")?.text;
  assert.ok(text, "MCP tool response must carry text content");
  return JSON.parse(text);
}

// ---------------------------------------------------------------------------
// Normalization: strip fields that are legitimately non-deterministic across
// adapters/processes/timing, and NOTHING else. Every stripped field is named
// below so the normalization itself stays auditable.
// ---------------------------------------------------------------------------

const VOLATILE_KEYS = new Set([
  // per-call receipt/session identifiers and wall-clock timings
  "receiptId", "createdMs", "elapsedMs",
  // process/transport identity that necessarily differs per adapter
  "socketPath", "pid",
]);

function stripVolatile(value) {
  if (Array.isArray(value)) return value.map(stripVolatile);
  if (value && typeof value === "object") {
    const out = {};
    for (const [key, val] of Object.entries(value)) {
      if (VOLATILE_KEYS.has(key)) continue;
      out[key] = stripVolatile(val);
    }
    return out;
  }
  return value;
}

// The status envelope additionally carries adapter/process-local runtime
// observations (whether a watcher happens to be running for THIS process,
// repo-wide overlay revision snapshots) that are not part of the
// "application semantics" BPT-042 claims parity over. These are named here,
// not silently dropped elsewhere.
function normalizeStatus(payload) {
  const { runtime, overlay, ...rest } = stripVolatile(payload);
  return rest;
}

function normalizeSearch(payload) {
  return stripVolatile(payload);
}

test("adapter success parity: status and search return identical normalized envelopes across all five adapters", { timeout: 60000 }, async () => {
  const repo = buildFixtureRepo("blueprint-parity-success-");
  const registry = new RootRegistry([{ root: repo, repoId: "parity-repo" }]);
  const service = createBlueprintApplicationService({ rootRegistry: registry, allowEmbeddedRoot: false });
  const endpoint = temporaryDaemonEndpoint("blueprint-parity-success");
  const daemon = createDaemonServer({ service, endpoint });
  let mcp;
  try {
    await daemon.listen();
    mcp = await startMcpClient(repo);

    // 1. Hub IPC daemon -- raw application service behind the daemon,
    //    reached through the same protocol scripts/service/server.mjs speaks.
    const daemonClient = new BlueprintClient({ endpoint, allowOneShot: false });
    const daemonStatus = await daemonClient.status({ repoId: "parity-repo" });
    const daemonSearch = await daemonClient.search({ repoId: "parity-repo", query: "placeOrder", limit: 5 });
    await daemonClient.close();

    // 2. Bounded one-shot -- EmbeddedBlueprintClient drives the same
    //    application service in-process, no daemon involved.
    const embedded = new EmbeddedBlueprintClient({ rootRegistry: registry, allowEmbeddedRoot: false });
    const embeddedStatus = await embedded.status({ repoRoot: repo });
    const embeddedSearch = await embedded.search({ repoRoot: repo, query: "placeOrder", limit: 5 });
    await embedded.close();

    // 3. CLI -- subprocess through scripts/blueprint.mjs's facade dispatch.
    const cliStatusResult = runCli(repo, ["status", "--json"]);
    assert.equal(cliStatusResult.status, 0, cliStatusResult.stderr);
    const cliStatus = JSON.parse(cliStatusResult.stdout);
    const cliSearchResult = runCli(repo, ["search", "--query", "placeOrder", "--limit", "5", "--json"]);
    assert.equal(cliSearchResult.status, 0, cliSearchResult.stderr);
    const cliSearch = JSON.parse(cliSearchResult.stdout);

    // 4. JS SDK -- BlueprintClient with the daemon absent, falling through to
    //    its bounded one-shot in-process call (src/sdk/client.mjs:16-21).
    const oneShotClient = new BlueprintClient({
      endpoint: temporaryDaemonEndpoint("blueprint-parity-one-shot-absent"),
      rootRegistry: registry,
    });
    const sdkOneShotStatus = await oneShotClient.status({ repoRoot: repo });
    const sdkOneShotSearch = await oneShotClient.search({ repoRoot: repo, query: "placeOrder", limit: 5 });
    await oneShotClient.close();

    // 5. Native MCP -- scripts/blueprint-mcp.mjs over stdio JSON-RPC.
    const mcpStatus = mcpPayload(await mcp.client.callTool({ name: "blueprint_status", arguments: {} }));
    const mcpSearch = mcpPayload(await mcp.client.callTool({ name: "blueprint_search", arguments: { query: "placeOrder", limit: 5 } }));
    // MCP additionally attaches a claimBoundary projection to blueprint_status
    // (CX-B1 claim-boundary contract) that the other four adapters' bare
    // `status()` call does not compute. That is an MCP-only enrichment layer
    // on top of the shared envelope, not a divergence in the envelope itself.
    const { claimBoundary: _mcpClaimBoundary, ...mcpStatusCore } = mcpStatus;

    const statuses = {
      daemon: normalizeStatus(daemonStatus),
      oneShotEmbedded: normalizeStatus(embeddedStatus),
      cli: normalizeStatus(cliStatus),
      sdkOneShot: normalizeStatus(sdkOneShotStatus),
      mcp: normalizeStatus(mcpStatusCore),
    };
    const searches = {
      daemon: normalizeSearch(daemonSearch),
      oneShotEmbedded: normalizeSearch(embeddedSearch),
      cli: normalizeSearch(cliSearch),
      sdkOneShot: normalizeSearch(sdkOneShotSearch),
      mcp: normalizeSearch(mcpSearch),
    };

    const statusBaseline = JSON.stringify(statuses.daemon);
    for (const [adapter, value] of Object.entries(statuses)) {
      assert.equal(JSON.stringify(value), statusBaseline, `status envelope diverges on adapter "${adapter}" vs daemon baseline`);
    }
    const searchBaseline = JSON.stringify(searches.daemon);
    for (const [adapter, value] of Object.entries(searches)) {
      assert.equal(JSON.stringify(value), searchBaseline, `search envelope diverges on adapter "${adapter}" vs daemon baseline`);
    }
  } finally {
    if (mcp) { await mcp.client.close().catch(() => {}); await mcp.transport.close().catch(() => {}); }
    await daemon.close().catch(() => {});
    unenroll(repo);
    rmSync(repo, { recursive: true, force: true });
  }
});

// ---------------------------------------------------------------------------
// Error taxonomy parity (BPT-044). errors.mjs's ERROR_METADATA is the single
// source of {retryable, summary, nextOperation}; this proves every adapter's
// egress actually carries it through for the SAME thrown code, rather than
// trusting the shared throw site.
// ---------------------------------------------------------------------------

// root_not_enrolled: an explicit, never-enrolled repoRoot. Reachable with an
// identical request shape ({repoRoot}) on all five adapters.
test("error taxonomy parity: root_not_enrolled", { timeout: 60000 }, async () => {
  const unenrolledRoot = mkdtempSync(join(tmpdir(), "blueprint-parity-unenrolled-"));
  const registry = new RootRegistry();
  const service = createBlueprintApplicationService({ rootRegistry: registry, allowEmbeddedRoot: false });
  const endpoint = temporaryDaemonEndpoint("blueprint-parity-root-not-enrolled");
  const daemon = createDaemonServer({ service, endpoint });
  let mcp;
  let mcpRepo;
  try {
    await daemon.listen();
    // MCP needs *some* root to boot against; point it at an enrolled fixture
    // and pass an unenrolled repoId argument instead, matching the exact
    // failure mode tests/mcp-server.test.mjs already exercises.
    mcpRepo = buildFixtureRepo("blueprint-parity-mcp-root-");
    mcp = await startMcpClient(mcpRepo);

    // 1. Hub IPC daemon.
    const daemonClient = new BlueprintClient({ endpoint, allowOneShot: false });
    const daemonError = await daemonClient.status({ repoRoot: unenrolledRoot }).then(
      () => assert.fail("expected root_not_enrolled"),
      (error) => error,
    );
    await daemonClient.close();

    // 2. Bounded one-shot (embedded, in-process, no daemon).
    const embedded = new EmbeddedBlueprintClient({ rootRegistry: registry, allowEmbeddedRoot: false });
    const embeddedError = await embedded.status({ repoRoot: unenrolledRoot }).then(
      () => assert.fail("expected root_not_enrolled"),
      (error) => error,
    );
    await embedded.close();

    // 3. CLI.
    const cliResult = runCli(unenrolledRoot, ["status", "--json"]);
    assert.notEqual(cliResult.status, 0);
    const cliErrorPayload = parseCliJsonStderr(cliResult).error;

    // 4. JS SDK, daemon absent -> bounded one-shot fallback path.
    const oneShotClient = new BlueprintClient({
      endpoint: temporaryDaemonEndpoint("blueprint-parity-root-one-shot-absent"),
      rootRegistry: registry,
    });
    const sdkOneShotError = await oneShotClient.status({ repoRoot: unenrolledRoot }).then(
      () => assert.fail("expected root_not_enrolled"),
      (error) => error,
    );
    await oneShotClient.close();

    // 5. Native MCP: repoId that resolves to nothing enrolled.
    const mcpResponse = await mcp.client.callTool({ name: "blueprint_status", arguments: { repoId: "does-not-exist-anywhere" } });
    assert.equal(mcpResponse.isError, true);
    const mcpErrorPayload = mcpPayload(mcpResponse).error;

    const taxonomy = (label, code, retryable, summary, nextOperation) => ({ label, code, retryable, summary, nextOperation });
    const results = [
      taxonomy("daemon (Hub IPC)", daemonError.code, daemonError.retryable, daemonError.remediation?.summary, daemonError.remediation?.nextOperation),
      taxonomy("boundedOneShot (embedded)", embeddedError.code, embeddedError.retryable, embeddedError.remediation?.summary, embeddedError.remediation?.nextOperation),
      taxonomy("cli", cliErrorPayload.code, cliErrorPayload.retryable, cliErrorPayload.remediation?.summary, cliErrorPayload.remediation?.nextOperation),
      taxonomy("sdk (one-shot fallback)", sdkOneShotError.code, sdkOneShotError.retryable, sdkOneShotError.remediation?.summary, sdkOneShotError.remediation?.nextOperation),
      taxonomy("mcp", mcpErrorPayload.code, mcpErrorPayload.retryable, mcpErrorPayload.remediation?.summary, mcpErrorPayload.remediation?.nextOperation),
    ];

    // All five must at minimum agree on the CODE -- this part does hold today.
    for (const result of results) {
      assert.equal(result.code, "root_not_enrolled", `${result.label} did not surface code root_not_enrolled (got ${result.code})`);
    }

    // BPT-044's full claim is retryable/summary/nextOperation parity too.
    // This is where the taxonomy currently breaks: the errors.mjs-computed
    // `retryable` flag (a top-level property BlueprintError sets from
    // ERROR_METADATA, e.g. errors.mjs:150-156) is preserved by the in-process
    // adapters (embedded bounded one-shot, CLI's raw JS object, SDK one-shot
    // fallback -- these never cross a wire) but is DROPPED by every adapter
    // that serializes the error across a transport boundary:
    //   - the Hub IPC daemon's transportError() (src/service/server.mjs:27-33)
    //     forwards only {code, message, details} -- `retryable` and the
    //     constructor-derived `remediation` never leave the daemon process;
    //   - the JS SDK's own daemon-response path (src/sdk/client.mjs:29-34)
    //     reconstructs the Error from exactly that trimmed wire shape, so a
    //     SDK client talking to a REAL resident daemon also loses it (this
    //     is distinct from the one-shot fallback path exercised above, which
    //     never touches the wire and is why it still passes).
    // MCP (scripts/blueprint-mcp.mjs:187-201) is the one cross-process
    // adapter that deliberately re-attaches `retryable` and `remediation` to
    // its own wire envelope, so it does NOT exhibit this gap.
    // errors.mjs's own BlueprintError (the "boundedOneShot (embedded)" result,
    // which never crosses a transport boundary) is ground truth for what
    // ERROR_METADATA says this code's retryable/nextOperation should be.
    const groundTruth = results.find((result) => result.label === "boundedOneShot (embedded)");
    for (const result of results) {
      assert.equal(
        result.retryable,
        groundTruth.retryable,
        `retryable diverges on adapter "${result.label}": expected ${groundTruth.retryable} (per errors.mjs), got ${result.retryable} -- ` +
        `see src/service/server.mjs transportError() and src/sdk/client.mjs #call(), which drop the BlueprintError-level ` +
        `retryable/remediation fields when relaying a response received over the Hub IPC socket`,
      );
    }
    // The remediated OPERATION must be identical on every adapter — that is
    // what the taxonomy pins. Its ARGUMENT cannot be: MCP forbids a
    // caller-supplied repoRoot by design (D07 root confinement; see
    // COMMON_FIELDS in scripts/blueprint-mcp.mjs, "repoRoot is deliberately
    // absent"), so `rootNotEnrolled` has no root to normalize and emits the
    // unparameterized form of the same command. That is a deliberate
    // architectural property of the MCP surface, not a taxonomy break, so it
    // is asserted explicitly rather than papered over with a loosened
    // comparison for every adapter.
    const operationVerb = (value) => String(value ?? "").split(" --")[0];
    for (const result of results) {
      assert.equal(
        operationVerb(result.nextOperation),
        operationVerb(groundTruth.nextOperation),
        `remediated operation diverges on adapter "${result.label}": expected ${JSON.stringify(operationVerb(groundTruth.nextOperation))}, got ${JSON.stringify(result.nextOperation)}`,
      );
    }
    for (const result of results) {
      if (result.label.startsWith("mcp")) {
        // Pinned deliberately: if MCP ever gains a repoRoot input, this
        // assertion fails and the carve-out above must be revisited.
        assert.equal(
          result.nextOperation,
          "blueprint init --root <repository-root>",
          "MCP cannot receive a repoRoot (D07), so its remediation stays unparameterized",
        );
        continue;
      }
      assert.equal(
        result.nextOperation,
        groundTruth.nextOperation,
        `nextOperation diverges on adapter "${result.label}": expected ${JSON.stringify(groundTruth.nextOperation)} (per errors.mjs), got ${JSON.stringify(result.nextOperation)}`,
      );
    }
  } finally {
    if (mcp) { await mcp.client.close().catch(() => {}); await mcp.transport.close().catch(() => {}); }
    await daemon.close().catch(() => {});
    if (mcpRepo) { unenroll(mcpRepo); rmSync(mcpRepo, { recursive: true, force: true }); }
    rmSync(unenrolledRoot, { recursive: true, force: true });
  }
});

// generation_mismatch: forced by pinning a bogus `generation` on search
// against an already-built repo. Reachable through the daemon, the embedded
// bounded one-shot, the SDK one-shot fallback, and MCP -- all of which accept
// a `generation` field on the search request.
//
// The CLI facade's `search` command (scripts/cli/commands.mjs) does NOT
// forward any generation-pinning input to the service at all (it only ever
// builds `{ repoRoot, query, limit }`), so there is no CLI invocation that
// expresses this same request. That is a genuine adapter capability gap, not
// an oversight in this test: the CLI is exercised for `root_not_enrolled`
// above (a request shape it CAN express) and is deliberately left out here,
// named rather than silently absent.
test("error taxonomy parity: generation_mismatch (daemon, bounded one-shot, SDK one-shot, MCP; CLI cannot express this request)", { timeout: 60000 }, async () => {
  const repo = buildFixtureRepo("blueprint-parity-genmismatch-");
  const registry = new RootRegistry([{ root: repo, repoId: "parity-repo" }]);
  const service = createBlueprintApplicationService({ rootRegistry: registry, allowEmbeddedRoot: false });
  const endpoint = temporaryDaemonEndpoint("blueprint-parity-genmismatch");
  const daemon = createDaemonServer({ service, endpoint });
  let mcp;
  try {
    await daemon.listen();
    mcp = await startMcpClient(repo);

    const daemonClient = new BlueprintClient({ endpoint, allowOneShot: false });
    const daemonError = await daemonClient.search({ repoId: "parity-repo", query: "placeOrder", generation: "not-a-real-generation" }).then(
      () => assert.fail("expected generation_mismatch"),
      (error) => error,
    );
    await daemonClient.close();

    const embedded = new EmbeddedBlueprintClient({ rootRegistry: registry, allowEmbeddedRoot: false });
    const embeddedError = await embedded.search({ repoRoot: repo, query: "placeOrder", generation: "not-a-real-generation" }).then(
      () => assert.fail("expected generation_mismatch"),
      (error) => error,
    );
    await embedded.close();

    const oneShotClient = new BlueprintClient({
      endpoint: temporaryDaemonEndpoint("blueprint-parity-genmismatch-absent"),
      rootRegistry: registry,
    });
    const sdkOneShotError = await oneShotClient.search({ repoRoot: repo, query: "placeOrder", generation: "not-a-real-generation" }).then(
      () => assert.fail("expected generation_mismatch"),
      (error) => error,
    );
    await oneShotClient.close();

    const mcpResponse = await mcp.client.callTool({ name: "blueprint_search", arguments: { query: "placeOrder", generation: "not-a-real-generation" } });
    assert.equal(mcpResponse.isError, true);
    const mcpErrorPayload = mcpPayload(mcpResponse).error;

    const taxonomy = (label, code, retryable, summary, nextOperation) => ({ label, code, retryable, summary, nextOperation });
    const results = [
      taxonomy("daemon (Hub IPC)", daemonError.code, daemonError.retryable, daemonError.remediation?.summary, daemonError.remediation?.nextOperation),
      taxonomy("boundedOneShot (embedded)", embeddedError.code, embeddedError.retryable, embeddedError.remediation?.summary, embeddedError.remediation?.nextOperation),
      taxonomy("sdk (one-shot fallback)", sdkOneShotError.code, sdkOneShotError.retryable, sdkOneShotError.remediation?.summary, sdkOneShotError.remediation?.nextOperation),
      taxonomy("mcp", mcpErrorPayload.code, mcpErrorPayload.retryable, mcpErrorPayload.summary ?? mcpErrorPayload.remediation?.summary, mcpErrorPayload.remediation?.nextOperation),
    ];

    for (const result of results) {
      assert.equal(result.code, "generation_mismatch", `${result.label} did not surface code generation_mismatch (got ${result.code})`);
    }

    // Same gap as root_not_enrolled above: the daemon-relayed adapters lose
    // the constructor-derived retryable/remediation because generation_mismatch
    // has NO manually-attached details.remediation (unlike root_not_enrolled) --
    // it relies entirely on errors.mjs's ERROR_METADATA auto-attachment
    // (errors.mjs:150-156), which transportError()/SDK #call() never forward.
    const groundTruth = results.find((result) => result.label === "boundedOneShot (embedded)");
    for (const result of results) {
      assert.equal(
        result.retryable,
        groundTruth.retryable,
        `retryable diverges on adapter "${result.label}": expected ${groundTruth.retryable} (per errors.mjs), got ${result.retryable}`,
      );
    }
    for (const result of results) {
      assert.equal(
        result.nextOperation,
        groundTruth.nextOperation,
        `nextOperation diverges on adapter "${result.label}": expected ${JSON.stringify(groundTruth.nextOperation)} (per errors.mjs), got ${JSON.stringify(result.nextOperation)}`,
      );
    }
  } finally {
    if (mcp) { await mcp.client.close().catch(() => {}); await mcp.transport.close().catch(() => {}); }
    await daemon.close().catch(() => {});
    unenroll(repo);
    rmSync(repo, { recursive: true, force: true });
  }
});
