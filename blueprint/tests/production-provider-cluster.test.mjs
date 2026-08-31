import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";

import { buildGraphGeneration, parseFileFacts, scanSourcesPublic } from "../src/graph/static-provider.mjs";
import { closeStore, loadGeneration, openStoreReadOnly } from "../src/graph/store-sqlite.mjs";

function fixture(files) {
  const root = mkdtempSync(join(tmpdir(), "blueprint-provider-cluster-"));
  for (const [path, text] of Object.entries(files)) {
    const target = join(root, path);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, text);
  }
  return root;
}

function providerIds(generation) {
  return new Set(generation.manifest.providerComposition.layers.map((layer) => layer.id));
}

test("production generation consumes module, framework, SQL, Terraform, SCIP, & explicit bridge providers", () => {
  const scipSymbol = "scip-python python fixture 1 pkg/util answer().";
  const root = fixture({
    "src/util.ts": "export function answer() { return 42; }\n",
    "src/main.ts": [
      "import { answer } from './util';",
      "import express from 'express';",
      "const app = express();",
      "app.get('/answer', answer);",
      "",
    ].join("\n"),
    "events.ts": "import { Kafka } from 'kafkajs';\npublish('orders.created');\n",
    "repo.ts": "import { PrismaClient } from '@prisma/client';\norders.save(order);\n",
    "db/001_orders.sql": "-- Migration: orders\nCREATE TABLE orders (id INTEGER);\nCREATE INDEX idx_orders ON orders(id);\n",
    "infra/main.tf": "resource \"aws_s3_bucket\" \"assets\" {}\n",
    "pkg/util.py": "def answer():\n    return 42\n",
    "pkg/main.py": "from pkg.util import answer\nanswer()\n",
    "native/lib.rs": "#[wasm_bindgen]\npub extern \"C\" fn start() {}\n",
    "native/bridge.go": "package native\nimport \"C\"\n",
    "api/service.proto": "service Greeter {\n  rpc Hello (Request) returns (Reply);\n}\n",
    "native/interop.cs": "[DllImport(\"kernel32.dll\")]\nstatic extern int Beep();\n",
    "index.scip.json": JSON.stringify({
      metadata: { version: "0.6.6", indexer: "scip-python" },
      documents: [
        { relativePath: "pkg/util.py", occurrences: [{ symbol: scipSymbol, roles: ["definition"], range: [0, 0, 0, 10] }] },
        { relativePath: "pkg/main.py", occurrences: [{ symbol: scipSymbol, roles: ["reference"], range: [0, 0, 0, 6] }] },
      ],
    }),
  });
  try {
    const generation = buildGraphGeneration(root, { outDir: ".agent", persist: true });
    const layers = providerIds(generation);
    for (const provider of [
      "blueprint-modules",
      "blueprint-frameworks",
      "blueprint-sql",
      "blueprint-terraform",
      "scip-python",
      "blueprint-bridge-seams",
    ]) assert.ok(layers.has(provider), provider);

    const moduleEdge = generation.edges.find((edge) => edge.kind === "IMPORTS"
      && edge.source === "file:src/main.ts"
      && edge.target === "file:src/util.ts");
    assert.ok(moduleEdge);
    assert.ok(moduleEdge.providerResolutions.some((claim) => claim.provider === "blueprint-modules" && claim.status === "RESOLVED"));
    assert.equal(moduleEdge.providerResolutions[0].evidence[0].contentHash.length > 0, true);

    const route = generation.nodes.find((node) => node.provider === "blueprint-frameworks" && node.labels.includes("HttpRoute"));
    assert.ok(route);
    assert.equal(route.routePath, "/answer");
    assert.equal(route.handler, "answer");
    assert.ok(generation.edges.some((edge) => edge.source === "file:events.ts" && edge.kind === "PRODUCES"));
    assert.ok(generation.edges.some((edge) => edge.source === "file:repo.ts" && edge.kind === "WRITES"));

    const sql = generation.nodes.find((node) => node.provider === "blueprint-sql" && node.name === "orders");
    assert.ok(sql);
    const terraform = generation.nodes.find((node) => node.provider === "blueprint-terraform" && node.resourceType === "aws_s3_bucket");
    assert.ok(terraform);

    const scipReference = generation.edges.find((edge) => edge.provider === "scip-python" && edge.kind === "REFERENCES");
    assert.ok(scipReference);
    assert.equal(scipReference.target, `symbol:pkg/util.py::${scipSymbol}`);
    assert.equal(scipReference.confidenceTier, "EXACT_RESOLUTION");
    assert.ok(scipReference.evidence[0].contentHash);

    const bridges = generation.nodes.filter((node) => node.provider === "blueprint-bridge-seams");
    assert.deepEqual(new Set(bridges.map((node) => node.bridgeKind)), new Set(["FFI", "cgo", "gRPC", "PInvoke", "WASM"]));
    const bridgeEdges = generation.edges.filter((edge) => edge.provider === "blueprint-bridge-seams");
    assert.ok(bridgeEdges.length >= bridges.length);
    assert.ok(bridgeEdges.every((edge) => edge.kind === "CONTAINS"));
    assert.ok(!bridgeEdges.some((edge) => edge.kind === "CALLS"));
    assert.ok(bridges.every((node) => node.evidence[0].contentHash));

    assert.equal(generation.augmentation.providers.scip.state, "ok");
    assert.ok(generation.augmentation.providers.frameworks.gatedFiles >= 3);
    assert.ok(generation.augmentation.providers.bridges.seams >= 5);

    const db = openStoreReadOnly(join(root, ".agent", "graph", "graph.db"));
    try {
      const loaded = loadGeneration(db);
      assert.ok(loaded.nodes.some((node) => node.provider === "blueprint-terraform"));
      assert.ok(loaded.edges.some((edge) => edge.provider === "blueprint-bridge-seams" && edge.kind === "CONTAINS"));
      assert.ok(!loaded.edges.some((edge) => edge.provider === "blueprint-bridge-seams" && edge.kind === "CALLS"));
    } finally {
      closeStore(db);
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("framework facts stay absent without explicit gates & seam comments never become evidence", () => {
  const root = fixture({
    "plain.ts": "app.get('/fake', handler);\npublish('fake.topic');\norders.save(value);\n",
    "comments.go": "package native\n// import \"C\"\n",
    "comments.cs": "// [DllImport(\"fake.dll\")]\n",
    "comments.py": "#ctypes.CDLL('fake.so')\n",
  });
  try {
    const generation = buildGraphGeneration(root);
    assert.equal(generation.nodes.filter((node) => node.provider === "blueprint-frameworks").length, 0);
    assert.equal(generation.nodes.filter((node) => node.provider === "blueprint-bridge-seams").length, 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("incremental file build emits same local provider facts without seam CALLS", () => {
  const root = fixture({
    "infra/main.tf": "resource \"aws_queue\" \"jobs\" {}\n",
    "native/bridge.go": "package native\nimport \"C\"\n",
  });
  try {
    const files = scanSourcesPublic(root).files;
    const terraform = files.find((file) => file.path === "infra/main.tf");
    const terraformFacts = parseFileFacts(root, terraform, { files });
    assert.ok(terraformFacts.nodes.some((node) => node.provider === "blueprint-terraform" && node.resourceName === "jobs"));
    const bridge = files.find((file) => file.path === "native/bridge.go");
    const bridgeFacts = parseFileFacts(root, bridge, { files });
    assert.ok(bridgeFacts.nodes.some((node) => node.provider === "blueprint-bridge-seams" && node.bridgeKind === "cgo"));
    assert.ok(!bridgeFacts.edges.some((edge) => edge.provider === "blueprint-bridge-seams" && edge.kind === "CALLS"));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
