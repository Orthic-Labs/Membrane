// D31: hook policy modes — advisory, recall-before-read, task-grants. Each
// mode has explicit fail-open/fail-closed behavior and a recovery command.

export const HOOK_POLICY_MODES = Object.freeze(["advisory", "recall-before-read", "task-grants"]);

export const HOOK_POLICIES = Object.freeze({
  advisory: { failClosed: false, description: "Hooks only advise; reads are never blocked." },
  "recall-before-read": { failClosed: true, description: "Reads are denied until a recall receipt exists for the session." },
  "task-grants": { failClosed: true, description: "Widened paths require a task-scoped grant." },
});

export function policyBehavior(mode) {
  const policy = HOOK_POLICIES[mode] ?? HOOK_POLICIES.advisory;
  return {
    mode,
    failClosed: policy.failClosed,
    recoveryCommand: mode === "recall-before-read"
      ? "blueprint recall --session <id>"
      : mode === "task-grants"
        ? "blueprint grant issue --task <id> --paths <glob>"
        : null,
  };
}
