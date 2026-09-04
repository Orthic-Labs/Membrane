import assert from "node:assert/strict";
import test from "node:test";

import { deriveBlueprintReadiness } from "../src/service/readiness.mjs";
import { serviceStatus } from "../src/service/status.mjs";

test("readiness distinguishes enrolled/live/current from merely enrolled", () => {
  const current = deriveBlueprintReadiness({
    graphState: "fresh",
    runtime: { targetEnrolled: true, targetWatcherLive: true, hubAvailable: true, running: true },
  });
  assert.equal(current.state, "ready_current");
  assert.equal(current.watcher.owned, true);
  assert.equal(current.graph.state, "current");

  const unwatched = deriveBlueprintReadiness({
    graphState: "fresh",
    runtime: { targetEnrolled: true, targetWatcherLive: false, hubAvailable: true, running: false },
  });
  assert.equal(unwatched.state, "installed_unwatched");
});

test("readiness reports catching-up, hub failure, and MCP failure as distinct product states", () => {
  assert.equal(deriveBlueprintReadiness({
    graphState: "stale",
    runtime: { targetEnrolled: true, targetWatcherLive: true, hubAvailable: true, running: true },
  }).state, "ready_catching_up");

  assert.equal(deriveBlueprintReadiness({
    graphState: "fresh",
    runtime: { targetEnrolled: true, targetWatcherLive: false, hubAvailable: false },
  }).state, "installed_hub_unavailable");

  assert.equal(deriveBlueprintReadiness({
    graphState: "fresh",
    runtime: { targetEnrolled: true, targetWatcherLive: true, hubAvailable: true },
    mcp: { probe: "failed" },
  }).state, "installed_mcp_unavailable");
});

test("service status exposes target enrollment separately from watcher liveness", () => {
  const target = "/tmp/example-repo";
  const status = serviceStatus({
    target,
    fleetStatus: () => ({ repos: [{ root: target, pid: 123, alive: false }] }),
  });
  assert.equal(status.targetEnrolled, true);
  assert.equal(status.targetWatcherLive, false);
  assert.equal(status.hubAvailable, true);
  assert.equal(status.running, false);
});
