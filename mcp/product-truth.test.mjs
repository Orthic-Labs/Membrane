import assert from "node:assert/strict";
import test from "node:test";
import {
  checkProductTruth,
  computeProductTruth,
  evaluateReadmeAgainstTruth,
} from "../scripts/tools/productization/generate-product-truth.mjs";

test("MBR-013: product truth is derived from live source (17 tools, 7 adapters)", async () => {
  const truth = await computeProductTruth();
  assert.equal(truth.schema, "membrane.product-truth.v1");
  assert.equal(truth.toolCount, 17);
  assert.equal(truth.tools.length, 17);
  assert.ok(truth.tools.includes("membrane_working_context"));
  assert.ok(truth.tools.includes("membrane_feedback"));
  // Live Diagnostics operational surface (design §12).
  for (const diagnosticTool of [
    "membrane_diagnostic_workspace",
    "membrane_diagnostic_mutation",
    "membrane_diagnostic_snapshot",
    "membrane_diagnostic_fence",
    "membrane_diagnostic_capabilities",
    "membrane_diagnostic_baseline",
    "membrane_diagnostic_provider",
  ]) {
    assert.ok(truth.tools.includes(diagnosticTool), `missing ${diagnosticTool}`);
  }
  assert.equal(truth.adapterCount, 7);
  // Tools are sorted for deterministic output.
  assert.deepEqual([...truth.tools].sort(), truth.tools);
});

test("MBR-013: the book-gate product-truth check passes on the current tree", async () => {
  const result = await checkProductTruth();
  assert.equal(result.ok, true, result.failures.join("; "));
  assert.deepEqual(result.failures, []);
});

test("MBR-013: the check fails when README says six tools while source exposes seventeen", async () => {
  const truth = await computeProductTruth();
  const staleReadme = "- **MCP server** — six tools over stdio (`membrane_context`, `membrane_source_read`, `membrane_knowledge_propose`, `membrane_checkpoint_save`, `membrane_checkpoint_load`, `membrane_feedback`).";
  const failures = evaluateReadmeAgainstTruth(staleReadme, truth, { docPresent: true });
  assert.ok(failures.some((f) => /claims 6 tools but source exposes 17/.test(f)), failures.join("; "));
  assert.ok(failures.some((f) => /tool list drift/.test(f)), failures.join("; "));
});

test("MBR-013: the check fails when a README-linked generated doc is absent", async () => {
  const truth = await computeProductTruth();
  const readme = "- **MCP server** — ten tools over stdio (`membrane_context`, `membrane_source_read`, `membrane_blueprint`, `membrane_knowledge_propose`, `membrane_checkpoint_save`, `membrane_checkpoint_load`, `membrane_working_context`, `membrane_temporal_fact`, `membrane_scratchpad`, `membrane_feedback`). See [docs/product-truth.md](docs/product-truth.md).";
  const failures = evaluateReadmeAgainstTruth(readme, truth, { docPresent: false });
  assert.ok(failures.some((f) => /generated doc absent/.test(f)), failures.join("; "));
});

test("MBR-013: the check fails when the README omits the generated-doc link", async () => {
  const truth = await computeProductTruth();
  const readme = "- **MCP server** — ten tools over stdio (`membrane_context`, `membrane_source_read`, `membrane_blueprint`, `membrane_knowledge_propose`, `membrane_checkpoint_save`, `membrane_checkpoint_load`, `membrane_working_context`, `membrane_temporal_fact`, `membrane_scratchpad`, `membrane_feedback`).";
  const failures = evaluateReadmeAgainstTruth(readme, truth, { docPresent: true });
  assert.ok(failures.some((f) => /does not link the generated product-truth doc/.test(f)), failures.join("; "));
});
