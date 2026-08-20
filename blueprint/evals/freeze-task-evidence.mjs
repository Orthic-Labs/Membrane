#!/usr/bin/env node
import { createXXHash128 } from "hash-wasm";

// XXH3-128, matching production. The harness previously hashed fixtures with
// sha256 while the providers emitted xxh128, and that impedance mismatch is
// exactly what silently failed every qualification task after 3988a735.
const xxhasher = await createXXHash128();

function xxh3Hex(bytes) {
  xxhasher.init();
  xxhasher.update(bytes);
  return xxhasher.digest("hex");
}

import { readFileSync, renameSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const STRUCTURAL_TASKS = new Set([
  "ts-symbol-definition",
  "ts-call-path",
  "ts-import-path",
  "ts-route-storage-flow",
  "ts-blast-radius",
  "ts-test-coverage",
  "ts-config-resource",
]);

const EXPECTED_EDGES = {
  "ts-call-path": [
    { source: "src/routes.ts::registerOrderRoute", kind: "CALLS", target: "src/service.ts::OrderService.placeOrder" },
    { source: "src/service.ts::OrderService.placeOrder", kind: "CALLS", target: "src/store.ts::OrderStore.find" },
    { source: "src/service.ts::OrderService.placeOrder", kind: "CALLS", target: "src/store.ts::OrderStore.save" },
  ],
  "ts-import-path": [
    { source: "src/routes.ts", kind: "IMPORTS", target: "src/service.ts" },
    { source: "src/service.ts", kind: "IMPORTS", target: "src/store.ts" },
  ],
  "ts-route-storage-flow": [
    { source: "src/routes.ts::registerOrderRoute", kind: "CALLS", target: "src/service.ts::OrderService.placeOrder" },
    { source: "src/service.ts::OrderService.placeOrder", kind: "CALLS", target: "src/store.ts::OrderStore.save" },
  ],
  "ts-blast-radius": [
    { source: "src/service.ts::OrderService.placeOrder", kind: "CALLS", target: "src/store.ts::OrderStore.save" },
    { source: "test/service.test.ts::placeOrder stores a new order", kind: "TESTS", target: "src/service.ts::OrderService.placeOrder" },
  ],
  "ts-test-coverage": [
    { source: "test/service.test.ts::placeOrder stores a new order", kind: "TESTS", target: "src/service.ts::OrderService.placeOrder" },
  ],
  "ts-config-resource": [
    { source: "src/config.ts::ORDER_DB_PATH", kind: "CONFIGURES", target: "resources/orders.json" },
  ],
};

function parseArgs(argv) {
  const args = {};
  for (let index = 0; index < argv.length; index += 1) {
    const key = argv[index];
    if (!key.startsWith("--")) throw new Error(`unexpected argument: ${key}`);
    const name = key.slice(2);
    const next = argv[index + 1];
    if (next && !next.startsWith("--")) {
      args[name] = next;
      index += 1;
    } else {
      args[name] = true;
    }
  }
  return args;
}

function sha256(path) {
  return xxh3Hex(readFileSync(path));
}

export function freezeTaskEvidence(tasksPath, options = {}) {
  const absoluteTasks = resolve(tasksPath);
  const reposRoot = resolve(options.reposRoot ?? join(dirname(absoluteTasks), "fixture-repos"));
  const tasks = readFileSync(absoluteTasks, "utf8")
    .split(/\r?\n/)
    .filter(Boolean)
    .map((line) => JSON.parse(line));
  const limit = Number(options.limit ?? 0);
  const selected = Number.isInteger(limit) && limit > 0 ? tasks.slice(0, limit) : tasks;
  const hashes = new Map();
  for (const task of selected) {
    task.qualificationClass = STRUCTURAL_TASKS.has(task.id)
      ? "mandatory_structural"
      : task.kind === "semantic_lookup" ? "semantic_optional" : "blueprint_integration";
    task.expectedEdges = EXPECTED_EDGES[task.id] ?? [];
    task.expectedEvidence = task.expectedEvidence.map((evidence) => ({
      ...evidence,
      contentHash: (() => {
        const path = join(reposRoot, task.repo, evidence.path);
        if (!hashes.has(path)) hashes.set(path, sha256(path));
        return hashes.get(path);
      })(),
    }));
  }
  if (options.write) {
    if (selected.length !== tasks.length) throw new Error("--write cannot be combined with --limit");
    const temporary = `${absoluteTasks}.${process.pid}.tmp`;
    writeFileSync(temporary, `${tasks.map((task) => JSON.stringify(task)).join("\n")}\n`);
    renameSync(temporary, absoluteTasks);
  }
  return {
    tasks: selected.length,
    evidence: selected.reduce((sum, task) => sum + task.expectedEvidence.length, 0),
    wrote: Boolean(options.write),
  };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = parseArgs(process.argv.slice(2));
  const result = freezeTaskEvidence(
    args.tasks ?? resolve(process.cwd(), "evals/graph-tasks.jsonl"),
    { reposRoot: args["repos-root"], limit: args.limit, write: args.write },
  );
  process.stdout.write(`${JSON.stringify(result)}\n`);
}
