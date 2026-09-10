import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { BM09, BM11, CASES, extractDescriptorBlocks, probeBM09Installed, probeBM11Installed, installedBinding, probeInstalledMembraneRow } from "./mem-windows.mjs";

const GOOD_HOOK_SOURCE = `
pub fn hook_injection_point_descriptors() -> [HookInjectionPointDescriptorV1; 8] {
    use HookInjectionPointId::*;
    [
        HookInjectionPointDescriptorV1 {
            id: SessionStart, canonical_name: "session_start",
            host_names: &["SessionStart"],
            purpose: "p", when_to_use: "w", when_not_to_use: "never on resume",
            input: "i", output: "o", cost_bound: "c", freshness_bound: "f",
            effect_bound: "e", budget: "b", dedup: "d", suppression: "s",
            receipt_kind: "r",
        },
        HookInjectionPointDescriptorV1 {
            id: UserPrompt, canonical_name: "user_prompt",
            host_names: &["UserPromptSubmit"],
            purpose: "p", when_to_use: "w", when_not_to_use: "never without token",
            input: "i", output: "o", cost_bound: "c", freshness_bound: "f",
            effect_bound: "e", budget: "b", dedup: "d", suppression: "s",
            receipt_kind: "r",
        },
        HookInjectionPointDescriptorV1 {
            id: PreTool, canonical_name: "pre_tool",
            host_names: &["PreToolUse"],
            purpose: "p", when_to_use: "w", when_not_to_use: "never for inspection",
            input: "i", output: "o", cost_bound: "c", freshness_bound: "f",
            effect_bound: "e", budget: "b", dedup: "d", suppression: "s",
            receipt_kind: "r",
        },
        HookInjectionPointDescriptorV1 {
            id: PostEdit, canonical_name: "post_edit",
            host_names: &["PostToolUse"],
            purpose: "p", when_to_use: "w", when_not_to_use: "never for reads",
            input: "i", output: "o", cost_bound: "c", freshness_bound: "f",
            effect_bound: "e", budget: "b", dedup: "d", suppression: "s",
            receipt_kind: "r",
        },
        HookInjectionPointDescriptorV1 {
            id: PostTool, canonical_name: "post_tool",
            host_names: &["PostToolUse"],
            purpose: "p", when_to_use: "w", when_not_to_use: "never untraced",
            input: "i", output: "o", cost_bound: "c", freshness_bound: "f",
            effect_bound: "e", budget: "b", dedup: "d", suppression: "s",
            receipt_kind: "r",
        },
        HookInjectionPointDescriptorV1 {
            id: PreCompaction, canonical_name: "pre_compaction",
            host_names: &["PreCompact"],
            purpose: "p", when_to_use: "w", when_not_to_use: "never speculative",
            input: "i", output: "o", cost_bound: "c", freshness_bound: "f",
            effect_bound: "e", budget: "b", dedup: "d", suppression: "s",
            receipt_kind: "r",
        },
        HookInjectionPointDescriptorV1 {
            id: Resume, canonical_name: "resume",
            host_names: &["SessionStart"],
            purpose: "p", when_to_use: "w", when_not_to_use: "never unknown session",
            input: "i", output: "o", cost_bound: "c", freshness_bound: "f",
            effect_bound: "e", budget: "b", dedup: "d", suppression: "s",
            receipt_kind: "r",
        },
        HookInjectionPointDescriptorV1 {
            id: ExplicitPull, canonical_name: "explicit_pull",
            host_names: &["explicit_client"],
            purpose: "p", when_to_use: "w", when_not_to_use: "never as user_prompt substitute",
            input: "i", output: "o", cost_bound: "c", freshness_bound: "f",
            effect_bound: "e", budget: "b", dedup: "d", suppression: "s",
            receipt_kind: "r",
        },
    ]
}
`;

test("BM09: extracts exactly eight descriptor blocks from well-formed source", () => {
  assert.equal(extractDescriptorBlocks(GOOD_HOOK_SOURCE).length, 8);
});

test("BM09: passes when all eight points carry every required field", () => {
  const result = BM09({ source: GOOD_HOOK_SOURCE });
  assert.equal(result.pass, true);
  assert.equal(result.findings.filter((f) => f.ok).length, 8);
});

test("BM09 negative control Z17: a descriptor lacking when_not_to_use fails", () => {
  const mutated = GOOD_HOOK_SOURCE.replace('when_not_to_use: "never on resume",', 'when_not_to_use: "",');
  const result = BM09({ source: mutated });
  assert.equal(result.pass, false);
  const sessionStart = result.findings.find((f) => f.point === "session_start");
  assert.equal(sessionStart.ok, false);
  assert.ok(sessionStart.missingFields.includes("when_not_to_use"));
});

test("BM09 negative control Z18: a point missing freshness/budget/dedup/suppression/receipt fails", () => {
  const mutated = GOOD_HOOK_SOURCE.replace('freshness_bound: "f",\n            effect_bound: "e", budget: "b", dedup: "d", suppression: "s",\n            receipt_kind: "r",\n        },\n        HookInjectionPointDescriptorV1 {\n            id: UserPrompt', 'freshness_bound: "f",\n            effect_bound: "e", budget: "", dedup: "", suppression: "",\n            receipt_kind: "",\n        },\n        HookInjectionPointDescriptorV1 {\n            id: UserPrompt');
  const result = BM09({ source: mutated });
  assert.equal(result.pass, false);
  const sessionStart = result.findings.find((f) => f.point === "session_start");
  assert.equal(sessionStart.ok, false);
  for (const field of ["budget", "dedup", "suppression", "receipt_kind"]) assert.ok(sessionStart.missingFields.includes(field));
});

test("BM09 negative control: a missing injection point (fewer than eight) fails", () => {
  const truncated = GOOD_HOOK_SOURCE.slice(0, GOOD_HOOK_SOURCE.indexOf("id: ExplicitPull")) + "]\n}\n";
  const result = BM09({ source: truncated });
  assert.equal(result.pass, false);
  assert.ok(result.findings.some((f) => f.point === "explicit_pull" && f.reason === "descriptor_missing"));
});

function withTempCorpus(fn) {
  const dir = mkdtempSync(join(tmpdir(), "cob-"));
  try {
    return fn(dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

const GOOD_TASK = { taskId: "t1", category: "exact_implementation", prompt: "x", repoSnapshot: { commit: "a".repeat(40), paths: ["x"] }, sourceSnapshot: { kind: "blueprint", generation: null }, hostConstraints: { budgetTokens: 100, client: "claude_code" } };
const GOOD_EVIDENCE = { taskId: "t1", requiredEvidenceIds: ["x#y"], falseConfidenceTraps: ["trap"], correctnessCriteria: "must state y", primaryMetric: "correctness", secondaryMetrics: [] };

test("BM11: passes on a well-formed minimal corpus", () => {
  withTempCorpus((dir) => {
    writeFileSync(join(dir, "tasks.jsonl"), `${JSON.stringify(GOOD_TASK)}\n`);
    writeFileSync(join(dir, "expected-evidence.jsonl"), `${JSON.stringify(GOOD_EVIDENCE)}\n`);
    const result = BM11({ corpusDir: dir });
    assert.equal(result.pass, true);
    assert.equal(result.taskCount, 1);
  });
});

test("BM11 negative control Z20: corpus fixture path undefined fails", () => {
  const result = BM11({ corpusDir: join(tmpdir(), "cob-does-not-exist-xyz") });
  assert.equal(result.pass, false);
  assert.equal(result.findings[0].reason, "corpus_path_undefined");
});

test("BM11 negative control: a task without matching expected evidence fails", () => {
  withTempCorpus((dir) => {
    writeFileSync(join(dir, "tasks.jsonl"), `${JSON.stringify(GOOD_TASK)}\n`);
    writeFileSync(join(dir, "expected-evidence.jsonl"), "");
    const result = BM11({ corpusDir: dir });
    assert.equal(result.pass, false);
    assert.ok(result.findings.some((f) => f.reason === "expected_evidence_missing_for_task"));
  });
});

test("BM11 negative control: empty correctnessCriteria fails (no independently authored evidence)", () => {
  withTempCorpus((dir) => {
    writeFileSync(join(dir, "tasks.jsonl"), `${JSON.stringify(GOOD_TASK)}\n`);
    writeFileSync(join(dir, "expected-evidence.jsonl"), `${JSON.stringify({ ...GOOD_EVIDENCE, correctnessCriteria: "" })}\n`);
    const result = BM11({ corpusDir: dir });
    assert.equal(result.pass, false);
    assert.ok(result.findings.some((f) => f.reason === "empty_correctness_criteria"));
  });
});

test("BM11 negative control: invalid primaryMetric (inferred/unsupported causality shape) fails", () => {
  withTempCorpus((dir) => {
    writeFileSync(join(dir, "tasks.jsonl"), `${JSON.stringify(GOOD_TASK)}\n`);
    writeFileSync(join(dir, "expected-evidence.jsonl"), `${JSON.stringify({ ...GOOD_EVIDENCE, primaryMetric: "model_inferred_vibe" })}\n`);
    const result = BM11({ corpusDir: dir });
    assert.equal(result.pass, false);
    assert.ok(result.findings.some((f) => f.reason === "invalid_primary_metric"));
  });
});

test("BM11: the real fixture corpus at its frozen path passes structurally", () => {
  const result = BM11();
  assert.equal(result.pass, true, JSON.stringify(result.findings));
  assert.equal(result.taskCount, 11);
});

test("BM09: the real membrane-protocol hook.rs source passes structurally", () => {
  const result = BM09();
  assert.equal(result.pass, true, JSON.stringify(result.findings));
});

test("BM09: configured installed probe is required and fails closed when binary is absent", () => {
  const root = join(tmpdir(), "membrane-bm09-no-binary");
  const result = BM09({ source: GOOD_HOOK_SOURCE, installedRoot: root });
  assert.equal(result.evidenceKind, "installed");
  assert.equal(result.pass, false);
  assert.equal(probeBM09Installed({ installedRoot: root }).pass, false);
});

test("BM11: configured installed probe is required and fails closed when binary is absent", () => {
  const root = join(tmpdir(), "membrane-bm11-no-binary");
  const result = BM11({ installedRoot: root });
  assert.equal(result.evidenceKind, "installed");
  assert.equal(result.pass, false);
  assert.equal(probeBM11Installed({ installedRoot: root }).pass, false);
});

test("MEM registry coverage: every windows-acceptance mem-windows row has a named export", () => {
  const ids = [
    "MEM_001", "MEM_002", "MEM_003", "MEM_004", "MEM_005", "MEM_007", "MEM_011",
    "MEM_013", "MEM_014", "MEM_015", "MEM_017", "MEM_022", "MEM_023", "MEM_024",
    "MEM_025", "MEM_026", "MEM_027", "MEM_028", "MEM_029", "MEM_030", "MEM_031",
    "MEM_032", "MEM_033", "MEM_034", "MEM_035", "MEM_036", "MEM_037", "MEM_038",
    "MEM_039", "MEM_040", "MEM_041", "MEM_042", "MEM_043", "MEM_045", "MEM_046",
    "MEM_047", "MEM_048", "MEM_049", "MEM_050", "MEM_051",
  ];
  for (const id of ids) assert.equal(typeof CASES[id], "function", `${id} missing`);
  assert.equal(new Set(ids.map((id) => CASES[id])).size, ids.length);
});

function installedFixture() {
  const root = mkdtempSync(join(tmpdir(), "membrane-installed-binding-"));
  const exe = join(root, "membrane.exe");
  writeFileSync(exe, "native-test-binary");
  const digest = createHash("sha256").update("native-test-binary").digest("hex");
  const generation = `sha256:${"a".repeat(64)}`;
  writeFileSync(join(root, "release.json"), JSON.stringify({ releaseGeneration: generation, files: { "membrane.exe": digest } }));
  const spawnSync = (_file, args, options = {}) => {
    if (args.join(" ") === "cli build-info") return { status: 0, stdout: JSON.stringify({ target: "x86_64-pc-windows-msvc", release_generation: generation, membrane_source_commit: "b".repeat(40) }), stderr: "" };
    const requests = String(options.input ?? "");
    if (requests.includes('"tools/call"')) return { status: 0, stdout: JSON.stringify({ id: 1, result: { serverInfo: { name: "membrane" }, protocolVersion: "2025-03-26" } }) + "\n" + JSON.stringify({ id: 2, result: { isError: true, structuredContent: { result: { code: "repository_scope_chain_denied" } } } }), stderr: "" };
    const lines = [
      { id: 1, result: { serverInfo: { name: "membrane" }, protocolVersion: "2025-03-26" } },
      { id: 2, result: { tools: [{ name: "membrane_context", description: "Federate bounded context through planner", inputSchema: { required: ["caller", "repository"] } }] } },
      { id: 3, result: { resources: [{ name: "resources-index" }, { name: "installation-manifest" }, { name: "lease-status" }, { name: "operation-registry" }] } },
      { id: 4, result: { prompts: [{ name: "recap" }, { name: "plan" }, { name: "summarize" }, { name: "checkpoint" }] } },
    ];
    return { status: 0, stdout: lines.map((line) => JSON.stringify(line)).join("\n"), stderr: "" };
  };
  return { root, exe, spawnSync, generation, digest };
}

test("MEM installed binding requires release manifest/hash/generation/target agreement", () => {
  const fixture = installedFixture();
  try {
    const binding = installedBinding({ installedRoot: fixture.root, allowNonCanonicalRoot: true, spawnSync: fixture.spawnSync });
    assert.equal(binding.ok, true, JSON.stringify(binding));
    writeFileSync(join(fixture.root, "release.json"), JSON.stringify({ releaseGeneration: fixture.generation, files: { "membrane.exe": "0".repeat(64) } }));
    assert.equal(installedBinding({ installedRoot: fixture.root, allowNonCanonicalRoot: true, spawnSync: fixture.spawnSync }).ok, false);
  } finally { rmSync(fixture.root, { recursive: true, force: true }); }
});

test("MEM installed binding rejects non-canonical checkout paths", () => {
  const fixture = installedFixture();
  try {
    const binding = installedBinding({ installedRoot: fixture.root, spawnSync: fixture.spawnSync });
    assert.equal(binding.ok, false);
    assert.equal(binding.reason, "installed_root_not_canonical_current");
  } finally { rmSync(fixture.root, { recursive: true, force: true }); }
});

test("MEM installed binding rejects a build from a different source revision", () => {
  const fixture = installedFixture();
  try {
    const binding = installedBinding({ installedRoot: fixture.root, allowNonCanonicalRoot: true, row: { qualifiedSourceRevision: "c".repeat(40) }, spawnSync: fixture.spawnSync });
    assert.equal(binding.ok, false);
    assert.equal(binding.checks.sourceMatches, false);
  } finally { rmSync(fixture.root, { recursive: true, force: true }); }
});

test("MEM installed row probe binds native MCP discovery to one installed identity", () => {
  const fixture = installedFixture();
  try {
    const result = probeInstalledMembraneRow("MEM_013", { installedRoot: fixture.root, allowNonCanonicalRoot: true, spawnSync: fixture.spawnSync });
    assert.equal(result.pass, true, JSON.stringify(result));
    assert.equal(result.evidenceKind, "installed");
    assert.equal(result.binding.checks.hashMatches, true);
  } finally { rmSync(fixture.root, { recursive: true, force: true }); }
});

test("MEM-004 installed probe requires typed denial for an unenrolled caller", () => {
  const fixture = installedFixture();
  try {
    const result = probeInstalledMembraneRow("MEM_004", { installedRoot: fixture.root, allowNonCanonicalRoot: true, workspaceRoot: fixture.root, spawnSync: fixture.spawnSync });
    assert.equal(result.pass, true, JSON.stringify(result));
    assert.equal(result.findings[0].code, "repository_scope_chain_denied");
  } finally { rmSync(fixture.root, { recursive: true, force: true }); }
});
