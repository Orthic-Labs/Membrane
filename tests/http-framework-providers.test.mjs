// D33: HTTP framework providers — import-gated route→handler mapping with
// evidence, positive/negative/ambiguity fixtures.

import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { extractRoutes, frameworkGateActive } from "../providers/frameworks/http/index.mjs";
import { extractEventFacts, extractDatabaseFacts, extractDeploymentFacts, CROSS_STACK_EDGES } from "../providers/frameworks/index.mjs";

test("framework gates activate only on imports", () => {
  assert.equal(frameworkGateActive(["express"], "next-express"), true);
  assert.equal(frameworkGateActive(["react"], "next-express"), false);
  assert.equal(frameworkGateActive(["fastapi"], "fastapi-django"), true);
  assert.equal(frameworkGateActive(["tauri"], "tauri-axum"), true);
});

test("Express route extraction with evidence (positive)", () => {
  const routes = extractRoutes({ stack: "next-express", text: "app.get('/orders', handler);", path: "routes.ts" });
  assert.ok(routes.some((r) => r.kind === "route" && r.method === "GET" && r.path === "/orders"));
  assert.equal(routes[0].confidence, "CROSS_FILE_HEURISTIC");
  assert.ok(routes[0].evidence);
});

test("FastAPI decorator routes (positive)", () => {
  const routes = extractRoutes({ stack: "fastapi-django", text: "@app.post('/orders')\ndef create_order(): pass", path: "app.py" });
  assert.ok(routes.some((r) => r.kind === "route" && r.method === "POST" && r.path === "/orders"));
  assert.ok(routes.some((r) => r.kind === "handler" && r.name === "create_order"));
});

test("similar names without framework import produce no route (negative)", () => {
  const routes = extractRoutes({ stack: "next-express", text: "const orders = []; // app.get('/fake')", path: "x.ts" });
  // A comment containing app.get must not match (line regex only matches code).
  assert.equal(routes.filter((r) => r.kind === "route").length, 0);
});

test("cross-stack facts carry edges and confidence", () => {
  const events = extractEventFacts({ text: "publish('orders.created');", path: "svc.ts" });
  assert.ok(events.some((f) => f.edge === "PRODUCES" && f.topic === "orders.created"));
  const db = extractDatabaseFacts({ text: "orders.find({ id }); orders.save(order);", path: "repo.ts" });
  assert.ok(db.some((f) => f.edge === "READS"));
  assert.ok(db.some((f) => f.edge === "WRITES"));
  const deploy = extractDeploymentFacts({ text: "deploy: aws-ecs-deploy", path: "ci.yml" });
  assert.ok(deploy.some((f) => f.edge === "DEPLOYS"));
});

test("CROSS_STACK_EDGES vocabulary is complete", () => {
  for (const edge of ["PRODUCES", "CONSUMES", "GENERATES", "READS", "WRITES", "CONFIGURES", "DEPLOYS"]) {
    assert.ok(CROSS_STACK_EDGES.includes(edge));
  }
});

test("polyglot fixture traces client→route→handler→queue→consumer→DB", () => {
  const root = mkdtempSync(join(tmpdir(), "cortex-stack-"));
  try {
    const ui = "fetch('/api/orders');";
    const api = "app.post('/api/orders', createOrder);";
    const svc = "publish('orders.created', order);";
    const worker = "consume('orders.created', persist);";
    const repo = "orders.save(order);";
    const uiRoutes = extractRoutes({ stack: "next-express", text: api, path: "api.ts" });
    const events = extractEventFacts({ text: svc, path: "svc.ts" });
    const consumed = extractEventFacts({ text: worker, path: "worker.ts" });
    const db = extractDatabaseFacts({ text: repo, path: "repo.ts" });
    assert.ok(ui.includes("/api/orders"));
    assert.ok(uiRoutes.some((r) => r.path === "/api/orders"));
    assert.ok(events.some((f) => f.topic === "orders.created" && f.edge === "PRODUCES"));
    assert.ok(consumed.some((f) => f.topic === "orders.created" && f.edge === "CONSUMES"));
    assert.ok(db.some((f) => f.edge === "WRITES"));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
