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

export async function authorizeTarget({
  callerBinding,
  targetBinding,
  childGrantLevel,
  taskGrantLevel,
  installationLevel = "write-proposed",
  action,
  hasExplicitChildGrant,
}) {
  const sameTarget = callerBinding?.repository_id === targetBinding?.repository_id;
  if (!sameTarget && !hasExplicitChildGrant) throw new Error("cross_root_binding_denied");
  const effectiveLevel = intersectAuthority(
    installationLevel,
    callerBinding?.grant_policy?.level || "read-only",
    targetBinding?.grant_policy?.level || "read-only",
    sameTarget ? "admin" : childGrantLevel || "read-only",
    taskGrantLevel || "read-only",
  );
  if (!permitsLevel(effectiveLevel, action)) throw new Error("caller_not_authorized");
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
