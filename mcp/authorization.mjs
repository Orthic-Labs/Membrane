// mcp/authorization.mjs — MBR-002 / SN-NODE-02 monotone effective authority.
//
// Effective privilege is the minimum (intersection) of installation, caller,
// target, child-grant, task/session-grant, and the operation capability. No
// downstream call may recover authority from the target binding alone.

const LEVEL_RANK = Object.freeze({
  "read-only": 0,
  "write-proposed": 1,
  "write-trusted": 2,
  admin: 3,
});
const RANK_LEVEL = Object.freeze(["read-only", "write-proposed", "write-trusted", "admin"]);

const READ_ACTIONS = new Set([
  "context", "source_read", "checkpoint_load", "working_context_load",
  "temporal_fact_query", "scratchpad_load", "system_status",
]);

export function levelRank(level) {
  if (!(level in LEVEL_RANK)) throw new Error(`unknown_authority_level:${level}`);
  return LEVEL_RANK[level];
}

export function intersectAuthority(...levels) {
  const normalized = levels.filter(Boolean).map(levelRank);
  if (normalized.length === 0) return "read-only";
  return RANK_LEVEL[Math.min(...normalized)];
}

export function permitsLevel(level, action) {
  if (READ_ACTIONS.has(action)) return levelRank(level) >= LEVEL_RANK["read-only"];
  return levelRank(level) >= LEVEL_RANK["write-proposed"];
}

function deny(gate, detail = gate) {
  const error = new Error(`authorization_denied:${gate}${detail === gate ? "" : `: ${detail}`}`);
  error.code = "authorization_denied";
  error.gate = gate;
  throw error;
}

function intervalValue(binding, key, camel) {
  const grant = binding?.token_grant || binding?.grantPolicy?.token_grant;
  const policy = binding?.grant_policy;
  const grantValidity = grant?.validity || grant?.validity_interval;
  const validity = binding?.validity || binding?.validity_interval || policy?.validity || policy?.validity_interval;
  return grant?.[key] ?? grant?.[camel] ?? grantValidity?.[key] ?? grantValidity?.[camel] ?? validity?.[key] ?? validity?.[camel] ?? policy?.[key] ?? policy?.[camel] ?? binding?.[key] ?? binding?.[camel];
}
function validityMillis(value) {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value !== "string" || !value.trim()) return null;
  const text = value.trim();
  if (/^-?\d+$/.test(text)) return Number(text);
  const parsed = Date.parse(text);
  return Number.isFinite(parsed) ? parsed : null;
}
function checkValidity(binding, label) {
  const beforeRaw = intervalValue(binding, "not_before", "notBefore");
  const afterRaw = intervalValue(binding, "not_after", "notAfter");
  const before = beforeRaw === undefined || beforeRaw === null ? null : validityMillis(beforeRaw);
  const after = afterRaw === undefined || afterRaw === null ? null : validityMillis(afterRaw);
  if ((beforeRaw !== undefined && before === null) || (afterRaw !== undefined && after === null)) deny("authorization_revoked", `${label} validity interval is invalid`);
  const now = Date.now();
  if (before !== null && now < before) deny("authorization_revoked", `${label} grant is not yet valid`);
  if (after !== null && now >= after) deny("authorization_revoked", `${label} grant validity interval has expired`);
}

export async function authorizeTarget({
  callerBinding,
  targetBinding,
  childGrantLevel,
  taskGrantLevel,
  installationLevel = "write-proposed",
  action,
  hasExplicitChildGrant,
  callerIdentity,
}) {
  // Gate 1 — installation grant. A missing binding is never an implicit
  // read-only installation and the installation ceiling is validated here.
  if (!callerBinding || !targetBinding) deny("installation_grant_denied", "installation binding is unavailable");
  try { levelRank(installationLevel); } catch (error) { deny("installation_grant_denied", error.message); }

  // Gate 2 — repository scope chain. Both identities must be enrolled before
  // any caller, authority, or cross-root decision is made.
  if (typeof callerBinding.repository_id !== "string" || typeof targetBinding.repository_id !== "string") {
    deny("repository_scope_chain_denied", "repository binding is malformed");
  }

  // Gate 3 — caller/target binding. The optional identity is used by the
  // server path; direct callers without an envelope retain the binding-only
  // contract used by the existing workspace primitive.
  if (callerIdentity && (callerIdentity.repositoryId !== callerBinding.repository_id || callerIdentity.scopeId !== callerBinding.scope_id)) {
    deny("caller_scope_binding_denied", "caller identity does not match the installation binding");
  }

  // Gate 4 — authority level (monotone minimum). Parse all levels before the
  // cross-root decision so the public order is identical to native Rust.
  let effectiveLevel;
  try {
    const sameTarget = callerBinding.repository_id === targetBinding.repository_id;
    effectiveLevel = intersectAuthority(
      installationLevel,
      callerBinding.grant_policy?.level || "read-only",
      targetBinding.grant_policy?.level || "read-only",
      sameTarget ? "admin" : childGrantLevel || "read-only",
      taskGrantLevel || callerBinding.grant_policy?.level || "read-only",
    );
    if (!permitsLevel(effectiveLevel, action)) deny("caller_not_authorized", `effective authority ${effectiveLevel} may not perform ${action}`);
  } catch (error) {
    if (error?.code === "authorization_denied") throw error;
    deny("caller_not_authorized", error.message);
  }

  // Gate 5 — cross-root denial runs after authority, exactly as native Rust.
  const sameTarget = callerBinding.repository_id === targetBinding.repository_id;
  if (!sameTarget && !hasExplicitChildGrant) deny("cross_root_binding_denied");

  // Gate 6 — validity interval and token-generation revocation share one
  // stable gate identity. Missing interval bounds are unbounded validity.
  checkValidity(targetBinding, "target");
  checkValidity(callerBinding, "caller");
  for (const [binding, label] of [[targetBinding, "target"], [callerBinding, "caller"]]) {
    const generation = binding.token_grant?.generation;
    const revoked = binding.token_grant?.revoked_generations;
    if (generation !== undefined && Array.isArray(revoked) && revoked.includes(generation)) deny("authorization_revoked", `${label} binding token generation ${generation} is revoked`);
  }
  return { callerBinding, targetBinding, effectiveLevel };
}

// MBR-003: the workspace aggregate must exercise exactly the same per-target
// authorization primitive as a direct repository call. Returns the effective
// level when the caller may reach the target for `action`; null on any denial
// (cross-root without grant, or insufficient monotone authority).
export async function canReachTarget({ callerBinding, targetBinding, action, taskGrantLevel, hasExplicitChildGrant }) {
  try {
    const authorized = await authorizeTarget({
      callerBinding,
      targetBinding,
      childGrantLevel: targetBinding?.grant_policy?.level || "read-only",
      taskGrantLevel,
      action,
      hasExplicitChildGrant,
    });
    return authorized.effectiveLevel;
  } catch {
    return null;
  }
}
