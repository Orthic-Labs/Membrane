import assert from "node:assert/strict";
import test from "node:test";

import { MEM_044, MEM_052, MEM_053 } from "./semantic-producer-windows.mjs";

test("semantic-producer case module performs no build/test/install execution", async () => {
  const { readFileSync } = await import("node:fs");
  const source = readFileSync(new URL("./semantic-producer-windows.mjs", import.meta.url), "utf8");
  assert.doesNotMatch(source, /spawnSync|execFileSync|child_process|cargo\s|pnpm\s+(test|build|install)/u);
});

test("MEM-044: bounded/idempotent/cancellable/checkpointable maintenance passes on real source", async () => {
  const result = await MEM_044();
  assert.equal(result.id, "MEM-044");
  assert.deepEqual(result.findings, []);
  assert.ok(result.passed, JSON.stringify(result));
  assert.ok(result.negativeControls.length > 0);
  for (const control of result.negativeControls) {
    assert.ok(control.passed, `${control.control} did not fail on its injected fault`);
  }
});

test("MEM-052: admission-under-policy passes on real source", async () => {
  const result = await MEM_052();
  assert.equal(result.id, "MEM-052");
  assert.deepEqual(result.findings, []);
  assert.ok(result.passed, JSON.stringify(result));
  for (const control of result.negativeControls) {
    assert.ok(control.passed, `${control.control} did not fail on its injected fault`);
  }
});

test("MEM-053: authenticated proposal-only review with persisted sink passes on real source", async () => {
  const result = await MEM_053();
  assert.equal(result.id, "MEM-053");
  assert.deepEqual(result.findings, []);
  assert.ok(result.passed, JSON.stringify(result));
  for (const control of result.negativeControls) {
    assert.ok(control.passed, `${control.control} did not fail on its injected fault`);
  }
});

test("negative controls actually fail when the marker they depend on is absent (self-check)", async () => {
  // A negative control that always reports passed regardless of the source
  // would be worthless. Prove each control's own fault-injection primitive
  // is capable of failing by pointing every case at a workspace root whose
  // source files are missing the markers entirely (empty stand-ins),
  // instead of only ever reading the real, currently-correct source.
  const { mkdtempSync, mkdirSync, writeFileSync } = await import("node:fs");
  const { tmpdir } = await import("node:os");
  const { join } = await import("node:path");

  const root = mkdtempSync(join(tmpdir(), "semantic-producer-case-"));
  const runtimeSrc = join(root, "engine/crates/membrane-runtime/src");
  mkdirSync(runtimeSrc, { recursive: true });
  writeFileSync(join(runtimeSrc, "background_review.rs"), "// intentionally empty stand-in\n");
  writeFileSync(join(runtimeSrc, "background_review_input.rs"), "// intentionally empty stand-in\n");

  for (const run of [MEM_044, MEM_052, MEM_053]) {
    const result = await run({ workspaceRoot: root });
    assert.equal(result.passed, false, `${result.id} must fail against source missing every marker`);
    assert.ok(result.findings.length > 0, `${result.id} must report which markers are missing`);
  }
});
