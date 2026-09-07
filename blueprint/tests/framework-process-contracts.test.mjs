import assert from "node:assert/strict";
import test from "node:test";

import { augmentFrameworkIntelligence } from "../src/graph/framework-intelligence.mjs";
import { buildContractRegistry, bridgeContractRegistries, stitchContractTraces } from "../src/graph/contract-registry.mjs";
import { buildProcessProjection } from "../src/graph/process-projection.mjs";
import { detectProjectConventions } from "../src/graph/conventions.mjs";

function ev(path, line = 1) { return [{ path, startLine: line, endLine: line, contentHash: `h:${path}` }]; }
function baseGeneration() {
  return {
    manifest: { generationId: "g1" },
    nodes: [
      { id: "file:src/api.ts", kind: "file", path: "src/api.ts", labels: ["File"], evidence: ev("src/api.ts") },
      { id: "file:src/screen.tsx", kind: "file", path: "src/screen.tsx", labels: ["File"], evidence: ev("src/screen.tsx") },
      { id: "symbol:src/api.ts::handlePing", kind: "symbol", path: "src/api.ts", name: "handlePing", qualifiedName: "handlePing", labels: ["Function"], evidence: ev("src/api.ts", 2) },
      { id: "symbol:src/api.ts::dependency", kind: "symbol", path: "src/api.ts", name: "dependency", qualifiedName: "dependency", labels: ["Function"], evidence: ev("src/api.ts", 3) },
      { id: "symbol:src/screen.tsx::Home", kind: "symbol", path: "src/screen.tsx", name: "Home", qualifiedName: "Home", labels: ["Function"], evidence: ev("src/screen.tsx", 2) },
      { id: "symbol:src/api.ts::main", kind: "symbol", path: "src/api.ts", name: "main", qualifiedName: "main", labels: ["Function", "Entrypoint"], entryPoint: true, evidence: ev("src/api.ts", 1) },
    ],
    edges: [
      { id: "call-main", kind: "CALLS", source: "symbol:src/api.ts::main", target: "symbol:src/api.ts::handlePing", confidenceTier: "EXACT_RESOLUTION", resolved: true, evidence: ev("src/api.ts") },
    ],
  };
}

const files = [
  { path: "src/api.ts", contentHash: "h:api", text: `
export function main() { handlePing(); }
export function handlePing() {}
export function dependency() {}
const x = process.env.API_URL;
const users = prisma.user.findMany();
@Inject("mailer")
const dep = Depends(dependency);
mcp.tool("ping", handlePing);
callTool("remote-tool");
` },
  { path: "src/screen.tsx", contentHash: "h:screen", text: `
export function Home() { return null; }
<Route path="/" element={<Home />} />
navigate("/settings");
` },
];

test("framework intelligence emits distinct DI ORM config RPC and UI evidence", () => {
  const generation = baseGeneration();
  const summary = augmentFrameworkIntelligence(generation, files);
  assert.ok(summary.di >= 2);
  assert.ok(summary.orm >= 1);
  assert.ok(summary.config >= 1);
  assert.ok(summary.rpc >= 2);
  assert.ok(summary.ui >= 2);
  assert.ok(generation.nodes.some((node) => node.labels?.includes("ConfigKey") && node.name === "API_URL"));
  assert.ok(generation.nodes.some((node) => node.labels?.includes("DatabaseModel") && node.name === "user"));
  const ping = generation.nodes.find((node) => node.labels?.includes("ToolContract") && node.name === "ping");
  assert.ok(ping);
  assert.ok(generation.edges.some((edge) => edge.kind === "HANDLES" && edge.source === ping.id && edge.target === "symbol:src/api.ts::handlePing"));
  const homeRoute = generation.nodes.find((node) => node.labels?.includes("UiRoute") && node.name === "/");
  assert.ok(generation.edges.some((edge) => edge.kind === "ROUTES_TO" && edge.source === homeRoute.id && edge.target === "symbol:src/screen.tsx::Home"));
});

test("contract registry and bridge stitching join only exact contract keys", () => {
  const provider = baseGeneration();
  augmentFrameworkIntelligence(provider, files);
  const providerTool = provider.nodes.find((node) => node.labels?.includes("ToolContract") && node.name === "ping");
  providerTool.contractRoles = ["provider"];
  const a = buildContractRegistry(provider, { repoId: "provider-repo" });

  const consumer = { manifest: { generationId: "g2" }, nodes: [], edges: [] };
  const consumerFiles = [{ path: "src/client.ts", contentHash: "h:c", text: `callTool("ping"); callTool("pinger");` }];
  consumer.nodes.push({ id: "file:src/client.ts", kind: "file", path: "src/client.ts", labels: ["File"], evidence: ev("src/client.ts") });
  augmentFrameworkIntelligence(consumer, consumerFiles);
  const b = buildContractRegistry(consumer, { repoId: "consumer-repo" });
  const bridges = bridgeContractRegistries([a, b]);
  assert.equal(bridges.bridges.length, 1);
  assert.equal(bridges.bridges[0].address, "ping");
  const traces = stitchContractTraces([a, b]);
  assert.equal(traces.traces.length, 1);
  assert.deepEqual(traces.traces[0].steps.map((step) => step.role), ["consumer", "provider"]);
});

test("process projection derives disposable Process/Step views from explicit entrypoints", () => {
  const generation = baseGeneration();
  const projection = buildProcessProjection(generation);
  assert.equal(projection.kind, "process-projection");
  const process = projection.processes.find((row) => row.entryPoint.id === "symbol:src/api.ts::main");
  assert.ok(process);
  assert.deepEqual(process.steps.map((step) => step.node.id), ["symbol:src/api.ts::main", "symbol:src/api.ts::handlePing"]);
  assert.equal(process.steps[0].ordinal, 0);
  assert.equal(process.steps[1].parentStepId, process.steps[0].stepId);
});

test("conventions are weak descriptive evidence and retain counterexamples", () => {
  const conventionFiles = [
    { path: "src/foo-bar.ts" }, { path: "src/baz-qux.ts" }, { path: "src/other-name.ts" }, { path: "src/not_style.ts" },
    { path: "tests/a.test.ts" }, { path: "tests/b.test.ts" }, { path: "tests/c.test.ts" }, { path: "src/d.spec.ts" },
  ];
  const result = detectProjectConventions(conventionFiles, { minimumExamples: 3, minimumCoverage: 0.6 });
  assert.equal(result.evidenceClass, "WeakEvidence");
  assert.equal(result.policyAuthority, false);
  const naming = result.evidence.find((row) => row.kind === "file_naming");
  assert.ok(naming);
  assert.ok(naming.counterexamples.some((path) => path.includes("not_style")));
  assert.equal(naming.policyAuthority, false);
});
