import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { TOOLS, diagnosticsCapability } from "./server.mjs";
import { parseToolsetConfig } from "./toolsets.mjs";
import { diagnosticsRequest, resolveDiagnosticsBaseUrl } from "./lib/diagnostics-client.mjs";

const DIAGNOSTIC_TOOLS = [
  "membrane_diagnostic_workspace",
  "membrane_diagnostic_mutation",
  "membrane_diagnostic_snapshot",
  "membrane_diagnostic_fence",
  "membrane_diagnostic_capabilities",
  "membrane_diagnostic_baseline",
  "membrane_diagnostic_provider",
];
const EPOCH = {
  schemaVersion: "workspace-epoch.v1",
  repoId: "repo-1",
  worktreeId: "wt-1",
  epoch: 5,
  sourceManifestDigest: "sha256:manifest",
  changedPaths: ["src/main.ts"],
  changedFileHashes: [{ path: "src/main.ts", hash: "sha256:main" }],
  projectConfigDigest: "sha256:config",
  toolchainDigest: "sha256:toolchain",
  sandboxPolicyDigest: "sha256:sandbox",
  origin: "transactional",
};
const OBSERVED_EPOCH = { ...EPOCH, origin: "observed_hook" };

function requestStub(handler) {
  const calls = [];
  return Object.assign(async (pathname, options) => {
    calls.push({ pathname, options });
    return handler(calls.length - 1, { pathname, options });
  }, { calls });
}
const responding = (body) => async () => ({ ok: true, status: 200, body });

test("registers every §12 diagnostics operation as a tight-schema tool", () => {
  const names = TOOLS.map((tool) => tool.name);
  for (const name of DIAGNOSTIC_TOOLS) assert.ok(names.includes(name), `${name} missing`);
  for (const tool of TOOLS.filter(({ name }) => DIAGNOSTIC_TOOLS.includes(name))) {
    assert.equal(tool.inputSchema.additionalProperties, false, `${tool.name} must reject extra properties`);
    assert.equal(tool.inputSchema.type, "object");
    assert.ok(typeof tool.description === "string" && tool.description.length > 40);
  }
  const readOnly = new Set(["membrane_diagnostic_fence", "membrane_diagnostic_capabilities"]);
  for (const tool of TOOLS.filter(({ name }) => DIAGNOSTIC_TOOLS.includes(name))) {
    assert.equal(tool.annotations.readOnlyHint, readOnly.has(tool.name), `${tool.name} readOnlyHint`);
    assert.equal(tool.annotations.destructiveHint, tool.name === "membrane_diagnostic_provider");
  }
});

test("tool descriptions state host-mode semantics and never overclaim enforcement", () => {
  const description = (name) => TOOLS.find((tool) => tool.name === name).description;
  assert.match(description("membrane_diagnostic_workspace"), /reconciliation_only/);
  assert.match(description("membrane_diagnostic_mutation"), /observed_hook/);
  assert.match(description("membrane_diagnostic_mutation"), /not disk persistence/);
  assert.match(description("membrane_diagnostic_fence"), /never clears the resident fence/);
  assert.match(description("membrane_diagnostic_snapshot"), /[Ee]vents and presentation never clear the fence/);
});

test("toolsets registry maps every diagnostic tool under the diagnostic group", async () => {
  const raw = await readFile(new URL("../schemas/registry/toolsets.yaml", import.meta.url), "utf8");
  const groups = parseToolsetConfig(raw);
  assert.ok(groups, "schemas/registry/toolsets.yaml must stay parseable");
  assert.deepEqual([...groups.diagnostic].sort(), [...DIAGNOSTIC_TOOLS].sort());
});

test("handlers pass resident bodies and typed errors through verbatim", async () => {
  const capabilityBody = { providers: [{ providerId: "typescript", costClass: "interactive" }] };
  const success = requestStub(responding(capabilityBody));
  const ok = await diagnosticsCapability("membrane_diagnostic_capabilities", {}, { request: success });
  assert.equal(ok.delivered, true);
  assert.equal(ok.result, capabilityBody);
  assert.equal(success.calls[0].pathname, "/diagnostics/capabilities");

  const failure = requestStub(async () => ({ ok: false, status: 409, error: { code: "epoch_conflict", detail: "expected epoch 5" } }));
  const denied = await diagnosticsCapability("membrane_diagnostic_mutation", { operation: "begin", repoId: "repo-1", worktreeId: "wt-1" }, { request: failure });
  assert.equal(denied.delivered, false);
  assert.deepEqual(denied.error, { code: "epoch_conflict", detail: "expected epoch 5" });
});

test("workspace and mutation handlers dispatch to the REST contract paths", async () => {
  const calls = [];
  const request = async (pathname, options) => { calls.push({ pathname, options }); return { ok: true, status: 200, body: { ok: true } }; };
  await diagnosticsCapability("membrane_diagnostic_workspace", { operation: "open", repoId: "repo-1", worktreeId: "wt 1" }, { request });
  assert.equal(calls.at(-1).pathname, "/diagnostics/workspace/open");
  assert.deepEqual(calls.at(-1).options.body, { repoId: "repo-1", worktreeId: "wt 1" });
  await diagnosticsCapability("membrane_diagnostic_workspace", { operation: "status", repoId: "repo-1", worktreeId: "wt 1" }, { request });
  assert.equal(calls.at(-1).pathname, `/diagnostics/workspace/status?repoId=${encodeURIComponent("repo-1")}&worktreeId=${encodeURIComponent("wt 1")}`);
  await diagnosticsCapability("membrane_diagnostic_workspace", { operation: "reconcile", repoId: "r", worktreeId: "w", manifestDigest: "sha256:m", hashes: [{ path: "a.ts", hash: "sha256:a" }] }, { request });
  assert.equal(calls.at(-1).pathname, "/diagnostics/reconcile");
  await diagnosticsCapability("membrane_diagnostic_mutation", { operation: "seal", repoId: "repo-1", worktreeId: "wt-1", epoch: EPOCH }, { request });
  assert.equal(calls.at(-1).pathname, "/diagnostics/mutation/seal");
  await diagnosticsCapability("membrane_diagnostic_mutation", { operation: "registerObserved", repoId: "repo-1", worktreeId: "wt-1", epoch: OBSERVED_EPOCH }, { request });
  assert.equal(calls.at(-1).pathname, "/diagnostics/mutation/registerObserved");
  await diagnosticsCapability("membrane_diagnostic_fence", { snapshot: { schemaVersion: "diagnostics-evidence-snapshot.v1" }, expectedEpoch: OBSERVED_EPOCH, policy: { profileName: "changed-files-zero", policyVersion: "1", policyDigest: "sha256:p", blockingCodes: [], requiredCapabilities: ["syntax"] } }, { request });
  assert.equal(calls.at(-1).pathname, "/diagnostics/fence/evaluate");
  await diagnosticsCapability("membrane_diagnostic_baseline", { operation: "capture", repoId: "r", worktreeId: "w", name: "pre-refactor" }, { request });
  assert.equal(calls.at(-1).pathname, "/diagnostics/baseline/capture");
  await diagnosticsCapability("membrane_diagnostic_provider", { operation: "restart", keyDigest: "digest-1" }, { request });
  assert.equal(calls.at(-1).pathname, "/diagnostics/provider/restart");
  assert.deepEqual(calls.at(-1).options.body, { keyDigest: "digest-1" });
});

test("handler input validation fails closed with typed codes", async () => {
  const noCalls = requestStub(responding({}));
  const failing = (name, args) => diagnosticsCapability(name, args, { request: noCalls });
  await assert.rejects(failing("membrane_diagnostic_mutation", { operation: "transmute", repoId: "r", worktreeId: "w" }), /invalid_diagnostic_operation/);
  await assert.rejects(failing("membrane_diagnostic_workspace", { operation: "open", repoId: "", worktreeId: "w" }), /invalid_diagnostic_identity/);
  await assert.rejects(failing("membrane_diagnostic_mutation", { operation: "seal", repoId: "repo-1", worktreeId: "wt-1", epoch: { ...EPOCH, schemaVersion: "wrong" } }), /invalid_workspace_epoch/);
  await assert.rejects(failing("membrane_diagnostic_mutation", { operation: "registerObserved", repoId: "repo-1", worktreeId: "wt-1", epoch: EPOCH }), /invalid_workspace_epoch/);
  await assert.rejects(failing("membrane_diagnostic_mutation", { operation: "seal", repoId: "other", worktreeId: "wt-1", epoch: EPOCH }), /invalid_workspace_epoch/);
  await assert.rejects(failing("membrane_diagnostic_workspace", { operation: "reconcile", repoId: "r", worktreeId: "w", manifestDigest: "sha256:m", hashes: [{ path: "a" }] }), /invalid_reconcile_hashes/);
  await assert.rejects(failing("membrane_diagnostic_fence", { snapshot: {}, expectedEpoch: EPOCH, policy: { profileName: "p", policyVersion: "", policyDigest: "", blockingCodes: [], requiredCapabilities: ["telepathy"] } }), /invalid_gate_policy/);
  await assert.rejects(failing("membrane_diagnostic_provider", { operation: "restart" }), /invalid_provider_key_digest/);
  assert.equal(noCalls.calls.length, 0, "invalid requests must not reach the resident");
});

test("await caches per workspace and get/explain/delta are client-side views", async () => {
  const decision = { schemaVersion: "diagnostics-gate-decision.v1", snapshotId: "snap-1", policyProfile: "changed-files-zero", outcome: "dirty_exact", blockingIssueIds: ["issue-1"], reasonCodes: ["exact_blocker"], omissions: [] };
  const snapshotBody = { decision, snapshot: { blueprintDelta: { findingsDelta: [] }, aggregateDelta: { issues: [{ issueId: "issue-1", classification: "new" }] } } };
  const snapshots = new Map();
  const request = requestStub(responding(snapshotBody));
  const awaited = await diagnosticsCapability("membrane_diagnostic_snapshot", { operation: "await", repoId: "repo-1", worktreeId: "wt-1", policyProfileName: "changed-files-zero", requiredCapabilities: ["type_semantics"], maxCost: "interactive", deadlineMs: 5000 }, { request, snapshots });
  assert.equal(awaited.delivered, true);
  assert.deepEqual(request.calls[0].options.body, { repoId: "repo-1", worktreeId: "wt-1", policyProfileName: "changed-files-zero", requiredCapabilities: ["type_semantics"], maxCost: "interactive", deadlineMs: 5000 });

  const got = await diagnosticsCapability("membrane_diagnostic_snapshot", { operation: "get", repoId: "repo-1", worktreeId: "wt-1" }, { request, snapshots });
  assert.deepEqual(got.result.decision, decision);
  const explained = await diagnosticsCapability("membrane_diagnostic_snapshot", { operation: "explain", repoId: "repo-1", worktreeId: "wt-1" }, { request, snapshots });
  assert.equal(explained.result.outcome, "dirty_exact");
  assert.match(explained.result.guidance, /repair first/);
  assert.match(explained.result.note, /cannot clear the fence/);
  const delta = await diagnosticsCapability("membrane_diagnostic_snapshot", { operation: "delta", repoId: "repo-1", worktreeId: "wt-1" }, { request, snapshots });
  assert.deepEqual(delta.result.aggregateDelta, snapshotBody.snapshot.aggregateDelta);

  await assert.rejects(diagnosticsCapability("membrane_diagnostic_snapshot", { operation: "get", repoId: "repo-2", worktreeId: "wt-1" }, { request, snapshots }), /snapshot_not_awaited/);

  const decisionOnly = new Map();
  await diagnosticsCapability("membrane_diagnostic_snapshot", { operation: "await", repoId: "repo-1", worktreeId: "wt-2", policyProfileName: "changed-files-zero" }, { request: requestStub(responding(decision)), snapshots: decisionOnly });
  const degradedDelta = await diagnosticsCapability("membrane_diagnostic_snapshot", { operation: "delta", repoId: "repo-1", worktreeId: "wt-2" }, { request: requestStub(responding(decision)), snapshots: decisionOnly });
  assert.equal(degradedDelta.result.blueprintDelta, null);
  assert.deepEqual(degradedDelta.result.omissions, [{ code: "snapshot_body_not_cached", detail: "the awaited decision carried no evidence snapshot; delta views need it" }]);
});

test("client resolves base URL from env before the served loopback default", () => {
  assert.equal(resolveDiagnosticsBaseUrl({}), "http://127.0.0.1:47851");
  assert.equal(resolveDiagnosticsBaseUrl({ MEMBRANE_LOOPBACK_URL: "http://127.0.0.1:9999/" }), "http://127.0.0.1:9999");
  assert.equal(resolveDiagnosticsBaseUrl({ MEMBRANE_RESIDENT_URL: "http://127.0.0.1:7001/api", MEMBRANE_LOOPBACK_URL: "http://127.0.0.1:9999" }), "http://127.0.0.1:7001/api");
  assert.equal(resolveDiagnosticsBaseUrl({ MEMBRANE_RESIDENT_URL: "   " }), "http://127.0.0.1:47851");
});

test("client normalizes typed errors without swallowing detail", async () => {
  const fetchImpl = async () => ({ ok: false, status: 422, text: async () => JSON.stringify({ error: { code: "gate_profile_unknown", detail: "no such profile" } }) });
  const result = await diagnosticsRequest("/diagnostics/snapshot/await", { method: "POST", body: {}, fetchImpl, env: {} });
  assert.equal(result.ok, false);
  assert.deepEqual(result.error, { code: "gate_profile_unknown", detail: "no such profile" });

  const malformed = async () => ({ ok: false, status: 500, text: async () => "boom" });
  const fallback = await diagnosticsRequest("/diagnostics/status", { fetchImpl: malformed, env: {} });
  assert.deepEqual(fallback.error, { code: "resident_http_500", detail: "boom" });
});

test("client retries a refused connection exactly once then reports typed unreachability", async () => {
  let attempts = 0;
  const flakyThenUp = async () => {
    attempts += 1;
    if (attempts === 1) throw new TypeError("fetch failed");
    return { ok: true, status: 200, text: async () => JSON.stringify({ healthy: true }) };
  };
  const recovered = await diagnosticsRequest("/diagnostics/status", { fetchImpl: flakyThenUp, env: {} });
  assert.equal(recovered.ok, true);
  assert.equal(attempts, 2);

  attempts = 0;
  const alwaysDown = async () => { attempts += 1; throw new TypeError("fetch failed"); };
  const unreachable = await diagnosticsRequest("/diagnostics/status", { fetchImpl: alwaysDown, env: {} });
  assert.equal(unreachable.ok, false);
  assert.deepEqual(unreachable.error, { code: "resident_unreachable", detail: "fetch failed" });
  assert.equal(attempts, 2, "network refusal gets exactly one immediate retry");

  let httpErrors = 0;
  const httpError = async () => { httpErrors += 1; return { ok: false, status: 409, text: async () => "{}" }; };
  await diagnosticsRequest("/diagnostics/status", { fetchImpl: httpError, env: {} });
  assert.equal(httpErrors, 1, "HTTP errors are never retried");
});

test("client enforces its AbortController timeout as a single typed attempt", async () => {
  let attempts = 0;
  const stalled = (url, init) => {
    attempts += 1;
    return new Promise((_, reject) => {
      init.signal.addEventListener("abort", () => {
        const error = new Error("The operation was aborted");
        error.name = "AbortError";
        reject(error);
      });
    });
  };
  const timedOut = await diagnosticsRequest("/diagnostics/snapshot/await", { method: "POST", body: {}, fetchImpl: stalled, env: {}, timeoutMs: 20 });
  assert.equal(timedOut.ok, false);
  assert.deepEqual(timedOut.error, { code: "resident_timeout", detail: "resident did not respond within 20ms" });
  assert.equal(attempts, 1, "timeouts are never retried");
});
