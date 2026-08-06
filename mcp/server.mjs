#!/usr/bin/env node
// Public Membrane MCP adapter. It deliberately exposes no raw memory CRUD surface.

import { spawn } from "node:child_process";
import { DatabaseSync } from "node:sqlite";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";
import { createHash } from "node:crypto";
import { McpServer, fromJsonSchema } from "@modelcontextprotocol/server";
import { serveStdio } from "@modelcontextprotocol/server/stdio";
import { bindingFor } from "./project-registry.mjs";
import { installationBindingFor, installationEnv } from "./installation-binding.mjs";
import { buildRepositoryCatalog, hasExplicitChildGrant, resolveByAlias } from "./repository-catalog.mjs";
import { feedbackEvent, feedbackPolicy } from "./feedback-loop.mjs";
import { eventDbFor, ProposalStore } from "./proposal-store.mjs";
import { ScratchpadStore, WorkingContextStore } from "./working-context.mjs";
import { mintScopeGrantV1 } from "./scope-grant-v1.mjs";
import { intersectAuthority, permitsLevel, canReachTarget } from "./authorization.mjs";
import { selectWorkspaceTargets } from "./workspace-routing.mjs";
import { createDeadline, deadlineSignal, mapConcurrent, terminalReason } from "./deadline.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const CLIENT = join(HERE, "client.mjs");
const PROTOCOL_URI = "membrane://protocol/v1";
const MAX_REQUEST_BYTES = 32 * 1024;
const SCOPE_GRANT_FRESHNESS_TIMEOUT_MS = 1200;
const MAX_PROPOSAL_BYTES = 16 * 1024;
const MAX_FEEDBACK_BYTES = 2 * 1024;
const RATE_WINDOW_MS = 60_000;
const RATE_LIMITS = { proposal: 12, checkpoint: 24, feedback: 48 };
const rateWindows = new Map();
// Installation-authority ceiling for MBR-002 monotone intersection. Until a
// per-installation policy is derived, the platform admission cap is
// write-proposed so it never caps a legitimate caller below its own declared
// level, while still bounding an over-privileged registry claim.
const INSTALLATION_AUTHORITY_LEVEL = "write-proposed";
// MBR-005 (SN-NODE-04 / R04): one ingress deadline and a bounded fan-out.
const WORKSPACE_TIMEOUT_MS = 2500;
const WORKSPACE_CONCURRENCY = 2;
const scratchpadStore = new ScratchpadStore();
const CALLER_SCHEMA = {
  type: "object",
  required: ["root", "repositoryId", "scopeId"],
  properties: {
    root: { type: "string", minLength: 1 },
    repositoryId: { type: "string", minLength: 1 },
    scopeId: { type: "string", minLength: 1 },
    scopeDescriptor: { type: "object" },
  },
  additionalProperties: false,
};
const TOOL_DEFINITIONS = [
  { name: "membrane_context", description: "Federated context packet for one exact caller binding.", inputSchema: { type: "object", required: ["task", "repository", "caller"], properties: { task: { type: "string", minLength: 1, pattern: "\\S" }, repository: { type: "string" }, caller: CALLER_SCHEMA, budget: { type: "integer", minimum: 1 }, intent: { type: "string" }, session: { type: "string" }, taskId: { type: "string" }, anchors: { type: "string" }, scopeGrantId: { type: "string" }, scope: { type: "string", enum: ["repo", "workspace"], description: "\"repo\" (default): single-repo query. \"workspace\": fan out across catalog repos by alias, fuse results." }, explicitRepositoryIds: { type: "array", items: { type: "string" }, description: "MBR-004 bounded routing: workspace scope only. Exact repository ids to select even without an alias mention." }, deadlineMs: { type: "integer", minimum: 1, description: "MBR-005: optional absolute budget for the workspace fan-out in ms; one ingress deadline bounds all children." } } } },
  { name: "membrane_source_read", description: "Hash-bound DocReadV1 section fetch for one exact caller binding.", inputSchema: { type: "object", required: ["repository", "caller", "sourceRef", "anchorId", "expectedContentHash"], properties: { repository: { type: "string" }, caller: CALLER_SCHEMA, sourceRef: { type: "string" }, anchorId: { type: "string" }, expectedContentHash: { type: "string" } } } },
  { name: "membrane_knowledge_propose", description: "Submit a bounded typed KnowledgeEmission proposal for quarantine review.", inputSchema: { type: "object", required: ["repository", "caller", "emission"], properties: { repository: { type: "string" }, caller: CALLER_SCHEMA, emission: { type: "object" } } } },
  { name: "membrane_checkpoint_save", description: "Save an A0 session checkpoint for one exact caller binding; never durable knowledge.", inputSchema: { type: "object", required: ["repository", "caller", "checkpoint"], properties: { repository: { type: "string" }, caller: CALLER_SCHEMA, checkpoint: { type: "object" } } } },
  { name: "membrane_checkpoint_load", description: "Load an unexpired A0 session checkpoint for one exact caller binding.", inputSchema: { type: "object", required: ["repository", "caller", "id"], properties: { repository: { type: "string" }, caller: CALLER_SCHEMA, id: { type: "string" }, asOfMs: { type: "integer", minimum: 0 } } } },
  { name: "membrane_working_context", description: "Save, load, or close bounded session/task working context; durability must be explicit.", inputSchema: { type: "object", required: ["repository", "caller", "operation"], properties: { repository: { type: "string" }, caller: CALLER_SCHEMA, operation: { type: "string", enum: ["save", "load", "close"] }, context: { type: "object" }, sessionId: { type: "string" }, taskId: { type: "string" }, contextId: { type: "string" }, asOf: { type: "string" } } } },
  { name: "membrane_temporal_fact", description: "Record or query provenance-bound temporal facts with explicit single-valued predicate policy.", inputSchema: { type: "object", required: ["repository", "caller", "operation"], properties: { repository: { type: "string" }, caller: CALLER_SCHEMA, operation: { type: "string", enum: ["record", "query"] }, fact: { type: "object" }, singleValuedPredicates: { type: "array", items: { type: "string" } }, scopeId: { type: "string" }, subject: { type: "string" }, predicate: { type: "string" }, asOf: { type: "string" } } } },
  { name: "membrane_scratchpad", description: "Save, load, or clear ephemeral non-searchable session/task scratchpad state.", inputSchema: { type: "object", required: ["repository", "caller", "operation"], properties: { repository: { type: "string" }, caller: CALLER_SCHEMA, operation: { type: "string", enum: ["save", "load", "clear"] }, scratchpad: { type: "object" }, sessionId: { type: "string" }, taskId: { type: "string" }, asOf: { type: "string" } } } },
  { name: "membrane_feedback", description: "Record bounded receipt-bound outcome feedback for quarantine review. Self-reported outcomes are advisory (non-ranking) unless verdictRef names a resolvable cited verdict.", inputSchema: { type: "object", required: ["repository", "caller", "receiptId", "outcome"], properties: { repository: { type: "string" }, caller: CALLER_SCHEMA, receiptId: { type: "string" }, outcome: { type: "string", enum: ["used", "ignored", "contradicted"] }, verdictRef: { type: "string", minLength: 1 } } } },
];
const TOOLS = TOOL_DEFINITIONS;

const TRACE_FIELDS = {
  traceparent: { type: "string", maxLength: 128 },
  tracestate: { type: "string", maxLength: 512 },
  baggage: { type: "string", maxLength: 8192 },
};
const TOOL_OUTPUT_SCHEMA = {
  type: "object",
  required: ["data", "trace"],
  properties: {
    data: {},
    trace: {
      type: "object",
      properties: TRACE_FIELDS,
      additionalProperties: false,
    },
  },
  additionalProperties: false,
};

const protocol = `# Membrane MCP v1\n\nmembrane_context routes through the loopback /federate endpoint, never raw recall, and injects exact session/task working context when supplied. Knowledge and feedback require durable Crypt persistence with LifecycleReceiptV1 readback; unavailable persistence is a tool error unless explicit advisory policy is selected. Working context is durable only when explicitly configured. Scratchpads are ephemeral, non-searchable, non-authoritative, and never consolidated. Temporal supersession requires an explicit single-valued predicate policy. Checkpoints are A0 session orientation state. Source reads require a hash-bound DocReadV1 reference.`;

function text(value) {
  if (typeof value !== "string") return value;
  try { return JSON.parse(value); }
  catch (_) { return value; }
}
function byteLength(value) { return Buffer.byteLength(JSON.stringify(value), "utf8"); }
function bounded(value, limit, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error(`${label} must be an object`);
  if (byteLength(value) > limit) throw new Error(`${label} exceeds ${limit} bytes`);
}
function receiptId(prefix, value) { return `${prefix}-${createHash("sha256").update(JSON.stringify(value)).digest("hex").slice(0, 24)}`; }
function digest(value) { return `sha256:${createHash("sha256").update(typeof value === "string" ? value : JSON.stringify(value)).digest("hex")}`; }
function lifecycleReceipt(operation, status, durableId, eventId, readback) {
  return {
    schema: "orthic.lifecycle-receipt.v1",
    operation,
    status,
    durable_id: durableId,
    event_id: eventId,
    readback_digest: digest(readback),
    recorded_at: new Date().toISOString(),
  };
}
function feedbackReadback(dbPath, eventId, candidateId, outcome, { source, verdictRef } = {}) {
  const db = new DatabaseSync(dbPath, { readOnly: true });
  try {
    const canonicalTrace = `trace-${createHash("sha256").update(eventId).digest("hex").slice(0, 32)}`;
    const row = db.prepare("SELECT trace_id, candidate_id, content_sha256, outcome, verified, verdict_ref FROM context_feedback WHERE trace_id = ? AND candidate_id = ?").get(canonicalTrace, candidateId);
    const expectedDigest = digest(candidateId).slice("sha256:".length);
    // Verification basis, not just trace/candidate/sha/outcome equality: an advisory (agent
    // self-report) row must persist verified=0, and a cited_verdict row must persist verified=1
    // with the exact verdict_ref it was submitted with -- so a false self-claim of verification
    // can never slip past the readback.
    const expectedVerified = source === undefined ? undefined : (source === "advisory" ? 0 : 1);
    const expectedVerdictRef = source === "cited_verdict" ? verdictRef : null;
    if (
      !row || row.trace_id !== canonicalTrace || row.candidate_id !== candidateId ||
      row.content_sha256 !== expectedDigest || row.outcome !== outcome ||
      (expectedVerified !== undefined && row.verified !== expectedVerified) ||
      (source !== undefined && (row.verdict_ref ?? null) !== expectedVerdictRef)
    ) {
      throw new Error(`durable feedback independent readback mismatch: ${JSON.stringify({ dbPath, row, eventId, canonicalTrace, candidateId, expectedDigest, outcome, source, expectedVerified, expectedVerdictRef })}`);
    }
    return row;
  } finally {
    db.close();
  }
}
function callerLevel(binding) { return binding.grant_policy?.level || "read-only"; }
/** Advisory durability is a per-binding decision (grant_policy.durability) so it can never mask
 * a production binding's failure suite-wide. MEMBRANE_DURABILITY_MODE remains a process-wide
 * fallback ONLY for bindings that declare no explicit durability policy (tests, dry runs) --
 * it never overrides an explicit "durable" binding, and BindingResolutionError bypasses this
 * check entirely (see resolveInstallation). */
function advisoryPolicy(binding) {
  const durability = binding?.grant_policy?.durability;
  if (durability === "advisory") return true;
  if (durability === "durable") return false;
  return process.env.MEMBRANE_DURABILITY_MODE === "advisory";
}
/** A binding that cannot be resolved to a real installation (corrupt/stale runtime.json) is a
 * distinct hard failure from "durable store reachable but the write failed" -- only the latter
 * is eligible for advisory downgrade. */
class BindingResolutionError extends Error {}
async function resolveInstallation(binding) {
  try {
    return await installationBindingFor(binding);
  } catch (error) {
    throw new BindingResolutionError(`binding could not be resolved to a real installation: ${error instanceof Error ? error.message : String(error)}`);
  }
}
function callerDescriptor(caller) { return caller.scopeDescriptor || { kind: "filesystem", path: caller.scopeId }; }
function stableDescriptor(value) {
  if (Array.isArray(value)) return value.map(stableDescriptor);
  if (value && typeof value === "object") return Object.fromEntries(Object.keys(value).sort().map((key) => [key, stableDescriptor(value[key])]));
  return value;
}
function sameDescriptor(left, right) { return JSON.stringify(stableDescriptor(left)) === JSON.stringify(stableDescriptor(right)); }
// MBR-002 / SN-NODE-02: effective privilege is the intersection of installation,
// caller, target, child-grant, and task/session authority. When no explicit
// task/session grant travels on the envelope, the caller's own level is the
// task authority so an absent grant never cold-caps legitimate same-root work
// below the caller's persisted level.
function effectiveAuthorityFor(callerBinding, targetBinding, sameRootBinding, granted, taskGrantLevel) {
  const callerLevel = callerBinding?.grant_policy?.level || "read-only";
  const targetLevel = targetBinding?.grant_policy?.level || "read-only";
  return intersectAuthority(
    INSTALLATION_AUTHORITY_LEVEL,
    callerLevel,
    targetLevel,
    sameRootBinding ? "admin" : (granted ? targetLevel : "read-only"),
    taskGrantLevel || callerLevel,
  );
}
function takeRate(binding, action) {
  const limit = RATE_LIMITS[action];
  if (!limit) return;
  const key = `${binding.root}:${binding.scope_id}:${action}`;
  const now = Date.now();
  const hits = (rateWindows.get(key) || []).filter((at) => now - at < RATE_WINDOW_MS);
  if (hits.length >= limit) throw new Error(`${action}_rate_limited`);
  hits.push(now);
  rateWindows.set(key, hits);
}
// Live filesystem re-derivation, not a trusted stored digest: the catalog is rebuilt from the
// caller's OWN root on every check, so repository_id/role come from what actually exists on
// disk right now. Fails closed (returns false) whenever the caller carries no explicit
// child_repository_ids grant, or the catalog cannot be rebuilt at all.
async function hasCatalogChildGrant(callerBinding, targetBinding) {
  const grants = callerBinding.grant_policy?.child_repository_ids;
  if (!Array.isArray(grants) || grants.length === 0 || !grants.includes(targetBinding.repository_id)) return false;
  let catalog;
  try { catalog = await buildRepositoryCatalog(callerBinding.root); }
  catch { return false; }
  return hasExplicitChildGrant(catalog, callerBinding.repository_id, targetBinding.repository_id, grants);
}
async function authorize(args, action) {
  if (!args || typeof args !== "object" || byteLength(args) > MAX_REQUEST_BYTES) throw new Error("request exceeds bounded 32768-byte limit");
  if (typeof args.repository !== "string" || !args.repository.trim()) throw new Error("repository is required");
  const binding = await bindingFor(args.repository);
  const caller = args.caller;
  if (!caller || typeof caller !== "object" || Array.isArray(caller)) throw new Error("caller binding is required");
  if (typeof caller.root !== "string" || !caller.root.trim()) throw new Error("caller root is required");
  const callerBinding = await bindingFor(caller.root);
  const sameRootBinding = binding.root === callerBinding.root && binding.repository_id === callerBinding.repository_id && sameDescriptor(binding.scope_descriptor, callerBinding.scope_descriptor);
  // A workspace-root-bound caller may reach a distinct child repository ONLY when the
  // registry's persisted grant_policy.child_repository_ids explicitly names it AND the live
  // repository catalog agrees that target is actually a child of that same workspace. Every
  // other cross-root call keeps failing exactly as before.
  let granted = sameRootBinding;
  if (!sameRootBinding) {
    granted = await hasCatalogChildGrant(callerBinding, binding);
    if (!granted) throw new Error("cross_root_binding_denied");
  }
  // The caller must accurately self-report ITS OWN identity -- checked against callerBinding
  // (the caller's persisted registry entry), not the target's, since a granted cross-repository
  // call intentionally has binding !== callerBinding.
  if (caller.repositoryId !== callerBinding.repository_id || caller.scopeId !== callerBinding.scope_id || !sameDescriptor(callerDescriptor(caller), callerBinding.scope_descriptor)) throw new Error("caller_scope_binding_denied");
  // MBR-002 / SN-NODE-02: a read-only caller stays read-only even when the target
  // binding claims a higher (e.g. write-trusted) level; effective privilege is the
  // exhaustion/intersection, not the target's declared level alone.
  const effectiveLevel = effectiveAuthorityFor(callerBinding, binding, sameRootBinding, granted, args.taskGrantLevel);
  if (!permitsLevel(effectiveLevel, action)) throw new Error("caller_not_authorized");
  return binding;
}

function run(command, args, input, env = process.env, signal) {
  // MBR-005: if the caller already aborted, do not spawn a child at all.
  if (signal?.aborted) return Promise.resolve({ code: 124, stdout: "", stderr: signal.reason?.message || "cancelled" });
  return new Promise((resolve) => {
    const child = spawn(command, args, { windowsHide: true, stdio: ["pipe", "pipe", "pipe"], env, signal });
    const terminate = () => { try { child.kill(); } catch (_) {} };
    signal?.addEventListener("abort", terminate, { once: true });
    let stdout = "", stderr = "";
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.on("error", (error) => resolve({ code: 127, stdout, stderr: error.message }));
    child.on("close", (code) => { signal?.removeEventListener("abort", terminate); resolve({ code: code ?? 1, stdout, stderr }); });
    child.stdin.end(input);
  });
}

// MBR-005 / R04: bounded-concurrency fan-out with STABLE, index-aligned output
// order and abort-awareness. On abort it marks never-started lanes as aborted
// instead of dropping partial receipts; items already running finish (their
// run() resolves with the deadline/cancel reason) and are recorded in place.
async function runBoundedOrdered(items, limit, fn, signal) {
  const out = new Array(items.length).fill(undefined);
  let cursor = 0;
  async function worker() {
    while (true) {
      const index = cursor++;
      if (index >= items.length) return;
      if (signal?.aborted) { out[index] = { aborted: true }; continue; }
      out[index] = await fn(items[index], index);
    }
  }
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, worker));
  return out;
}

async function currentFreshness(binding, install, sessionId) {
  try {
    let token = "";
    for (const tokenPath of [install.tokenPath, join(install.workspaceRoot, "tools", ".cache", "memory", "api-token")]) {
      try {
        token = (await readFile(tokenPath, "utf8")).trim();
        if (token) break;
      } catch {
        // Match the MCP transport: an old registry path falls back to current workspace token.
      }
    }
    if (!token) return null;
    const response = await fetch(`${install.endpoint}/freshness`, {
      method: "POST",
      headers: { "content-type": "application/json", authorization: `Bearer ${token}` },
      body: JSON.stringify({ repoRoot: binding.root, repositoryId: binding.repository_id, sessionId }),
      signal: AbortSignal.timeout(SCOPE_GRANT_FRESHNESS_TIMEOUT_MS),
    });
    if (!response.ok) return null;
    const body = await response.json();
    return body && typeof body === "object" && !Array.isArray(body) ? body : null;
  } catch {
    return null;
  }
}

function cryptArgs(args, bindingRecord) {
  const db = bindingRecord?.db || process.env.CRYPT_DB || "";
  return ["--db", db, ...args].filter((v, i) => !(i === 1 && !v));
}
async function bindingEnv(binding) {
  const installation = await installationBindingFor(binding);
  return { ...process.env, ...installationEnv(installation) };
}
async function durableProposal(binding, emission) {
  const content = String(emission.text ?? emission.content ?? "").trim();
  if (!content) throw new Error("emission text is required");
  const proposalId = receiptId("proposal", { scope: binding.scope_id, emission });
  const installation = await resolveInstallation(binding);
  const store = new ProposalStore(eventDbFor(installation.db));
  let readback;
  try { readback = store.create({ proposalId, repositoryId: binding.repository_id, scopeId: binding.scope_id, emission: { ...emission, text: content } }); }
  finally { store.close(); }
  const eventId = receiptId("event", { operation: "knowledge_propose", proposalId });
  return {
    status: "needs_review",
    durable: true,
    proposalId,
    durableId: proposalId,
    reviewState: readback.state,
    lifecycleReceipt: lifecycleReceipt("knowledge_propose", "needs_review", proposalId, eventId, readback),
    provenance: { repositoryId: binding.repository_id, scopeId: binding.scope_id, callerLevel: callerLevel(binding) },
  };
}
// membrane_feedback outcomes are entirely model/agent self-reported: this tool never observed
// the downstream action itself, so it must never emit "observed_action" (that source is
// reserved for Membrane's own store-internal observations). A caller-supplied verdictRef names
// a resolvable cited verdict; absent that, the report is advisory -- persisted for
// observability but never eligible to affect ranking (matches the store's fail-safe default).
function feedbackSourceFor(args) { return args.verdictRef ? "cited_verdict" : "advisory"; }
async function durableFeedback(binding, args) {
  const source = feedbackSourceFor(args);
  try {
    const feedbackId = receiptId("feedback", { scope: binding.scope_id, receiptId: args.receiptId, outcome: args.outcome });
    const eventId = receiptId("event", { operation: "feedback", feedbackId });
    const installation = await resolveInstallation(binding);
    const env = { ...process.env, ...installationEnv(installation) };
    const binary = process.env.CRYPT_BIN || "crypt";
    const out = await run(binary, cryptArgs([
      "feedback", "--trace", eventId, "--candidate", args.receiptId,
      "--sha", digest(args.receiptId).slice("sha256:".length), "--outcome", args.outcome,
      "--source", source, "--scope", binding.scope_id,
      ...(source === "cited_verdict" ? ["--verdict-ref", args.verdictRef] : []),
    ], installation), "", env);
    if (out.code !== 0) throw new Error(out.stderr.trim() || out.stdout.trim());
    let response;
    try { response = JSON.parse(out.stdout.trim()); } catch { throw new Error("returned invalid JSON"); }
    const expectedVerified = source !== "advisory";
    if (response.ok !== true || Boolean(response.verified) !== expectedVerified) {
      throw new Error(source === "advisory" ? "advisory self-report must not be persisted as verified" : "was not independently verified");
    }
    const readback = feedbackReadback(installation.db, eventId, args.receiptId, args.outcome, { source, verdictRef: args.verdictRef });
    return {
      status: "persisted",
      durable: true,
      feedbackId,
      receiptId: args.receiptId,
      outcome: args.outcome,
      source,
      verified: expectedVerified,
      lifecycleReceipt: lifecycleReceipt("feedback", "persisted", feedbackId, eventId, readback),
      provenance: { repositoryId: binding.repository_id, scopeId: binding.scope_id, callerLevel: callerLevel(binding) },
      feedbackEvent: feedbackEvent({ eventId, receiptId: args.receiptId, outcome: args.outcome }),
      feedbackPolicy: feedbackPolicy(feedbackEvent({ eventId, receiptId: args.receiptId, outcome: args.outcome })),
    };
  } catch (error) {
    if (error instanceof BindingResolutionError) throw error;
    throw new Error(`durable feedback write failed: ${error instanceof Error ? error.message : String(error)}`);
  }
}
async function callTool(name, args, trace = {}) {
  if (name === "membrane_context") {
    const binding = await authorize(args, "context");
    const install = await installationBindingFor(binding);

    // Plan 4.2: workspace-scoped federation.
    if (args.scope === "workspace") {
      const catalog = await buildRepositoryCatalog(binding.root);
      const repos = catalog?.repositories || [];
      if (repos.length === 0) return { ok: false, error: "workspace_scope_no_repos", scope: "workspace" };
      // MBR-004 / SN-NODE-03: bounded routing. A task with no explicit or
      // mentioned target abstains instead of querying every repository.
      const routing = selectWorkspaceTargets({ catalog, task: args.task, explicitRepositoryIds: args.explicitRepositoryIds || [] });
      if (routing.status === "abstained") {
        return text({ ok: false, scope: "workspace", error: routing.reason, considered: routing.considered });
      }
      const selected = routing.targets;
      const budget = Number.isInteger(args.budget) ? args.budget : 4096;
      const perRepoBudget = Math.max(1, Math.floor(budget / selected.length));
      const fused = { ok: true, scope: "workspace", repos: [], totalCandidates: 0, totalOmissions: 0 };
      const trace = { traceparent: args.traceparent, tracestate: args.tracestate, baggage: args.baggage };
      const workspaceGrants = binding.grant_policy?.child_repository_ids;
      const catalogRootId = repos.find((entry) => entry.role === "workspace-root")?.repository_id;
      // MBR-003 authorizes EVERY target independently (same primitive as direct
      // calls). MBR-005 (SN-NODE-04 / R04) bounds the whole fan-out by ONE
      // absolute monotonic ingress deadline with bounded concurrency; receipts
      // stay in stable repository order. Denied/aborted/expired targets become
      // typed terminal omissions, never collapsed into federation_error.
      const deadlineMs = createDeadline(Number.isInteger(args.deadlineMs) ? args.deadlineMs : WORKSPACE_TIMEOUT_MS);
      const lane = deadlineSignal(deadlineMs);
      const abortRow = (entry) => ({ row: { repoId: entry.repository_id, basis: "aborted", generationId: null, candidates: 0, omissions: [terminalReason(lane.signal) || "cancelled"] }, omissions: 1, candidates: 0 });
      const deniedRow = (entry) => ({ row: { repoId: entry.repository_id, basis: "denied", generationId: null, candidates: 0, omissions: ["target_denied"] }, omissions: 1, candidates: 0 });
      const results = await runBoundedOrdered(selected, WORKSPACE_CONCURRENCY, async (entry) => {
        if (lane.signal.aborted) return abortRow(entry);
        const targetRoot = resolve(binding.root, entry.root === "." ? "." : entry.root);
        let targetBinding;
        try { targetBinding = await bindingFor(targetRoot); } catch { return deniedRow(entry); }
        const hasGrant = hasExplicitChildGrant(catalog, catalogRootId, entry.repository_id, workspaceGrants);
        const effectiveLevel = await canReachTarget({
          callerBinding: binding,
          targetBinding,
          action: "context",
          taskGrantLevel: args.taskGrantLevel,
          hasExplicitChildGrant: hasGrant,
        });
        if (!effectiveLevel) return deniedRow(entry);
        const request = { task: args.task, repo: targetRoot, maxTokens: perRepoBudget, intent: args.intent, session: args.session, anchors: args.anchors, scopeGrantId: args.scopeGrantId, scopeDescriptor: binding.scope_descriptor, ...trace };
        const out = await run(process.execPath, [CLIENT, "--input", "-"], JSON.stringify(request), { ...await bindingEnv(binding), WORKSPACE_ROOT: targetRoot, MEMBRANE_DEADLINE_AT_MS: String(deadlineMs) }, lane.signal);
        const reason = terminalReason(lane.signal);
        if (reason) return { row: { repoId: entry.repository_id, basis: "aborted", generationId: null, candidates: 0, omissions: [reason] }, omissions: 1, candidates: 0 };
        const packet = text(out.stdout.trim() || "");
        const degraded = Boolean(packet?.degradationReason && packet.degradationReason !== "none");
        return {
          row: {
            repoId: entry.repository_id,
            generationId: packet?.packet?.freshness?.revision || null,
            candidates: (packet?.packet?.blocks || []).length,
            omissions: degraded ? [packet.degradationReason] : [],
          },
          omissions: degraded ? 1 : 0,
          candidates: (packet?.packet?.blocks || []).length,
        };
      }, lane.signal);
      for (const item of results) {
        if (!item) continue;
        fused.repos.push(item.row);
        fused.totalOmissions += item.omissions;
        fused.totalCandidates += item.candidates;
      }
      fused.repos.sort((a, b) => String(a.repoId).localeCompare(String(b.repoId))); // stable receipt order by repository id
      if (lane.signal.aborted) fused.deadlineExceeded = true;
      lane.close();
      return text(fused);
    }

    // Single-repo path (default).
    const request = { task: args.task, repo: binding.root, maxTokens: args.budget, intent: args.intent, session: args.session, anchors: args.anchors, scopeGrantId: args.scopeGrantId, scopeDescriptor: binding.scope_descriptor, ...trace };
    const out = await run(process.execPath, [CLIENT, "--input", "-"], JSON.stringify(request), { ...await bindingEnv(binding), WORKSPACE_ROOT: binding.root });
    const packet = text(out.stdout.trim() || { status: "unavailable", error: out.stderr.slice(0, 240) });
    if (!packet || typeof packet !== "object" || Array.isArray(packet)) return packet;
    if (args.session && args.taskId && packet.ok === true && packet.packet && typeof packet.packet === "object") {
      const freshness = await currentFreshness(binding, install, args.session);
      const scopeGrant = mintScopeGrantV1({ binding, args, packet: packet.packet, freshness });
      if (scopeGrant) packet.scopeGrant = scopeGrant;
    }
    if (!args.session || !args.taskId) return packet;
    const store = new WorkingContextStore(eventDbFor(install.db));
    let contexts;
    try { contexts = store.activeContexts({ sessionId: args.session, taskId: args.taskId }); }
    finally { store.close(); }
    const budget = Number.isInteger(args.budget) ? args.budget : 4096;
    let used = 0;
    const workingContexts = contexts.filter((context) => {
      const cost = Math.ceil(byteLength(context.items) / 4);
      if (used + cost > budget) return false;
      used += cost;
      return true;
    });
    return { ...packet, working_contexts: workingContexts };
  }
  if (name === "membrane_source_read") {
    const binding = await authorize(args, "source_read");
    const install = await installationBindingFor(binding);
    const out = await run(process.env.CRYPT_BIN || "crypt", cryptArgs(["doc", "read", "--source-ref", args.sourceRef, "--anchor", args.anchorId, "--expected-hash", args.expectedContentHash], install), "", await bindingEnv(binding));
    return text(out.stdout.trim() || { error: "source_read_unavailable", detail: out.stderr.slice(0, 240) });
  }
  if (name === "membrane_checkpoint_save") {
    const binding = await authorize(args, "checkpoint");
    const install = await installationBindingFor(binding);
    bounded(args.checkpoint, MAX_PROPOSAL_BYTES, "checkpoint");
    takeRate(binding, "checkpoint");
    const out = await run(process.env.CRYPT_BIN || "crypt", cryptArgs(["checkpoint", "save"], install), JSON.stringify(args.checkpoint), await bindingEnv(binding));
    return text(out.stdout.trim() || { error: "checkpoint_save_unavailable", detail: out.stderr.slice(0, 240) });
  }
  if (name === "membrane_checkpoint_load") {
    const binding = await authorize(args, "checkpoint_load");
    const install = await installationBindingFor(binding);
    const params = ["checkpoint", "load", args.id];
    if (Number.isInteger(args.asOfMs)) params.push("--as-of-ms", String(args.asOfMs));
    const out = await run(process.env.CRYPT_BIN || "crypt", cryptArgs(params, install), "", await bindingEnv(binding));
    return text(out.stdout.trim() || { error: "checkpoint_load_unavailable", detail: out.stderr.slice(0, 240) });
  }
  if (name === "membrane_working_context") {
    const operation = args.operation;
    const binding = await authorize(args, operation === "load" ? "working_context_load" : "checkpoint");
    const install = await installationBindingFor(binding);
    const store = new WorkingContextStore(eventDbFor(install.db));
    try {
      if (operation === "save") {
        bounded(args.context, MAX_PROPOSAL_BYTES, "working context");
        takeRate(binding, "checkpoint");
        return { status: "saved", context: store.saveContext(args.context) };
      }
      if (operation === "load") {
        if (!args.sessionId || !args.taskId) throw new Error("working_context_scope_required");
        return { status: "loaded", contexts: store.activeContexts({ sessionId: args.sessionId, taskId: args.taskId, ...(args.asOf ? { asOf: args.asOf } : {}) }) };
      }
      if (operation === "close") {
        if (!args.contextId) throw new Error("working_context_id_required");
        return { status: "closed", contextId: args.contextId, closed: store.closeContext(args.contextId) };
      }
      throw new Error("working_context_operation_invalid");
    } finally { store.close(); }
  }
  if (name === "membrane_temporal_fact") {
    const operation = args.operation;
    const binding = await authorize(args, operation === "query" ? "temporal_fact_query" : "checkpoint");
    const install = await installationBindingFor(binding);
    const store = new WorkingContextStore(eventDbFor(install.db));
    try {
      if (operation === "record") {
        bounded(args.fact, MAX_PROPOSAL_BYTES, "temporal fact");
        if (args.fact.scopeId !== binding.scope_id) throw new Error("temporal_fact_scope_denied");
        takeRate(binding, "checkpoint");
        return { status: "recorded", fact: store.recordTemporalFact(args.fact, { singleValuedPredicates: args.singleValuedPredicates || [] }) };
      }
      if (operation === "query") {
        if (args.scopeId !== binding.scope_id || !args.subject || !args.predicate || !args.asOf) throw new Error("temporal_fact_query_invalid");
        return { status: "loaded", facts: store.temporalFactsAsOf({ scopeId: args.scopeId, subject: args.subject, predicate: args.predicate, asOf: args.asOf }) };
      }
      throw new Error("temporal_fact_operation_invalid");
    } finally { store.close(); }
  }
  if (name === "membrane_scratchpad") {
    const operation = args.operation;
    const binding = await authorize(args, operation === "load" ? "scratchpad_load" : "checkpoint");
    const install = await installationBindingFor(binding);
    const path = eventDbFor(install.db);
    if (operation === "save") {
      bounded(args.scratchpad, MAX_PROPOSAL_BYTES, "scratchpad");
      takeRate(binding, "checkpoint");
      return { status: "saved_ephemeral", scratchpad: scratchpadStore.save(path, args.scratchpad) };
    }
    if (!args.sessionId || !args.taskId) throw new Error("scratchpad_scope_required");
    if (operation === "load") return { status: "loaded", scratchpad: scratchpadStore.load(path, { sessionId: args.sessionId, taskId: args.taskId, ...(args.asOf ? { asOf: args.asOf } : {}) }) };
    if (operation === "clear") return { status: "cleared", cleared: scratchpadStore.clear(path, { sessionId: args.sessionId, taskId: args.taskId }) };
    throw new Error("scratchpad_operation_invalid");
  }
  if (name === "membrane_knowledge_propose") {
    const binding = await authorize(args, "proposal");
    bounded(args.emission, MAX_PROPOSAL_BYTES, "emission");
    takeRate(binding, "proposal");
    try { return await durableProposal(binding, args.emission); }
    catch (error) {
      // An unresolvable binding is never eligible for advisory downgrade -- only a durable
      // store that is reachable but whose write failed can be quarantined-ephemeral.
      if (error instanceof BindingResolutionError) throw error;
      if (!advisoryPolicy(binding)) throw error;
      const proposalId = receiptId("proposal", { scope: binding.scope_id, emission: args.emission });
      return text({ status: "quarantined_ephemeral", durable: false, proposalId, lifecycleReceipt: lifecycleReceipt("knowledge_propose", "quarantined_ephemeral", proposalId, receiptId("event", { operation: "knowledge_propose", proposalId }), args.emission), provenance: { repositoryId: binding.repository_id, scopeId: binding.scope_id, callerLevel: callerLevel(binding) } });
    }
  }
  if (name === "membrane_feedback") {
    const binding = await authorize(args, "feedback");
    if (typeof args.receiptId !== "string" || !args.receiptId.trim() || !["used", "ignored", "contradicted"].includes(args.outcome)) throw new Error("invalid_feedback");
    if (args.verdictRef !== undefined && (typeof args.verdictRef !== "string" || !args.verdictRef.trim())) throw new Error("invalid_feedback_verdict_ref");
    if (byteLength({ receiptId: args.receiptId, outcome: args.outcome, ...(args.verdictRef ? { verdictRef: args.verdictRef } : {}) }) > MAX_FEEDBACK_BYTES) throw new Error("feedback exceeds 2048 bytes");
    takeRate(binding, "feedback");
    try { return await durableFeedback(binding, args); }
    catch (error) {
      // An unresolvable binding is never eligible for advisory downgrade -- only a durable
      // store that is reachable but whose write failed can be accepted-advisory.
      if (error instanceof BindingResolutionError) throw error;
      if (!advisoryPolicy(binding)) throw error;
      const feedbackId = receiptId("feedback", { scope: binding.scope_id, receiptId: args.receiptId, outcome: args.outcome });
      return text({ status: "accepted_advisory", durable: false, feedbackId, receiptId: args.receiptId, outcome: args.outcome, lifecycleReceipt: lifecycleReceipt("feedback", "accepted_advisory", feedbackId, receiptId("event", { operation: "feedback", feedbackId }), { receiptId: args.receiptId, outcome: args.outcome }), feedbackEvent: feedbackEvent({ eventId: receiptId("event", { operation: "feedback", feedbackId }), receiptId: args.receiptId, outcome: args.outcome }), feedbackPolicy: feedbackPolicy(feedbackEvent({ eventId: receiptId("event", { operation: "feedback", feedbackId }), receiptId: args.receiptId, outcome: args.outcome })), provenance: { repositoryId: binding.repository_id, scopeId: binding.scope_id, callerLevel: callerLevel(binding) } });
    }
  }
  throw new Error("unknown tool");
}

function validTraceparent(value) {
  if (typeof value !== "string" || Buffer.byteLength(value, "utf8") > 128) return false;
  const match = /^(?<version>[0-9a-f]{2})-(?<trace>[0-9a-f]{32})-(?<parent>[0-9a-f]{16})-(?<flags>[0-9a-f]{2})(?<extension>(?:-[\x21-\x7E]+)*)$/.exec(value);
  return Boolean(match && match.groups.version !== "ff" && (match.groups.version !== "00" || !match.groups.extension) && !/^0+$/.test(match.groups.trace) && !/^0+$/.test(match.groups.parent));
}
const TRACESTATE_SIMPLE_KEY = /^[a-z][a-z0-9_*/-]{0,255}$/;
const TRACESTATE_MULTI_KEY = /^[a-z0-9][a-z0-9_*/-]{0,240}@[a-z][a-z0-9_*/-]{0,13}$/;
const TRACESTATE_VALUE = /^[\x20-\x2B\x2D-\x3C\x3E-\x7E]+$/;
const BAGGAGE_TOKEN = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;
const BAGGAGE_VALUE = /^[\x21\x23-\x2B\x2D-\x3A\x3C-\x5B\x5D-\x7E]*$/;
const trimOws = (value) => value.replace(/^[ \t]+|[ \t]+$/g, "");
function validTracestateMember(member, keys) {
  const value = trimOws(member);
  if (!value) return true;
  const separator = value.indexOf("=");
  if (separator <= 0 || value.indexOf("=", separator + 1) !== -1) return false;
  const stateValue = value.slice(separator + 1).replace(/[ \t]+$/, "");
  const key = value.slice(0, separator);
  if (!TRACESTATE_SIMPLE_KEY.test(key) && !TRACESTATE_MULTI_KEY.test(key)) return false;
  if (stateValue.length > 256 || !TRACESTATE_VALUE.test(stateValue) || stateValue.endsWith(" ") || keys.has(key)) return false;
  keys.add(key);
  return true;
}
function validTracestate(value) {
  if (typeof value !== "string" || Buffer.byteLength(value, "utf8") > 512) return false;
  const members = value.split(",");
  const keys = new Set();
  return members.length <= 32 && members.every((member) => validTracestateMember(member, keys));
}
function validBaggageProperty(value) {
  const property = trimOws(value);
  const separator = property.indexOf("=");
  if (separator === -1) return BAGGAGE_TOKEN.test(property);
  const key = property.slice(0, separator).replace(/[ \t]+$/, "");
  const propertyValue = property.slice(separator + 1).replace(/^[ \t]+|[ \t]+$/g, "");
  return BAGGAGE_TOKEN.test(key) && BAGGAGE_VALUE.test(propertyValue);
}
function validBaggageMember(member) {
  const value = trimOws(member);
  const separator = value.indexOf("=");
  if (separator <= 0) return false;
  const key = value.slice(0, separator).replace(/[ \t]+$/, "");
  const [baggageValue, ...properties] = value.slice(separator + 1).replace(/^[ \t]+/, "").split(";");
  return BAGGAGE_TOKEN.test(key) && BAGGAGE_VALUE.test(baggageValue.replace(/[ \t]+$/, "")) && properties.every(validBaggageProperty);
}
function validBaggage(value) {
  if (typeof value !== "string" || !value || Buffer.byteLength(value, "utf8") > 8192) return false;
  const members = value.split(",");
  return members.length <= 64 && members.every(validBaggageMember);
}
function boundedTrace(args = {}) {
  const trace = {};
  const hasTraceparent = validTraceparent(args.traceparent);
  if (hasTraceparent) trace.traceparent = args.traceparent;
  if (hasTraceparent && validTracestate(args.tracestate)) trace.tracestate = args.tracestate;
  if (validBaggage(args.baggage)) trace.baggage = args.baggage;
  return trace;
}
function structuredResult(data, trace = {}) {
  const structuredContent = { data, trace: boundedTrace(trace) };
  return {
    content: [{ type: "text", text: typeof data === "string" ? data : JSON.stringify(data) }],
    structuredContent,
    isError: false,
  };
}
export function buildServer() {
  const server = new McpServer(
    { name: "membrane", version: "1.0.0" },
    { instructions: "Use membrane_context for federated context through /federate. Never expect raw memory CRUD." },
  );
  for (const tool of TOOLS) {
    server.registerTool(tool.name, {
      description: tool.description,
      inputSchema: fromJsonSchema(tool.inputSchema),
      outputSchema: fromJsonSchema(TOOL_OUTPUT_SCHEMA),
    }, async (args, ctx) => {
      const trace = boundedTrace(ctx.mcpReq._meta);
      return structuredResult(await callTool(tool.name, args, trace), trace);
    });
  }
  server.registerResource("Membrane protocol v1", PROTOCOL_URI, { mimeType: "text/markdown" }, async (uri) => {
    return { contents: [{ uri: uri.href, mimeType: "text/markdown", text: protocol }] };
  });
  return server;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  serveStdio(() => buildServer(), { onerror: (error) => process.stderr.write(`membrane-mcp: ${error.message}\n`) });
}

export { TOOLS, TOOL_OUTPUT_SCHEMA };
