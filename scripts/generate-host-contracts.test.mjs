import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { checkArtifacts } from "./generate-host-contracts.mjs";

const ROOT_SCHEMA = new URL("../schemas/context-candidate-set.v1.schema.json", import.meta.url);
const RUST_SCHEMA = new URL("../engine/crates/membrane-protocol/assets/schemas/context-candidate-set.v1.schema.json", import.meta.url);

test("host contract projections are deterministic and current", () => {
  const result = checkArtifacts();
  assert.equal(result.valid, true, result.failures.join(", "));
});

test("Membrane schema projections declare Blueprint ownership", () => {
  const root = JSON.parse(readFileSync(ROOT_SCHEMA, "utf8"));
  const rust = JSON.parse(readFileSync(RUST_SCHEMA, "utf8"));
  for (const schema of [root, rust]) {
    assert.equal(schema["x-blueprint-source"], "blueprint/schemas/context-candidate-set.v1.schema.json");
    assert.match(schema["x-blueprint-source-hash"], /^sha256:[a-f0-9]{64}$/);
    assert.equal(schema["x-blueprint-generator"], "scripts/generate-host-contracts.mjs");
  }
  assert.equal(root["x-blueprint-source-hash"], rust["x-blueprint-source-hash"]);
});
