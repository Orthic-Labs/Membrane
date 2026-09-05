import assert from "node:assert/strict";
import test from "node:test";
import { BUILD_REQUIRED_TESTS, selectTestFiles } from "../scripts/test-random.mjs";

test("no-build corpus reports exactly the package-producing omissions", () => {
  const files = ["tests/store-sqlite.test.mjs", ...Object.keys(BUILD_REQUIRED_TESTS)];
  assert.deepEqual(selectTestFiles(files).included, files);
  assert.deepEqual(selectTestFiles(files, { noBuild: true }).included, [files[0]]);
  const omitted = selectTestFiles(files, { noBuild: true }).excluded;
  assert.equal(omitted.length, 3);
  assert.ok(omitted.every((item) => item.reason.length > 0));
  assert.equal(selectTestFiles(["tests\\runtime-bundle.test.mjs"], { noBuild: true }).excluded.length, 1);
});
