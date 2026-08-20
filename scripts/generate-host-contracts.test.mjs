import assert from "node:assert/strict";
import test from "node:test";
import { checkArtifacts } from "./generate-host-contracts.mjs";

test("host contract projections are deterministic and current", () => {
  const result = checkArtifacts();
  assert.equal(result.valid, true, result.failures.join(", "));
});
