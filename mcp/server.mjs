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
import { createDeadline, deadlineSignal, mapConcurrent, terminalReason, timeoutReceipt } from "./deadline.mjs";
import { boundedLifecycleId, createLifecycle, withCancellationGrace } from "./lifecycle.mjs";
import { toolsetNames } from "./toolsets.mjs";
import { executeCodeBatch } from "../schemas/registry/code/mbr402-batch.mjs";
import { requestBlueprint } from "./blueprint-readiness.mjs";
import { diagnosticsRequest } from "./lib/diagnostics-client.mjs";

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
const DIAGNOSTIC_ID = /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/;
const CAPABILITY_VOCABULARY = ["syntax", "repository_module_resolution", "import_export_binding", "name_resolution", "type_semantics", "configured_static_policy", "compiler_project_semantics", "generated_source_awareness"];
const COST_CLASSES = ["instant", "interactive", "verification", "build", "test"];
const EPOCH_ORIGINS = ["transactional", "observed_hook", "reconciliation"];
const LAST_SNAPSHOTS = new Map();
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
  { name: "membrane_context", description: "Use when you need a federated context packet for one exact caller binding. Do not use for raw memory CRUD, arbitrary filesystem reads, or bypassing repository-bound access.", inputSchema: { type: "object", required: ["task", "repository", "caller"], properties: { task: { type: "string", minLength: 1, pattern: "\\S" }, repository: { type: "string" }, caller: CALLER_SCHEMA, budget: { type: "integer", minimum: 1 }, intent: { type: "string" }, session: { type: "string" }, taskId: { type: "string" }, anchors: { type: "string" }, scopeGrantId: { type: "string" }, scope: { type: "string", enum: ["repo", "workspace"], description: "\"repo\" (default): single-repo query. \"workspace\": fan out across catalog repos by alias, fuse results." }, explicitRepositoryIds: { type: "array", items: { type: "string" }, description: "MBR-004 bounded routing: workspace scope only. Exact repository ids to select even without an alias mention." }, deadlineMs: { type: "integer", minimum: 1, description: "MBR-005: optional absolute budget for the workspace fan-out in ms; one ingress deadline bounds all children." }, taskEnvelope: { type: "object", description: "MBR-007: membrane.task-envelope.v1 identity preserved end to end." }, turnEnvelope: { type: "object", description: "MBR-007: membrane.turn-envelope.v1 identity preserved end to end." }, clientEnvelope: { type: "object", description: "MBR-007: membrane.client-envelope.v1 identity." }, overlay: { type: "object", description: "MBR-007: membrane.overlay-identity.v1 worktree/session overlay." } } } },
  { name: "membrane_source_read", description: "Hash-bound DocReadV1 section fetch for one exact caller binding.", inputSchema: { type: "object", required: ["repository", "caller", "sourceRef", "anchorId", "expectedContentHash"], properties: { repository: { type: "string" }, caller: CALLER_SCHEMA, sourceRef: { type: "string" }, anchorId: { type: "string" }, expectedContentHash: { type: "string" } } } },
  { name: "membrane_blueprint", description: "Bounded Blueprint architecture, symbol, reference, impact, or read-only snapshot view with generation freshness and source-hash resolver handles.", inputSchema: { type: "object", required: ["repository", "caller", "operation"], additionalProperties: false, properties: { repository: { type: "string" }, caller: CALLER_SCHEMA, operation: { type: "string", enum: ["architecture", "symbol", "reference", "references", "impact", "changes", "snapshot_get", "snapshot_list", "changes_since"] }, node: { type: "string", minLength: 1, maxLength: 256, pattern: "^[A-Za-z0-9_.$:/-]+$" }, name: { type: "string", minLength: 1, maxLength: 128, pattern: "^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$" }, depth: { type: "integer", minimum: 1, maximum: 5 }, limit: { type: "integer", minimum: 1, maximum: 100 }, budget: { type: "integer", minimum: 1, maximum: 10000 }, deadlineMs: { type: "integer", minimum: 1, maximum: 5000 }, items: { type: "array", minItems: 1, maxItems: 50, items: { type: "object" } } }, oneOf: [{ properties: { operation: { enum: ["architecture", "changes", "snapshot_get", "snapshot_list", "changes_since"] } }, not: { required: ["node"] } }, { properties: { operation: { enum: ["symbol", "reference", "references", "impact"] } }, required: ["node"] }] } },
  { name: "membrane_knowledge_propose", description: "Submit a bounded typed KnowledgeEmission proposal for quarantine review.", inputSchema: { type: "object", required: ["repository", "caller", "emission"], properties: { repository: { type: "string" }, caller: CALLER_SCHEMA, emission: { type: "object" } } } },
  { name: "membrane_checkpoint_save", description: "Save an A0 session checkpoint for one exact caller binding; never durable knowledge.", inputSchema: { type: "object", required: ["repository", "caller", "checkpoint"], properties: { repository: { type: "string" }, caller: CALLER_SCHEMA, checkpoint: { type: "object" } } } },
  { name: "membrane_checkpoint_load", description: "Load an unexpired A0 session checkpoint for one exact caller binding.", inputSchema: { type: "object", required: ["repository", "caller", "id"], properties: { repository: { type: "string" }, caller: CALLER_SCHEMA, id: { type: "string" }, asOfMs: { type: "integer", minimum: 0 } } } },
  { name: "membrane_working_context", description: "Save, load, or close bounded session/task working context; durability must be explicit.", inputSchema: { type: "object", required: ["repository", "caller", "operation"], properties: { repository: { type: "string" }, caller: CALLER_SCHEMA, operation: { type: "string", enum: ["save", "load", "close"] }, context: { type: "object" }, sessionId: { type: "string" }, taskId: { type: "string" }, contextId: { type: "string" }, asOf: { type: "string" }, cursor: { type: "string", maxLength: 512 }, limit: { type: "integer", minimum: 1, maximum: 100 } } } },
  { name: "membrane_temporal_fact", description: "Record or query provenance-bound temporal facts with explicit single-valued predicate policy.", inputSchema: { type: "object", required: ["repository", "caller", "operation"], properties: { repository: { type: "string" }, caller: CALLER_SCHEMA, operation: { type: "string", enum: ["record", "query"] }, fact: { type: "object" }, singleValuedPredicates: { type: "array", items: { type: "string" } }, scopeId: { type: "string" }, subject: { type: "string" }, predicate: { type: "string" }, asOf: { type: "string" } } } },
  { name: "membrane_scratchpad", description: "Save, load, or clear ephemeral non-searchable session/task scratchpad state.", inputSchema: { type: "object", required: ["repository", "caller", "operation"], properties: { repository: { type: "string" }, caller: CALLER_SCHEMA, operation: { type: "string", enum: ["save", "load", "clear"] }, scratchpad: { type: "object" }, sessionId: { type: "string" }, taskId: { type: "string" }, asOf: { type: "string" } } } },
  { name: "membrane_feedback", description: "Record bounded receipt-bound outcome feedback for quarantine review. Self-reported outcomes are advisory (non-ranking) unless verdictRef names a resolvable cited verdict.", inputSchema: { type: "object", required: ["repository", "caller", "receiptId", "outcome"], properties: { repository: { type: "string" }, caller: CALLER_SCHEMA, receiptId: { type: "string" }, outcome: { type: "string", enum: ["used", "ignored", "contradicted"] }, verdictRef: { type: "string", minLength: 1 } } } },
  { name: "membrane_diagnostic_workspace", description: "Open, close, inspect, or reconcile one live-diagnostics workspace session on the resident Membrane service. status reads session state; reconcile proves exact current worktree bytes for reconciliation_only hosts and any mismatch against the latest cleared epoch classifies unknown_conflict or superseded, invalidating prior clearance. open binds one canonical absolute projectRoot (design §3 WorkspaceEngineKey); same repo/worktree + different root is a typed conflict, uncanonicalizable root is rejected.", annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: false }, inputSchema: { type: "object", required: ["operation"], additionalProperties: false, properties: { operation: { type: "string", enum: ["open", "close", "status", "reconcile"] }, repoId: { type: "string", minLength: 1, maxLength: 128 }, worktreeId: { type: "string", minLength: 1, maxLength: 128 }, projectRoot: { type: "string", minLength: 1, maxLength: 1024, description: "Canonical absolute worktree/project root to bind at open (design §3). Same repo/worktree + different canonical root is a typed conflict; uncanonicalizable root is rejected." }, manifestDigest: { type: "string", minLength: 1, maxLength: 256 }, hashes: { type: "array", minItems: 0, maxItems: 4096, items: { type: "object", required: ["path", "hash"], additionalProperties: false, properties: { path: { type: "string", minLength: 1 }, hash: { type: "string", minLength: 1 } } } } }, oneOf: [{ properties: { operation: { enum: ["open"] } }, required: ["repoId", "worktreeId", "projectRoot"] , not: { anyOf: [{ required: ["manifestDigest"] }, { required: ["hashes"] }] } }, { properties: { operation: { enum: ["close"] } }, required: ["repoId", "worktreeId"], not: { anyOf: [{ required: ["manifestDigest"] }, { required: ["hashes"] }] } }, { properties: { operation: { enum: ["status"] } }, required: ["repoId", "worktreeId"], not: { anyOf: [{ required: ["manifestDigest"] }, { required: ["hashes"] }] } }, { properties: { operation: { const: "reconcile" } }, required: ["repoId", "worktreeId", "manifestDigest", "hashes"] }] } },
  { name: "membrane_diagnostic_mutation", description: "Transactionally begin or seal one coherent mutation batch, or register exact observed resulting bytes (registerObserved with observed_hook origin) for hosts without edit transactions. Seal/register invalidate stale clearance. Never blocks or rolls back writes: the fence gates semantic acceptance, not disk persistence.", annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: false }, inputSchema: { type: "object", required: ["operation", "repoId", "worktreeId"], additionalProperties: false, properties: { operation: { type: "string", enum: ["begin", "seal", "registerObserved"] }, repoId: { type: "string", minLength: 1, maxLength: 128 }, worktreeId: { type: "string", minLength: 1, maxLength: 128 }, epoch: { type: "object", description: "WorkspaceEpochV1 envelope (workspace-epoch.v1) bound to this repoId/worktreeId; origin transactional for seal, observed_hook for registerObserved." } }, oneOf: [{ properties: { operation: { const: "begin" } }, not: { required: ["epoch"] } }, { properties: { operation: { enum: ["seal", "registerObserved"] } }, required: ["epoch"] }] } },
  { name: "membrane_diagnostic_snapshot", description: "Await a mutation-bound evidence snapshot plus planner gate decision (the operational fence path), or read get/explain/delta views of the last awaited snapshot cached per repoId:worktreeId in this server process. Events and presentation never clear the fence; only snapshot-await (and resident-side fence evaluation) produces operational decisions. get/explain/delta are cached views, never re-evaluation.", annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: false }, inputSchema: { type: "object", required: ["operation"], additionalProperties: false, properties: { operation: { type: "string", enum: ["await", "get", "explain", "delta"] }, repoId: { type: "string", minLength: 1, maxLength: 128 }, worktreeId: { type: "string", minLength: 1, maxLength: 128 }, policyProfileName: { type: "string", minLength: 1, maxLength: 128 }, requiredCapabilities: { type: "array", minItems: 0, maxItems: 8, items: { type: "string", enum: CAPABILITY_VOCABULARY } }, maxCost: { type: "string", enum: COST_CLASSES, description: "Hard acquisition ceiling; defaults to interactive." }, deadlineMs: { type: "integer", minimum: 1, maximum: 60000, description: "Absolute wait budget for await; defaults to 10000." } }, oneOf: [{ properties: { operation: { const: "await" } }, required: ["repoId", "worktreeId", "policyProfileName"] }, { properties: { operation: { enum: ["get", "explain", "delta"] } }, required: ["repoId", "worktreeId"], not: { anyOf: [{ required: ["policyProfileName"] }, { required: ["requiredCapabilities"] }, { required: ["maxCost"] }, { required: ["deadlineMs"] }] } }] } },
  { name: "membrane_diagnostic_fence", description: "Pure Semantic Edit Fence evaluation: sends the exact DiagnosticEvidenceSnapshotV1, expected WorkspaceEpochV1 envelope, and planner-owned GatePolicyProfileV1 to the resident deterministic evaluator and returns DiagnosticGateDecisionV1 verbatim. It invents no policy, performs no provider acquisition, and never clears the resident fence by itself; the coding host enforces the returned decision.", annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true }, inputSchema: { type: "object", required: ["snapshot", "expectedEpoch", "policy"], additionalProperties: false, properties: { snapshot: { type: "object", description: "diagnostic-evidence-snapshot.v1 envelope." }, expectedEpoch: { type: "object", description: "workspace-epoch.v1 envelope the snapshot must match exactly." }, policy: { type: "object", description: "Planner-owned GatePolicyProfileV1: profileName, policyVersion, policyDigest, blockingCodes, requiredCapabilities." } } } },
  { name: "membrane_diagnostic_capabilities", description: "Read the resident live-diagnostics capability advertisement: qualified providers, cost classes, and supported semantic capabilities. Read-only.", annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true }, inputSchema: { type: "object", required: [], additionalProperties: false, properties: {} } },
  { name: "membrane_diagnostic_baseline", description: "Capture or update a named diagnostics baseline for a workspace session; subsequent snapshot deltas classify issues as new, persistent, resolved, moved, changed, or unknown_baseline against it.", annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: true }, inputSchema: { type: "object", required: ["operation", "repoId", "worktreeId", "name"], additionalProperties: false, properties: { operation: { type: "string", enum: ["capture", "update"] }, repoId: { type: "string", minLength: 1, maxLength: 128 }, worktreeId: { type: "string", minLength: 1, maxLength: 128 }, name: { type: "string", minLength: 1, maxLength: 128 } } } },
  { name: "membrane_diagnostic_provider", description: "List qualified providers (capabilities view), read resident supervisor health/status, or restart one supervised engine by workspace-engine key digest. list/status are read-only views; restart is a lifecycle action performed by the resident supervisor.", annotations: { readOnlyHint: false, destructiveHint: true, idempotentHint: false }, inputSchema: { type: "object", required: ["operation"], additionalProperties: false, properties: { operation: { type: "string", enum: ["list", "status", "restart"] }, keyDigest: { type: "string", minLength: 1, maxLength: 256, description: "WorkspaceEngineKey digest identifying the engine to restart." } }, oneOf: [{ properties: { operation: { const: "restart" } }, required: ["keyDigest"] }, { properties: { operation: { enum: ["list", "status"] } }, not: { required: ["keyDigest"] } }] } },
];
const TOOLS = TOOL_DEFINITIONS.map((tool) => ({
  ...tool,
  inputSchema: { ...tool.inputSchema, additionalProperties: false },
  annotations: tool.annotations ?? { readOnlyHint: tool.name === "membrane_context" || tool.name === "membrane_source_read" || tool.name === "membrane_blueprint", destructiveHint: false, idempotentHint: tool.name === "membrane_context" || tool.name === "membrane_source_read" || tool.name === "membrane_blueprint" },
}));

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

const protocol = `# Membrane MCP v1\n\nmembrane_context routes through the loopback /federate endpoint, never raw recall, and injects exact session/task working context when supplied. Knowledge and feedback require durable Cortex persistence with LifecycleReceiptV1 readback; unavailable persistence is a tool error unless explicit advisory policy is selected. Working context is durable only when explicitly configured. Scratchpads are ephemeral, non-searchable, non-authoritative, and never consolidated. Temporal supersession requires an explicit single-valued predicate policy. Checkpoints are A0 session orientation state. Source reads require a hash-bound DocReadV1 reference.`;

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
    schema: "membrane.lifecycle-receipt.v1",
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
// caller's OWN root on every check, so binding identity comes from what actually exists on
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

// MBR-006 / R05: canonical generation + manifest identity. Reads ONLY
// freshness.generationId (never the retired freshness.revision hub identity);
// falls back to the catalog identity captured in this same request snapshot;
// otherwise reports a typed unknown reason.
function repositoryIdentity(packet, entry) {
  const freshness = packet?.packet?.freshness || {};
  const generationId = freshness.generationId ?? entry?.blueprintGenerationId ?? null;
  const manifestDigest = freshness.manifestDigest ?? entry?.manifestDigest ?? null;
  const sourceCommit = freshness.sourceCommit ?? entry?.sourceCommit ?? "";
  const identityStatus = generationId ? "known" : "unknown";
  const identityReason = identityStatus === "known" ? null : "generation_identity_unavailable";
  return { generationId, manifestDigest, sourceCommit, identityStatus, identityReason };
}

// MBR-007 / R06: build exact versioned envelopes and preserve them byte-for-byte
// across the request. Missing fields stay null/unknown; identities are never
// synthesized from unrelated deployment variables.
function requestEnvelopeFor(args) {
  const task = args.taskEnvelope || {};
  const turn = args.turnEnvelope || {};
  const clientEnv = args.clientEnvelope || {};
  const overlay = args.overlay || {};
  const sessionId = turn.sessionId || args.session || null;
  return {
    taskEnvelope: {
      schema: "membrane.task-envelope.v1",
      taskId: task.taskId || args.taskId || null,
      text: task.text || args.task || null,
      intent: task.intent || args.intent || null,
    },
    turnEnvelope: {
      schema: "membrane.turn-envelope.v1",
      turnId: turn.turnId || null,
      sessionId,
      sequence: Number.isInteger(turn.sequence) ? turn.sequence : null,
    },
    clientEnvelope: {
      schema: "membrane.client-envelope.v1",
      clientId: clientEnv.clientId || args.client || "mcp",
      adapterVersion: clientEnv.adapterVersion || null,
    },
    overlay: {
      schema: "membrane.overlay-identity.v1",
      sessionId: overlay.sessionId || sessionId,
      worktreePath: overlay.worktreePath || args.repo || args.repository || null,
    },
  };
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

async function currentFreshness(binding, install, sessionId, worktreePath) {
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
      body: JSON.stringify({ repoRoot: binding.root, repositoryId: binding.repository_id, sessionId, worktreePath: worktreePath || binding.root }),
      signal: AbortSignal.timeout(SCOPE_GRANT_FRESHNESS_TIMEOUT_MS),
    });
    if (!response.ok) return null;
    const body = await response.json();
    return body && typeof body === "object" && !Array.isArray(body) ? body : null;
  } catch {
    return null;
  }
}

function cortexArgs(args, bindingRecord) {
  const db = bindingRecord?.db || process.env.CORTEX_DB || "";
  return ["--db", db, ...args].filter((v, i) => !(i === 1 && !v));
}
function durableCli(binding, installation, args) {
  // Cortex remains the durable-memory owner, but its resident binary was retired.
  // Membrane's single binary exposes the durable projection behind `cli`.
  if (process.env.MEMBRANE_BIN) return { binary: process.env.MEMBRANE_BIN, args: ["cli", ...cortexArgs(args, installation)] };
  const binary = join(installation.workspaceRoot, "tools", "bin", process.platform === "win32" ? "membrane.exe" : "membrane");
  return { binary, args: ["cli", ...cortexArgs(args, installation)] };
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
    const command = durableCli(binding, installation, [
      "feedback", "--trace", eventId, "--candidate", args.receiptId,
      "--sha", digest(args.receiptId).slice("sha256:".length), "--outcome", args.outcome,
      "--source", source, "--scope", binding.scope_id,
      ...(source === "cited_verdict" ? ["--verdict-ref", args.verdictRef] : []),
    ]);
    const out = await run(command.binary, command.args, "", env);
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

const BLUEPRINT_OPERATIONS = new Set(["architecture", "symbol", "reference", "references", "impact", "changes", "snapshot_get", "snapshot_list", "changes_since"]);
const SAFE_BLUEPRINT_VALUE = /^[A-Za-z0-9_.$:/-]{1,256}$/;
const HASH = /^(?:sha256:[0-9a-f]{64}|xxh128:[0-9a-f]{32})$/;
const GIT_COMMIT = /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/;
function blueprintValue(value) { return typeof value === "string" && SAFE_BLUEPRINT_VALUE.test(value) ? value : null; }
function blueprintHash(value) { return typeof value === "string" && HASH.test(value) ? value : null; }
function validBlueprintAuth(args) {
  const caller = args.caller;
  const bounded = (value, max) => typeof value === "string" && value.length > 0 && Buffer.byteLength(value, "utf8") <= max;
  if (!bounded(args.repository, 4096) || !caller || typeof caller !== "object" || Array.isArray(caller)) return false;
  if (!bounded(caller.root, 4096) || !bounded(caller.repositoryId, 256) || !bounded(caller.scopeId, 256)) return false;
  const allowed = new Set(["root", "repositoryId", "scopeId", "scopeDescriptor"]);
  return Object.keys(caller).every((key) => allowed.has(key)) && (caller.scopeDescriptor === undefined || (caller.scopeDescriptor && typeof caller.scopeDescriptor === "object" && !Array.isArray(caller.scopeDescriptor)));
}
function blueprintInteger(value, fallback, min, max, field) {
  if (value === undefined) return fallback;
  if (!Number.isInteger(value) || value < min || value > max) throw new Error(`invalid blueprint ${field}`);
  return value;
}
export function blueprintCommand(args) {
  const operation = args.operation;
  if (!BLUEPRINT_OPERATIONS.has(operation)) throw new Error("invalid blueprint operation");
  if (!validBlueprintAuth(args)) throw new Error("invalid blueprint auth envelope");
  const snapshotOperation = operation === "snapshot_get" || operation === "snapshot_list" || operation === "changes_since";
  const allowed = operation === "architecture" || operation === "changes" ? new Set(["repository", "caller", "operation", "limit", "budget", "items", "deadlineMs"])
    : snapshotOperation ? new Set(["repository", "caller", "operation", "name", "limit", "deadlineMs"])
    : new Set(["repository", "caller", "operation", "node", "limit", "budget", "depth"]);
  for (const key of Object.keys(args)) if (!allowed.has(key)) throw new Error(`invalid blueprint ${key}`);
  const term = args.node ?? args.query;
  if (snapshotOperation && operation !== "snapshot_list" && !blueprintValue(args.name)) throw new Error("invalid blueprint snapshot name");
  if (!snapshotOperation && (operation === "architecture" || operation === "changes" ? (args.node !== undefined || args.query !== undefined || args.from !== undefined || args.to !== undefined)
    : !blueprintValue(term))) throw new Error("invalid blueprint node or query");
  const limit = blueprintInteger(args.limit, 20, 1, 100, "limit");
  const budget = blueprintInteger(args.budget, 2000, 1, 10000, "budget");
  const depth = args.depth === undefined ? null : blueprintInteger(args.depth, 1, 1, 5, "depth");
  const command = operation === "snapshot_get" ? ["snapshot", "get", args.name]
    : operation === "snapshot_list" ? ["snapshot", "list"]
    : operation === "changes_since" ? ["changes-since", args.name]
    : operation === "architecture" ? ["architecture"]
    : operation === "changes" ? ["manifest"]
    : operation === "symbol" ? ["resolve", "--node", term]
    : (operation === "references" || operation === "reference") ? ["neighbors", "--node", term, "--direction", "both"]
    : operation === "impact" ? ["impact", "--node", term]
    : ["search", "--query", term];
  command.push("--json");
  if (!snapshotOperation || operation === "changes_since") command.push("--limit", String(limit));
  if (!snapshotOperation) command.push("--budget", String(budget));
  if (depth !== null) command.push("--depth", String(depth));
  return { operation, command };
}

export async function blueprintBatchCapability(binding, args, signal) {
  const batch = args.items;
  return executeCodeBatch({
    items: batch,
    deadlineMs: args.deadlineMs ?? 5000,
    signal,
    authorize: async (item) => {
      const target = { ...item, repository: item.repository || args.repository, caller: item.caller || args.caller, operation: item.operation || args.operation };
      if (!item || typeof item !== "object" || !BLUEPRINT_OPERATIONS.has(target.operation) || typeof item.generationId !== "string" || !item.generationId || typeof item.sourceHash !== "string" || !item.sourceHash) return { ok: false, code: "freshness_required" };
      const commandArgs = { repository: target.repository, caller: target.caller, operation: target.operation };
      if (target.node !== undefined) commandArgs.node = target.node;
      if (target.limit !== undefined) commandArgs.limit = target.limit;
      if (target.budget !== undefined) commandArgs.budget = target.budget;
      if (target.depth !== undefined) commandArgs.depth = target.depth;
      blueprintCommand(commandArgs);
      const targetBinding = await authorize(target, "context");
      if (targetBinding.blueprint_generation_id && targetBinding.blueprint_generation_id !== item.generationId) return { ok: false, code: "stale_generation" };
      if (!blueprintHash(item.sourceHash)) return { ok: false, code: "invalid_source_hash" };
      return { ok: true, repositoryRoot: targetBinding.root, authority: callerLevel(targetBinding), generationId: item.generationId, sourceHash: item.sourceHash };
    },
    // Blueprint's IPC protocol has no batch method. Returning no provider rows is
    // a safe typed fallback: Membrane never substitutes a CLI child process.
    provider: async () => [],
  });
}
function blueprintFreshness(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const generationId = blueprintValue(value.generationId || value.id);
  const barrierResult = value.barrierResult === "caught_up" || value.barrierResult === "timeout" ? value.barrierResult : null;
  const receiptId = blueprintValue(value.receiptId);
  return generationId && barrierResult && receiptId ? { generationId, barrierResult, receiptId } : null;
}
function blueprintGitIdentity(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const head = value.head || value.commit;
  const dirty = typeof value.dirty === "boolean" ? value.dirty : (typeof value.clean === "boolean" ? !value.clean : null);
  return typeof head === "string" && GIT_COMMIT.test(head) && dirty === false ? { head, dirty } : null;
}
function blueprintGeneration(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const generationId = blueprintValue(value.generationId);
  const manifestDigest = blueprintHash(value.manifestDigest);
  const sourceObservation = blueprintGitIdentity(value.sourceObservation);
  return generationId && manifestDigest && sourceObservation ? { generationId, manifestDigest, sourceObservation } : null;
}
export function sanitizeBlueprintPayload(payload, operation) {
  if (!payload || typeof payload !== "object" || Array.isArray(payload)) throw new Error("blueprint_unavailable");
  const freshness = blueprintFreshness(payload.freshness || payload.freshnessReceipt || payload.manifest || payload.sourceGeneration);
  if (operation === "snapshot_get") {
    const identity = blueprintGeneration(payload);
    const name = blueprintValue(payload.name);
    const leaves = Array.isArray(payload.leaves) ? payload.leaves.map((leaf) => ({ path: blueprintValue(leaf?.path), digest: blueprintHash(leaf?.digest) })).filter((leaf) => leaf.path && leaf.digest) : null;
    if (!identity || !name || !leaves || leaves.length !== payload.leaves.length) throw new Error("blueprint_unavailable");
    return { operation, name, ...identity, leaves };
  }
  if (operation === "snapshot_list") {
    if (!Array.isArray(payload)) throw new Error("blueprint_unavailable");
    const snapshots = payload.map((row) => { const identity = blueprintGeneration(row); const name = blueprintValue(row?.name); return identity && name ? { name, ...identity } : null; });
    if (snapshots.some((row) => !row)) throw new Error("blueprint_unavailable");
    return { operation, snapshots };
  }
  if (operation === "changes_since") {
    const base = blueprintGeneration(payload.base); const head = blueprintGeneration(payload.head);
    const changes = Array.isArray(payload.changes) ? payload.changes.map((row) => ({ path: blueprintValue(row?.path), kind: row?.kind })).filter((row) => row.path && ["added", "modified", "deleted"].includes(row.kind)) : null;
    const receipt = payload.receipt;
    const ordered = changes.every((row, index) => index === 0 || changes[index - 1].path.localeCompare(row.path) <= 0);
    if (!base || !head || !changes || changes.length !== payload.changes.length || !ordered || !receipt || !Number.isInteger(receipt.total) || receipt.total < changes.length || !Number.isInteger(receipt.limit) || receipt.limit < 1 || receipt.limit > 10000 || typeof receipt.truncated !== "boolean" || receipt.truncated !== (receipt.total > receipt.limit)) throw new Error("blueprint_unavailable");
    return { operation, base, head, changes, receipt: { total: receipt.total, limit: receipt.limit, truncated: receipt.truncated } };
  }
  if (operation === "changes") {
    const generationId = blueprintValue(payload.generationId);
    const manifestDigest = blueprintHash(payload.manifestDigest);
    const observation = payload.sourceObservation;
    const commitValue = observation?.commit || observation?.head;
    const commit = typeof commitValue === "string" && GIT_COMMIT.test(commitValue) ? commitValue : null;
    const clean = typeof observation?.clean === "boolean" ? observation.clean : null;
    if (!generationId || !manifestDigest || !commit || clean === null || !freshness || freshness.generationId !== generationId) throw new Error("blueprint_unavailable");
    return { operation, sourceGeneration: generationId, freshness, changes: { generationId, manifestDigest, sourceObservation: { commit, clean } }, resolver: "blueprint graph manifest --json" };
  }
  const rows = payload.results || payload.nodes || payload.impacted || payload.examples || (payload.id ? [payload] : []);
  const items = (Array.isArray(rows) ? rows : [rows]).filter((row) => row && typeof row === "object").map((row) => {
    const id = blueprintValue(row.id || row.nodeId || row.path);
    const sourceHash = blueprintHash(row.sourceHash || row.contentHash || row.contentDigest);
    const resolver = id ? `blueprint graph resolve --node ${id}` : null;
    return { id, name: blueprintValue(row.name || row.qualifiedName), kind: blueprintValue(row.kind), path: blueprintValue(row.path || row.sourceRef), sourceHash, startLine: Number.isInteger(row.startLine) && row.startLine > 0 ? row.startLine : null, endLine: Number.isInteger(row.endLine) && row.endLine > 0 ? row.endLine : null, freshness, resolver: resolver && resolver.length <= 320 ? resolver : null };
  }).filter((item) => item.id && item.sourceHash && item.freshness && item.resolver);
  return { operation, sourceGeneration: freshness?.generationId || null, freshness, items };
}
function blueprintIpcRequest(operation, args, binding, signal, request = requestBlueprint) {
  const deadlineMs = args.deadlineMs ?? 2000;
  const input = { repoRoot: binding.root };
  if (operation === "architecture") Object.assign(input, { budget: args.budget ?? 2000 });
  else if (operation === "symbol") Object.assign(input, { nodeId: args.node });
  else if (operation === "reference" || operation === "references") Object.assign(input, { anchor: args.node, direction: "both", depth: args.depth ?? 1, budget: args.budget ?? 2000 });
  else if (operation === "impact") Object.assign(input, { anchor: args.node, depth: args.depth ?? 3, budget: args.budget ?? 2000 });
  else throw new Error("blueprint_unavailable");
  const method = operation === "symbol" ? "resolve"
    : operation === "reference" || operation === "references" ? "expand"
      : operation;
  return request(method, input, { deadlineMs, signal });
}

export async function blueprintCapability(binding, args, signal, { request = requestBlueprint } = {}) {
  const { operation } = blueprintCommand(args);
  let payload;
  try { payload = await blueprintIpcRequest(operation, args, binding, signal, request); }
  catch { throw new Error("blueprint_unavailable"); }
  try { return sanitizeBlueprintPayload(payload, operation); }
  catch { throw new Error("blueprint_unavailable"); }
}

const SNAPSHOT_CACHE_LIMIT = 64;
function diagnosticIdentity(args) {
  if (!args || typeof args !== "object") throw new Error("invalid_diagnostic_identity");
  for (const value of [args.repoId, args.worktreeId]) {
    if (typeof value !== "string" || !DIAGNOSTIC_ID.test(value)) throw new Error("invalid_diagnostic_identity");
  }
  // Optional exact project-root binding (design §3 WorkspaceEngineKey):
  // opening with a canonical root pins the workspace's engine lanes to that
  // worktree instead of the resident process current directory.
  const identity = { repoId: args.repoId, worktreeId: args.worktreeId };
  if (typeof args.projectRoot === "string" && args.projectRoot.trim()) {
    if (args.projectRoot.length > 1024) throw new Error("invalid_diagnostic_identity");
    identity.projectRoot = args.projectRoot.trim();
  } else if (args.projectRoot !== undefined) {
    throw new Error("invalid_diagnostic_identity");
  }
  return identity;
}
function validateWorkspaceEpoch(epoch, { repoId, worktreeId, origin } = {}) {
  bounded(epoch, MAX_REQUEST_BYTES, "workspace epoch");
  if (epoch.schemaVersion !== "workspace-epoch.v1" || typeof epoch.repoId !== "string" || !DIAGNOSTIC_ID.test(epoch.repoId) || typeof epoch.worktreeId !== "string" || !DIAGNOSTIC_ID.test(epoch.worktreeId)) throw new Error("invalid_workspace_epoch");
  if ((repoId !== undefined && epoch.repoId !== repoId) || (worktreeId !== undefined && epoch.worktreeId !== worktreeId)) throw new Error("invalid_workspace_epoch");
  if (!Number.isInteger(epoch.epoch) || epoch.epoch < 0) throw new Error("invalid_workspace_epoch");
  if (typeof epoch.sourceManifestDigest !== "string" || !epoch.sourceManifestDigest || epoch.sourceManifestDigest.length > 256) throw new Error("invalid_workspace_epoch");
  if (!EPOCH_ORIGINS.includes(epoch.origin)) throw new Error("invalid_workspace_epoch");
  if (origin && epoch.origin !== origin) throw new Error("invalid_workspace_epoch");
  return epoch;
}
function validateReconcileHashes(hashes) {
  if (!Array.isArray(hashes) || hashes.length > 4096) throw new Error("invalid_reconcile_hashes");
  for (const entry of hashes) {
    if (!entry || typeof entry !== "object" || Array.isArray(entry) || typeof entry.path !== "string" || !entry.path || typeof entry.hash !== "string" || !entry.hash) throw new Error("invalid_reconcile_hashes");
  }
  return hashes;
}
function validateGatePolicy(policy) {
  bounded(policy, MAX_REQUEST_BYTES, "gate policy");
  if (typeof policy.profileName !== "string" || !policy.profileName.trim() || policy.profileName.length > 128) throw new Error("invalid_gate_policy");
  if (typeof policy.policyVersion !== "string" || typeof policy.policyDigest !== "string") throw new Error("invalid_gate_policy");
  if (!Array.isArray(policy.blockingCodes) || policy.blockingCodes.some((code) => typeof code !== "string")) throw new Error("invalid_gate_policy");
  if (!Array.isArray(policy.requiredCapabilities) || policy.requiredCapabilities.some((capability) => !CAPABILITY_VOCABULARY.includes(capability))) throw new Error("invalid_gate_policy");
  return policy;
}
function rememberSnapshot(snapshots, key, record) {
  snapshots.set(key, record);
  if (snapshots.size > SNAPSHOT_CACHE_LIMIT) snapshots.delete(snapshots.keys().next().value);
}
function normalizeAwaitRecord(body) {
  if (!body || typeof body !== "object" || Array.isArray(body)) return { decision: null, snapshot: null };
  if (body.decision && typeof body.decision === "object" && !Array.isArray(body.decision)) return { decision: body.decision, snapshot: body.snapshot && typeof body.snapshot === "object" && !Array.isArray(body.snapshot) ? body.snapshot : null };
  if (typeof body.outcome === "string") return { decision: body, snapshot: null };
  return { decision: null, snapshot: null };
}
const FENCE_GUIDANCE = {
  clean_exact: "continue or escalate proportionally; nothing additional is blocked",
  dirty_exact: "repair first: unrelated implementation, ordinary tests/builds, and completion stay blocked until a re-await proves the bytes clean; inspect/search/explain/rerun diagnostics remain allowed",
  superseded: "await the newest workspace epoch; decisions based on old evidence stay invalid",
};
function unknownGuidance() { return "repair the provider/service/config or run an approved V1 verifier; clean claims, completion, and escalation assuming semantic cleanliness stay blocked"; }

export async function diagnosticsCapability(name, args = {}, { request = diagnosticsRequest, snapshots = LAST_SNAPSHOTS } = {}) {
  if (!args || typeof args !== "object" || Array.isArray(args)) throw new Error("invalid_diagnostic_operation");
  const operation = args.operation;
  const outcome = async (promise) => {
    const response = await promise;
    return response.ok
      ? { delivered: true, status: response.status ?? null, result: response.body }
      : { delivered: false, status: response.status ?? null, error: response.error };
  };
  if (name === "membrane_diagnostic_workspace") {
    const identity = diagnosticIdentity(args);
    if (operation === "open") {
      if (!identity.projectRoot) throw new Error("invalid_diagnostic_identity");
      return outcome(request("/diagnostics/workspace/open", { method: "POST", body: identity }));
    }
    if (operation === "close") return outcome(request("/diagnostics/workspace/close", { method: "POST", body: identity }));
    if (operation === "status") return outcome(request(`/diagnostics/workspace/status?repoId=${encodeURIComponent(identity.repoId)}&worktreeId=${encodeURIComponent(identity.worktreeId)}`, { method: "GET" }));
    if (operation === "reconcile") {
      if (typeof args.manifestDigest !== "string" || !args.manifestDigest || args.manifestDigest.length > 256) throw new Error("invalid_reconcile_manifest_digest");
      return outcome(request("/diagnostics/reconcile", { method: "POST", body: { ...identity, manifestDigest: args.manifestDigest, hashes: validateReconcileHashes(args.hashes) } }));
    }
    throw new Error("invalid_diagnostic_operation");
  }
  if (name === "membrane_diagnostic_mutation") {
    const identity = diagnosticIdentity(args);
    if (operation === "begin") return outcome(request("/diagnostics/mutation/begin", { method: "POST", body: identity }));
    if (operation === "seal") return outcome(request("/diagnostics/mutation/seal", { method: "POST", body: { ...identity, epoch: validateWorkspaceEpoch(args.epoch, { repoId: identity.repoId, worktreeId: identity.worktreeId }) } }));
    if (operation === "registerObserved") return outcome(request("/diagnostics/mutation/registerObserved", { method: "POST", body: { ...identity, epoch: validateWorkspaceEpoch(args.epoch, { repoId: identity.repoId, worktreeId: identity.worktreeId, origin: "observed_hook" }) } }));
    throw new Error("invalid_diagnostic_operation");
  }
  if (name === "membrane_diagnostic_snapshot") {
    const identity = diagnosticIdentity(args);
    const key = `${identity.repoId}:${identity.worktreeId}`;
    if (operation === "await") {
      if (typeof args.policyProfileName !== "string" || !args.policyProfileName.trim() || args.policyProfileName.length > 128) throw new Error("invalid_policy_profile_name");
      const requiredCapabilities = args.requiredCapabilities ?? [];
      if (!Array.isArray(requiredCapabilities) || requiredCapabilities.some((capability) => !CAPABILITY_VOCABULARY.includes(capability))) throw new Error("invalid_required_capabilities");
      const maxCost = args.maxCost ?? "interactive";
      if (!COST_CLASSES.includes(maxCost)) throw new Error("invalid_max_cost");
      const deadlineMs = args.deadlineMs ?? 10000;
      if (!Number.isInteger(deadlineMs) || deadlineMs < 1 || deadlineMs > 60000) throw new Error("invalid_deadline_ms");
      const response = await outcome(request("/diagnostics/snapshot/await", { method: "POST", body: { ...identity, policyProfileName: args.policyProfileName, requiredCapabilities, maxCost, deadlineMs } }));
      if (response.delivered) rememberSnapshot(snapshots, key, normalizeAwaitRecord(response.result));
      return response;
    }
    if (!snapshots.has(key)) throw new Error("snapshot_not_awaited");
    const record = snapshots.get(key);
    if (operation === "get") return { delivered: true, status: null, result: record };
    if (operation === "explain") {
      const decision = record?.decision;
      if (!decision) throw new Error("snapshot_not_awaited");
      return {
        delivered: true,
        status: null,
        result: {
          snapshotId: decision.snapshotId ?? null,
          outcome: decision.outcome ?? null,
          policyProfile: decision.policyProfile ?? args.policyProfileName ?? null,
          blockingIssueIds: decision.blockingIssueIds ?? [],
          reasonCodes: decision.reasonCodes ?? [],
          omissions: decision.omissions ?? [],
          guidance: FENCE_GUIDANCE[decision.outcome] || unknownGuidance(),
          note: "events and presentation cannot clear the fence; only snapshot-await or fence-evaluation produces an operational decision and only exact outcomes prove state",
        },
      };
    }
    if (operation === "delta") {
      const snapshot = record?.snapshot;
      if (!snapshot) {
        return { delivered: true, status: null, result: { blueprintDelta: null, aggregateDelta: null, omissions: [{ code: "snapshot_body_not_cached", detail: "the awaited decision carried no evidence snapshot; delta views need it" }] } };
      }
      return { delivered: true, status: null, result: { blueprintDelta: snapshot.blueprintDelta ?? null, aggregateDelta: snapshot.aggregateDelta ?? null, omissions: [] } };
    }
    throw new Error("invalid_diagnostic_operation");
  }
  if (name === "membrane_diagnostic_fence") {
    bounded(args.snapshot, MAX_REQUEST_BYTES, "evidence snapshot");
    validateWorkspaceEpoch(args.expectedEpoch);
    return outcome(request("/diagnostics/fence/evaluate", { method: "POST", body: { snapshot: args.snapshot, expectedEpoch: args.expectedEpoch, policy: validateGatePolicy(args.policy) } }));
  }
  if (name === "membrane_diagnostic_capabilities") return outcome(request("/diagnostics/capabilities", { method: "GET" }));
  if (name === "membrane_diagnostic_baseline") {
    const identity = diagnosticIdentity(args);
    if (operation !== "capture" && operation !== "update") throw new Error("invalid_diagnostic_operation");
    if (typeof args.name !== "string" || !args.name.trim() || args.name.length > 128) throw new Error("invalid_baseline_name");
    return outcome(request(`/diagnostics/baseline/${operation}`, { method: "POST", body: { ...identity, name: args.name } }));
  }
  if (name === "membrane_diagnostic_provider") {
    if (operation === "list") return outcome(request("/diagnostics/capabilities", { method: "GET" }));
    if (operation === "status") return outcome(request("/diagnostics/status", { method: "GET" }));
    if (operation === "restart") {
      if (typeof args.keyDigest !== "string" || !args.keyDigest || args.keyDigest.length > 256) throw new Error("invalid_provider_key_digest");
      return outcome(request("/diagnostics/provider/restart", { method: "POST", body: { keyDigest: args.keyDigest } }));
    }
    throw new Error("invalid_diagnostic_operation");
  }
  throw new Error("invalid_diagnostic_operation");
}
async function callTool(name, args, trace = {}, lifecycle) {
  if (name === "membrane_context") {
    await lifecycle?.checkpoint("authorization", 10);
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
        return text({ ok: false, scope: "workspace", error: routing.reason, considered: routing.considered, routing: routing.receipt });
      }
      const selected = routing.targets;
      const budget = Number.isInteger(args.budget) ? args.budget : 4096;
      const perRepoBudget = Math.max(1, Math.floor(budget / selected.length));
      const fused = { ok: true, scope: "workspace", repos: [], totalCandidates: 0, totalOmissions: 0, routing: routing.receipt };
      const trace = { traceparent: args.traceparent, tracestate: args.tracestate, baggage: args.baggage };
      const workspaceGrants = binding.grant_policy?.child_repository_ids;
      const catalogRootId = repos.find((entry) => entry.repoId === catalog.workspace_id)?.repoId;
      const workspaceEnvelope = requestEnvelopeFor(args);
      // MBR-003 authorizes EVERY target independently (same primitive as direct
      // calls). MBR-005 (SN-NODE-04 / R04) bounds the whole fan-out by ONE
      // absolute monotonic ingress deadline with bounded concurrency; receipts
      // stay in stable repository order. Denied/aborted/expired targets become
      // typed terminal omissions, never collapsed into federation_error.
      const deadlineMs = createDeadline(Number.isInteger(args.deadlineMs) ? args.deadlineMs : WORKSPACE_TIMEOUT_MS);
      const lane = deadlineSignal(deadlineMs, lifecycle?.signal);
      const abortRow = (entry, reason) => {
        const r = reason || terminalReason(lane.signal) || "cancelled";
        const row = { repoId: entry.repoId, basis: "aborted", generationId: null, manifestDigest: null, sourceCommit: "", identityStatus: "unknown", identityReason: "generation_identity_unavailable", candidates: 0, omissions: [r] };
        if (r === "deadline_exceeded") row.timeoutReceipt = timeoutReceipt("server.fan-out", deadlineMs);
        return { row, omissions: 1, candidates: 0 };
      };
      const deniedRow = (entry) => ({ row: { repoId: entry.repoId, basis: "denied", generationId: null, manifestDigest: null, sourceCommit: "", identityStatus: "unknown", identityReason: "generation_identity_unavailable", candidates: 0, omissions: ["target_denied"] }, omissions: 1, candidates: 0 });
      await lifecycle?.checkpoint("provider_dispatch", 30);
      const results = await runBoundedOrdered(selected, WORKSPACE_CONCURRENCY, async (entry) => {
        if (lane.signal.aborted) return abortRow(entry);
        const targetRoot = entry.rootBinding;
        let targetBinding;
        try { targetBinding = await bindingFor(targetRoot); } catch { return deniedRow(entry); }
        const hasGrant = hasExplicitChildGrant(catalog, catalogRootId, entry.repoId, workspaceGrants);
        const effectiveLevel = await canReachTarget({
          callerBinding: binding,
          targetBinding,
          action: "context",
          taskGrantLevel: args.taskGrantLevel,
          hasExplicitChildGrant: hasGrant,
        });
        if (!effectiveLevel) return deniedRow(entry);
      const request = { task: args.task, repo: targetRoot, maxTokens: perRepoBudget, intent: args.intent, session: args.session, anchors: args.anchors, scopeGrantId: args.scopeGrantId, scopeDescriptor: binding.scope_descriptor, taskEnvelope: workspaceEnvelope.taskEnvelope, turnEnvelope: workspaceEnvelope.turnEnvelope, clientEnvelope: workspaceEnvelope.clientEnvelope, overlay: workspaceEnvelope.overlay, ...trace };
        const out = await run(process.execPath, [CLIENT, "--input", "-"], JSON.stringify(request), { ...await bindingEnv(binding), WORKSPACE_ROOT: targetRoot, MEMBRANE_DEADLINE_AT_MS: String(deadlineMs) }, lane.signal);
        const reason = terminalReason(lane.signal);
        if (reason) return { row: { repoId: entry.repoId, basis: "aborted", generationId: null, candidates: 0, omissions: [reason] }, omissions: 1, candidates: 0 };
        const packet = text(out.stdout.trim() || "");
        const degraded = Boolean(packet?.degradationReason && packet.degradationReason !== "none");
        const identity = repositoryIdentity(packet, entry);
        return {
          row: {
            repoId: entry.repoId,
            ...identity,
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
      await lifecycle?.checkpoint("provider_results", 80);
      fused.repos.sort((a, b) => String(a.repoId).localeCompare(String(b.repoId))); // stable receipt order by repository id
      if (lane.signal.aborted) fused.deadlineExceeded = true;
      lane.close();
      // MBR-007 / R06: preserve the exact versioned envelopes on the delivery.
      fused.taskEnvelope = workspaceEnvelope.taskEnvelope;
      fused.turnEnvelope = workspaceEnvelope.turnEnvelope;
      fused.clientEnvelope = workspaceEnvelope.clientEnvelope;
      fused.overlay = workspaceEnvelope.overlay;
      return text(fused);
    }

    // Single-repo path (default).
    await lifecycle?.checkpoint("provider_dispatch", 40);
    const singleEnvelope = requestEnvelopeFor(args);
    const request = { task: args.task, repo: binding.root, maxTokens: args.budget, intent: args.intent, session: args.session, anchors: args.anchors, scopeGrantId: args.scopeGrantId, scopeDescriptor: binding.scope_descriptor, taskEnvelope: singleEnvelope.taskEnvelope, turnEnvelope: singleEnvelope.turnEnvelope, clientEnvelope: singleEnvelope.clientEnvelope, overlay: singleEnvelope.overlay, ...trace };
    const out = await run(process.execPath, [CLIENT, "--input", "-"], JSON.stringify(request), { ...await bindingEnv(binding), WORKSPACE_ROOT: binding.root }, lifecycle?.signal);
    const packet = text(out.stdout.trim() || { status: "unavailable", error: out.stderr.slice(0, 240) });
    if (!packet || typeof packet !== "object" || Array.isArray(packet)) return packet;
    if (args.session && args.taskId && packet.ok === true && packet.packet && typeof packet.packet === "object") {
      const freshness = await currentFreshness(binding, install, args.session, singleEnvelope.overlay.worktreePath);
      const scopeGrant = mintScopeGrantV1({ binding, args, packet: packet.packet, freshness });
      if (scopeGrant) packet.scopeGrant = scopeGrant;
    }
    if (!args.session || !args.taskId) {
      // MBR-007 / R06: exact envelopes ride on the delivery even without a session.
      if (args.taskEnvelope || args.turnEnvelope) {
        return { ...packet, taskEnvelope: singleEnvelope.taskEnvelope, turnEnvelope: singleEnvelope.turnEnvelope, clientEnvelope: singleEnvelope.clientEnvelope, overlay: singleEnvelope.overlay };
      }
      return packet;
    }
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
    await lifecycle?.checkpoint("packet_ready", 90);
    return { ...packet, working_contexts: workingContexts };
  }
  if (name === "membrane_source_read") {
    const binding = await authorize(args, "source_read");
    const install = await installationBindingFor(binding);
    const command = durableCli(binding, install, ["doc", "read", "--source-ref", args.sourceRef, "--anchor", args.anchorId, "--expected-hash", args.expectedContentHash]);
    const out = await run(command.binary, command.args, "", await bindingEnv(binding));
    return text(out.stdout.trim() || { error: "source_read_unavailable", detail: out.stderr.slice(0, 240) });
  }
  if (name === "membrane_blueprint") {
    const binding = await authorize(args, "context");
    if (Array.isArray(args.items)) return blueprintBatchCapability(binding, args, lifecycle?.signal);
    return blueprintCapability(binding, args, lifecycle?.signal);
  }
  if (name === "membrane_checkpoint_save") {
    const binding = await authorize(args, "checkpoint");
    const install = await installationBindingFor(binding);
    bounded(args.checkpoint, MAX_PROPOSAL_BYTES, "checkpoint");
    takeRate(binding, "checkpoint");
    const command = durableCli(binding, install, ["checkpoint", "save"]);
    const out = await run(command.binary, command.args, JSON.stringify(args.checkpoint), await bindingEnv(binding));
    return text(out.stdout.trim() || { error: "checkpoint_save_unavailable", detail: out.stderr.slice(0, 240) });
  }
  if (name === "membrane_checkpoint_load") {
    const binding = await authorize(args, "checkpoint_load");
    const install = await installationBindingFor(binding);
    const params = ["checkpoint", "load", args.id];
    if (Number.isInteger(args.asOfMs)) params.push("--as-of-ms", String(args.asOfMs));
    const command = durableCli(binding, install, params);
    const out = await run(command.binary, command.args, "", await bindingEnv(binding));
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
        if (args.cursor !== undefined || args.limit !== undefined) {
          const page = store.activeContextPage({ sessionId: args.sessionId, taskId: args.taskId, ...(args.asOf ? { asOf: args.asOf } : {}), ...(args.cursor !== undefined ? { cursor: args.cursor } : {}), ...(args.limit !== undefined ? { limit: args.limit } : {}) });
          return { status: "loaded", contexts: page.items, nextCursor: page.nextCursor };
        }
        return { status: "loaded", contexts: store.activeContexts({ sessionId: args.sessionId, taskId: args.taskId, ...(args.asOf ? { asOf: args.asOf } : {}) }), nextCursor: null };
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
  if (name.startsWith("membrane_diagnostic_")) return diagnosticsCapability(name, args);
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
function typedErrorResult(error) {
  const message = error instanceof Error ? error.message : String(error);
  const code = /^[a-z][a-z0-9_]{2,80}$/i.test(message) ? message : "tool_execution_failed";
  const retryable = /(?:timeout|rate_limited|unavailable)/i.test(code);
  return {
    content: [{ type: "text", text: JSON.stringify({ error: { code, message, retryable, remediation: retryable ? "retry after the temporary condition clears" : "check the caller binding and required arguments" } }) }],
    isError: true,
  };
}
export function buildServer({ shutdownSignal } = {}) {
  const server = new McpServer(
    { name: "membrane", version: "1.0.0" },
    { capabilities: { logging: {} }, instructions: "Use membrane_context for federated context through /federate. Never expect raw memory CRUD." },
  );
  for (const tool of TOOLS) {
    server.registerTool(tool.name, {
      description: tool.description,
      inputSchema: fromJsonSchema(tool.inputSchema),
      outputSchema: fromJsonSchema(TOOL_OUTPUT_SCHEMA),
      annotations: tool.annotations,
    }, async (args, ctx) => {
      const trace = boundedTrace(ctx.mcpReq._meta);
      const requestSignal = shutdownSignal ? AbortSignal.any([ctx.mcpReq.signal, shutdownSignal]) : ctx.mcpReq.signal;
      const lifecycle = createLifecycle({
        operation: tool.name,
        requestId: boundedLifecycleId(ctx.mcpReq.id),
        signal: requestSignal,
        progressToken: ctx.mcpReq._meta?.progressToken,
        log: async (event) => ctx.mcpReq.log("info", event, "membrane"),
        progress: async (event) => {
          if (event.progressToken !== undefined) await ctx.mcpReq.notify({ method: "notifications/progress", params: event });
        },
      });
      await lifecycle.begin();
      try {
        const result = await withCancellationGrace(() => callTool(tool.name, args, trace, lifecycle), { signal: requestSignal });
        await lifecycle.complete();
        return structuredResult(result, trace);
      } catch (error) {
        if (requestSignal.aborted) await lifecycle.cancelled();
        return typedErrorResult(error);
      }
    });
  }
  server.server.setRequestHandler("tools/list", (request) => ({
    tools: TOOLS.filter((tool) => toolsetNames(request.params).includes(tool.name)).map((tool) => ({
      name: tool.name, description: tool.description, inputSchema: tool.inputSchema, outputSchema: TOOL_OUTPUT_SCHEMA, annotations: tool.annotations,
    })),
  }));
  server.registerResource("Membrane protocol v1", PROTOCOL_URI, { mimeType: "text/markdown" }, async (uri) => {
    return { contents: [{ uri: uri.href, mimeType: "text/markdown", text: protocol }] };
  });
  return server;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const shutdown = new AbortController();
  const stdio = serveStdio(() => buildServer({ shutdownSignal: shutdown.signal }), { onerror: (error) => process.stderr.write(`membrane-mcp: ${error.message}\n`) });
  // The SDK transport listens for data but does not close itself when a parent
  // closes stdin. Close its pinned instance explicitly so MCP subprocesses do
  // not survive a client disconnect or test harness shutdown.
  process.stdin.once("end", () => {
    shutdown.abort(new Error("stdio_closed"));
    void stdio.close();
  });
}

export { TOOLS, TOOL_OUTPUT_SCHEMA };
