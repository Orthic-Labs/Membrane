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

test("CCS projections declare Membrane schema ownership", () => {
  const root = JSON.parse(readFileSync(ROOT_SCHEMA, "utf8"));
  const rust = JSON.parse(readFileSync(RUST_SCHEMA, "utf8"));
  assert.equal(root["x-membrane-source"], undefined);
  assert.equal(root["x-blueprint-source"], undefined);
  assert.equal(rust["x-membrane-source"], "schemas/context-candidate-set.v1.schema.json");
  assert.match(rust["x-membrane-source-hash"], /^sha256:[a-f0-9]{64}$/);
  assert.equal(rust["x-membrane-generator"], "scripts/generate-host-contracts.mjs");
  assert.equal(rust.description, root.description);
  assert.deepEqual(rust.$defs, root.$defs);
});
