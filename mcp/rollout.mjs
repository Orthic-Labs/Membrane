const STAGES = ["shadow", "advisory", "context_enforced", "tool_enforced", "learning"];

export function advanceRollout(current, next, evidence = {}) {
  const from = STAGES.indexOf(current);
  const to = STAGES.indexOf(next);
  if (from < 0 || to !== from + 1) throw new Error("rollout stage transition is not ordered");
  const required = { shadow: ["current_commit", "install_binding", "rollback_command"], advisory: ["genuine_tasks"], context_enforced: ["macOS", "Windows"], tool_enforced: ["zero_p0_regressions"], learning: ["full_feedback_readback"] }[next] || [];
  const missing = required.filter((key) => evidence[key] !== true);
  if (missing.length) throw new Error(`rollout promotion missing: ${missing.join(",")}`);
  return { schema: "orthic.rollout-receipt.v1", from: current, to: next, evidence: required, status: "promoted" };
}

export { STAGES };
