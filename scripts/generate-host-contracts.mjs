#!/usr/bin/env node
import { createHash } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { CAPABILITY_MATRIX_DIGEST, HOST_CAPABILITY_MATRIX } from "../mcp/host/capability-matrix.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const BLUEPRINT_SCHEMA = join(ROOT, "blueprint", "schemas", "context-candidate-set.v1.schema.json");
const ROOT_SCHEMA = join(ROOT, "schemas", "context-candidate-set.v1.schema.json");
const RUST_SCHEMA = join(ROOT, "engine", "crates", "membrane-protocol", "assets", "schemas", "context-candidate-set.v1.schema.json");
const REGISTRY_SCHEMA = join(ROOT, "schemas", "registry", "context-candidate-set.v1.schema.json");
const PROJECTION = join(ROOT, "schemas", "registry", "context-candidate-set.v1.projection.json");
const RUST_PROJECTION = join(ROOT, "schemas", "registry", "context-candidate-set.v1.rust.json");
const MATRIX = join(ROOT, "mcp", "host", "capability-matrix.v1.json");
const BLUEPRINT_SOURCE = "blueprint/schemas/context-candidate-set.v1.schema.json";
const GENERATOR = "scripts/generate-host-contracts.mjs";

function hash(text) { return `sha256:${createHash("sha256").update(text).digest("hex")}`; }
function stable(value) {
  if (Array.isArray(value)) return value.map(stable);
  if (value && typeof value === "object") return Object.fromEntries(Object.keys(value).sort().map((key) => [key, stable(value[key])]));
  return value;
}
function readJson(path) { return JSON.parse(readFileSync(path, "utf8")); }
function output(value) { return `${JSON.stringify(value, null, 2)}\n`; }

export function expectedArtifacts() {
  const schema = stable(readJson(BLUEPRINT_SCHEMA));
  const schemaText = output(schema);
  const sourceHash = hash(schemaText);
  const projection = {
    ...schema,
    "x-blueprint-source": BLUEPRINT_SOURCE,
    "x-blueprint-source-hash": sourceHash,
    "x-blueprint-generator": GENERATOR,
  };
  const registry = {
    ...projection,
    $id: "https://membrane/schemas/registry/context-candidate-set.v1.schema.json",
    title: "MembraneContextCandidateSetV1",
  };
  return {
    [ROOT_SCHEMA]: output(projection),
    [RUST_SCHEMA]: output(projection),
    [REGISTRY_SCHEMA]: output(registry),
    [PROJECTION]: output({ $schema: "https://json-schema.org/draft/2020-12/schema", $id: "https://membrane/schemas/projections/context-candidate-set.v1.json", title: "MembraneContextCandidateSetV1Projection", source: BLUEPRINT_SOURCE, sourceHash, generator: GENERATOR, projection: "json", schemaVersion: 1 }),
    [RUST_PROJECTION]: output({ schemaVersion: 1, source: BLUEPRINT_SOURCE, sourceHash, generator: GENERATOR, projection: "rust", required: ["schemaVersion", "traceId", "indexedAt"] }),
    [MATRIX]: output(stable({ ...HOST_CAPABILITY_MATRIX, sourceHash: CAPABILITY_MATRIX_DIGEST })),
  };
}

export function checkArtifacts() {
  const failures = [];
  for (const [path, expected] of Object.entries(expectedArtifacts())) {
    if (!existsSync(path)) { failures.push(`${path}: missing`); continue; }
    if (readFileSync(path, "utf8") !== expected) failures.push(`${path}: drift`);
  }
  return { valid: failures.length === 0, failures };
}

export function generate({ check = false } = {}) {
  const artifacts = expectedArtifacts();
  if (check) return checkArtifacts();
  for (const [path, text] of Object.entries(artifacts)) writeFileSync(path, text, "utf8");
  return { valid: true, generated: Object.keys(artifacts) };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const result = generate({ check: process.argv.includes("--check") });
  process.stdout.write(`${JSON.stringify(result)}\n`);
  if (!result.valid) process.exitCode = 2;
}
