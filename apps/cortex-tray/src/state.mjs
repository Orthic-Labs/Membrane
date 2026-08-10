export const TRAY_STATES = Object.freeze(["offline", "starting", "healthy", "degraded", "stopping"]);

export function trayView(snapshot, lifecycle = "running") {
  if (lifecycle === "starting" || lifecycle === "stopping") return { state: lifecycle, summary: lifecycle === "starting" ? "Starting Cortex" : "Stopping Cortex" };
  if (!snapshot || snapshot.error) return { state: "offline", summary: "Cortex is offline", reason: snapshot?.error?.message ?? "No service response" };
  const freshness = String(snapshot.state ?? snapshot.freshness?.state ?? "unknown");
  const degraded = freshness !== "fresh" || Boolean(snapshot.degraded) || Number(snapshot.degradations?.length ?? 0) > 0;
  return {
    state: degraded ? "degraded" : "healthy",
    summary: degraded ? "Cortex needs attention" : "Cortex is healthy",
    reason: snapshot.reason ?? (degraded ? `Freshness: ${freshness}` : "Repository evidence is current"),
    repository: snapshot.repoRoot ?? snapshot.repository?.root ?? "Current repository",
    repositories: (snapshot.enrolledRepos ?? []).map((repo) => repo.root).filter(Boolean),
    freshness,
    watcher: snapshot.watcher?.state ?? snapshot.watchState ?? (degraded ? "degraded" : "running"),
  };
}

export function transition(current, event) {
  if (event === "start") return "starting";
  if (event === "stop") return "stopping";
  if (event === "offline") return "offline";
  if (event === "healthy" || event === "degraded") return event;
  return TRAY_STATES.includes(current) ? current : "offline";
}

export function diagnostics(view) {
  return { state: view.state, freshness: view.freshness ?? null, watcher: view.watcher ?? null, repository: view.repository ?? null, repositories: view.repositories ?? [] };
}
