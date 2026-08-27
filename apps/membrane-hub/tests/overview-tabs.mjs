import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { renderView } from "../src/overview.mjs";

const payload = {
  schemaVersion: 1,
  observedAtUnixMs: Date.now(),
  sections: {
    memory: { state: "available", items: [{ memoryCount: 7 }] },
    providers: { state: "available", items: [{ service: "membrane", ok: true }] },
    repositories: { state: "unavailable", reason: "transport_unavailable" },
  },
  subsystems: { pull: { state: "not_configured", reason: "not_instrumented" } },
  admission: {
    schemaVersion: 1,
    windowHours: 24,
    decisionsTotal: 12,
    omissionsTotal: 3,
    omissionsByReason: [{ reason: "cross_root", count: 1 }, { reason: "budget_exhausted", count: 2 }],
    budgetPressureTotal: 2,
  },
};
const runtime = { serviceState: "running", snapshotState: "available", lastReason: "ok" };

test("Hub dashboard exposes functional Overview, Ledger, Sources, and Subsystems projections", () => {
  for (const [view, marker] of [["overview", "data-admission-chart"], ["ledger", "data-ledger-filters"], ["sources", "source-meta"], ["subsystems", "subsystem-detail-list"]]) {
    const root = { innerHTML: "" };
    renderView(payload, root, runtime, view);
    assert.match(root.innerHTML, new RegExp(`view-${view}`));
    assert.match(root.innerHTML, new RegExp(marker));
  }
});

test("Hub shell routes selected tab through hash navigation without startup owner", async () => {
  const html = await readFile(new URL("../index.html", import.meta.url), "utf8");
  assert.match(html, /data-tab="overview"/);
  assert.match(html, /data-tab="ledger"/);
  assert.match(html, /data-tab="sources"/);
  assert.match(html, /data-tab="subsystems"/);
  assert.match(html, /hashchange/);
  assert.match(html, /window\.__membraneHub/);
  assert.doesNotMatch(html, /Launch Membrane Hub at login|set_startup|startup_setting/);
});

test("Overview owns desktop viewport while narrow layouts may scroll", async () => {
  const css = await readFile(new URL("../src/overview.css", import.meta.url), "utf8");
  assert.match(css, /\.body\[data-view="overview"\]\{overflow:hidden/);
  assert.match(css, /\.body\[data-view="overview"\]\{overflow:auto/);
  assert.match(css, /\.view-overview\{height:100%;min-height:0/);
});
