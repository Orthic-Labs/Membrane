import assert from "node:assert/strict";
import test from "node:test";
import * as boundary from "../../scripts/ci/check-network-boundary.mjs";

test("POSIX inventory paths preserve literal backslashes", () => {
  assert.equal(
    boundary.toInventoryPath("lib\\literal\\name.mjs", "/"),
    "lib\\literal\\name.mjs",
  );
});

test("Windows inventory paths normalize native separators", () => {
  assert.equal(
    boundary.toInventoryPath("src\\lib\\http-server.mjs", "\\"),
    "src/lib/http-server.mjs",
  );
});
