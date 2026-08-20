// D11: non-destructive repair plans. `cortex doctor --repair-plan --json`
// returns ordered repair actions; `--apply-repair` requires explicit
// confirmation unless `--yes`. Every action is reversible or explicitly
// non-reversible.

export const REPAIR_PLAN_VERSION = 1;

function action(id, { kind = "command", command = null, reversible = true, details = {} }) {
  return { id, kind, command, reversible, details };
}

export function buildRepairPlan({ root, outDir = ".agent", graphState = "unknown", reasons = [] } = {}) {
  const actions = [];
  const blockers = reasons.filter((reason) => reason.severity === "blocker");
  const missingMap = blockers.some((reason) => reason.code === "missing_map");
  const corruptMap = blockers.some((reason) => reason.code === "corrupt_map");
  const staleGraph = blockers.some((reason) => reason.code === "stale_graph" || reason.code === "manifest_generation_mismatch");
  const eventGap = blockers.some((reason) => reason.code === "event_gap");
  const corruptStore = blockers.some((reason) => reason.code === "corrupt_understanding" || reason.code === "corrupt_portable_manifest");
  const missingDocs = blockers.some((reason) => reason.code === "missing_human_docs" || reason.code === "stale_human_docs");

  // Ordered: rebuild the graph before regenerating docs; reconcile gaps
  // before trusting freshness.
  if (missingMap || corruptMap || corruptStore) {
    actions.push(action("rebuild-graph", {
      command: `cortex build --out ${outDir}`,
      reversible: false,
      details: { reason: "graph artifacts missing or corrupt" },
    }));
  } else if (staleGraph) {
    actions.push(action("rebuild-graph", {
      command: `cortex build --out ${outDir}`,
      reversible: false,
      details: { reason: "graph stale relative to source" },
    }));
  }
  if (eventGap) {
    actions.push(action("reconcile-watcher", {
      command: `cortex-watch reconcile --root ${root}`,
      reversible: true,
      details: { reason: "watcher continuity not proven" },
    }));
  }
  if (missingDocs) {
    actions.push(action("regenerate-docs", {
      command: `cortex build --out ${outDir} --no-readme-link`,
      reversible: true,
      details: { reason: "generated docs missing or stale" },
    }));
  }
  if (actions.length === 0) {
    actions.push(action("no-op", {
      command: null,
      reversible: true,
      details: { reason: "no repair actions required" },
    }));
  }
  return {
    schemaVersion: REPAIR_PLAN_VERSION,
    root,
    graphState,
    actions,
  };
}
