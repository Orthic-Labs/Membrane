// Canonical Blueprint operational readiness model (BPT-100 / BPI-004).
// Liveness and semantic readiness are separate axes: an enrolled repo is not
// necessarily being watched, and a live watcher does not make a stale graph current.

function graphReadiness(graphState) {
  switch (String(graphState ?? "missing")) {
    case "fresh": return "current";
    case "stale": return "catching_up";
    case "indeterminate": return "degraded";
    case "incomplete": return "degraded";
    case "corrupt": return "degraded";
    case "missing": return "missing";
    default: return "degraded";
  }
}

export function deriveBlueprintReadiness({ graphState, runtime = {}, mcp = null, providers = {}, projections = {} } = {}) {
  const graph = graphReadiness(graphState);
  const targetEnrolled = runtime.targetEnrolled === true;
  const watcherLive = runtime.targetWatcherLive === true;
  const hubAvailable = runtime.hubAvailable !== false;
  const mcpOk = mcp == null || mcp.probe === "ok" || mcp.available === true;
  const degradedProvider = Object.values(providers).some((state) => String(state).startsWith("degraded") || state === "failed");

  let state;
  if (!hubAvailable) state = "installed_hub_unavailable";
  else if (!mcpOk) state = "installed_mcp_unavailable";
  else if (degradedProvider) state = "degraded_provider";
  else if (!targetEnrolled || !watcherLive) state = "installed_unwatched";
  else if (graph === "current") state = "ready_current";
  else if (graph === "catching_up") state = "ready_catching_up";
  else state = "degraded_provider";

  return Object.freeze({
    schemaVersion: 1,
    state,
    service: Object.freeze({ live: Boolean(runtime.serviceLive ?? runtime.running), hubAvailable }),
    watcher: Object.freeze({ enrolled: targetEnrolled, owned: watcherLive, state: watcherLive ? graph : "unwatched" }),
    graph: Object.freeze({ state: graph }),
    providers: Object.freeze({ ...providers }),
    projections: Object.freeze({ ...projections }),
    mcp: Object.freeze(mcp ?? { probe: "unknown" }),
  });
}
