import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { readEnvelope, mutateManifest } from "./_store-helpers.mjs";
import { closeStore, openStore } from "../src/graph/store-sqlite.mjs";
import { markDomainPending, readPendingDomains } from "../src/graph/delta-store.mjs";
import { PARSED_LANGUAGE_EXTENSIONS } from "../src/graph/language-extractors.mjs";
import { languageCapabilityRecords } from "../src/graph/language-registry.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const BLUEPRINT = path.resolve(HERE, "..");
const CLI = path.join(BLUEPRINT, "scripts/blueprint.mjs");
const FIXTURE = path.join(BLUEPRINT, "evals/fixture-repos/typescript-commerce");
const EXPECTED_PARSED_EXTENSIONS = [...new Set([
  ...PARSED_LANGUAGE_EXTENSIONS,
  ...languageCapabilityRecords().flatMap((record) => record.extensions),
])].sort();

function copyFixture() {
  const dir = path.join(os.tmpdir(), `blueprint-graph-cli-${process.pid}-${Date.now()}`);
  fs.cpSync(FIXTURE, dir, { recursive: true });
  return dir;
}

function run(args, cwd) {
  return spawnSync(process.execPath, [CLI, ...args], { cwd, encoding: "utf8" });
}

test("blueprint graph build/status/search works from a repo root", () => {
  const repo = copyFixture();
  try {
    fs.writeFileSync(path.join(repo, "opaque.png"), Buffer.from([0, 1, 2, 3, 255]));
    const build = run(["graph", "build", "--out", ".agent"], repo);
    assert.equal(build.status, 0, build.stderr);
    assert.match(build.stdout, /graph built .agent\/graph\/graph\.db/);
    assert.ok(fs.existsSync(path.join(repo, ".agent/graph/graph.db")));
    assert.ok(!fs.existsSync(path.join(repo, ".agent/graph/graph.json")), "graph.json must not be written");
    assert.ok(!fs.existsSync(path.join(repo, ".agent/graph/manifest.json")), "manifest.json must not be written");

    const status = run(["graph", "status", "--out", ".agent"], repo);
    assert.equal(status.status, 0, status.stderr);
    assert.match(status.stdout, /graph fresh provider=blueprint-static/);

    const search = run(["graph", "search", "--query", "placeOrder", "--out", ".agent", "--limit", "3"], repo);
    assert.equal(search.status, 0, search.stderr);
    const payload = JSON.parse(search.stdout);
    assert.equal(payload.results[0].evidence[0].path, "src/service.ts");

    const fullBuild = run(["build", "--out", ".agent"], repo);
    assert.equal(fullBuild.status, 0, fullBuild.stderr || fullBuild.stdout);
    const doctor = run(["doctor", "--out", ".agent"], repo);
    assert.equal(doctor.status, 0, doctor.stderr || doctor.stdout);
    const doctorPayload = JSON.parse(doctor.stdout);
    assert.equal(doctorPayload.state, "ready");
    assert.ok(!doctorPayload.reasons.some((reason) => reason.code === "ledger_root_mismatch"));
    assert.deepEqual(doctorPayload.capabilities.languageCoverage.parsedExtensions, EXPECTED_PARSED_EXTENSIONS);
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("graph audit-projection emits compact current working-tree discovery", () => {
  const repo = copyFixture();
  try {
    const git = (...args) => spawnSync("git", args, { cwd: repo, encoding: "utf8" });
    assert.equal(git("init").status, 0);
    assert.equal(git("config", "user.email", "blueprint@example.invalid").status, 0);
    assert.equal(git("config", "user.name", "Blueprint Test").status, 0);
    assert.equal(git("add", ".").status, 0);
    assert.equal(git("commit", "-m", "fixture").status, 0);
    fs.appendFileSync(path.join(repo, "src/service.ts"), "\nexport const dirty = true;\n");
    fs.writeFileSync(path.join(repo, "src/untracked.ts"), "export const untracked = true;\n");
    fs.rmSync(path.join(repo, "resources/orders.json"));

    const build = run(["graph", "build", "--out", ".agent"], repo);
    assert.equal(build.status, 0, build.stderr || build.stdout);
    const projectionResult = run(["graph", "audit-projection", "--out", ".agent", "--json"], repo);
    assert.equal(projectionResult.status, 0, projectionResult.stderr || projectionResult.stdout);
    const projection = JSON.parse(projectionResult.stdout);
    assert.equal(projection.schema, "membrane.blueprint-packet.v1");
    assert.equal(projection.status, "ready");
    assert.equal(projection.state, "ready");
    assert.match(projection.generationId, /^xxh128:/);
    assert.match(projection.manifestDigest, /^sha256:/);
    assert.equal(projection.sourceObservation.dirty, true);
    assert.ok(projection.files.includes("src/service.ts"));
    assert.ok(projection.files.includes("src/untracked.ts"));
    assert.ok(!projection.files.includes("resources/orders.json"));
    assert.deepEqual(projection.files, [...projection.files].sort());
    assert.equal(projection.fileCount, projection.files.length);
    assert.ok(projection.sourceFileCount >= 5);
    assert.ok(projection.parsedExtensions.includes("ts"));
    assert.equal(projection.overlay.state, "ready");
    assert.ok(projection.overlay.dirtyTracked >= 1);
    assert.equal(projection.overlay.untracked, 1);
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("doctor degrades for unsupported source while catalog coverage remains ready", () => {
  const repo = copyFixture();
  try {
    fs.writeFileSync(path.join(repo, "src/worker.cr"), "def perform\n  true\nend\n");
    const build = run(["build", "--out", ".agent"], repo);
    assert.equal(build.status, 0, build.stderr || build.stdout);
    const doctor = run(["doctor", "--json", "--out", ".agent"], repo);
    assert.equal(doctor.status, 0, doctor.stderr || doctor.stdout);
    const payload = JSON.parse(doctor.stdout);
    assert.equal(payload.state, "degraded");
    assert.deepEqual(payload.graphCapabilities.unsupportedExtensions, ["cr"]);
    assert.ok(payload.reasons.some((reason) => reason.code === "unsupported_languages"));
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("blueprint graph status exits distinctly when no generation exists", () => {
  const repo = copyFixture();
  try {
    const status = run(["graph", "status", "--out", ".agent"], repo);
    assert.equal(status.status, 2);
    assert.match(status.stdout, /graph missing/);

    const doctor = run(["doctor", "--out", ".agent"], repo);
    assert.equal(doctor.status, 2);
    assert.equal(JSON.parse(doctor.stdout).state, "missing");
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("explicit graph root query rejects an unenrolled normalized root", () => {
  const repo = copyFixture();
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "blueprint-graph-cli-home-"));
  try {
    const result = spawnSync(process.execPath, [CLI, "graph", "status", "--root", repo, "--out", ".agent"], {
      cwd: repo,
      encoding: "utf8",
      env: { ...process.env, HOME: home, USERPROFILE: home, HOMEDRIVE: path.parse(home).root, HOMEPATH: path.relative(path.parse(home).root, home) },
    });
    assert.equal(result.status, 1, result.stderr || result.stdout);
    const payload = JSON.parse(result.stderr.trim());
    assert.equal(payload.error.code, "root_not_enrolled");
    assert.equal(payload.error.normalizedRoot, path.resolve(repo));
    assert.equal(payload.error.remediation.nextOperation, `blueprint init --root ${JSON.stringify(path.resolve(repo))}`);
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test("doctor reports 'corrupt' for a present-but-unparseable map (distinct from missing)", () => {
  const repo = copyFixture();
  try {
    assert.equal(run(["build", "--out", ".agent"], repo).status, 0);
    fs.writeFileSync(path.join(repo, ".agent/map.json"), "{ not valid json");
    const doctor = run(["doctor", "--out", ".agent"], repo);
    const payload = JSON.parse(doctor.stdout);
    assert.equal(payload.state, "corrupt");
    assert.ok(payload.reasons.some((r) => r.code === "corrupt_map"));
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("doctor reports 'broken' when the graph provider version is incompatible", () => {
  const repo = copyFixture();
  try {
    assert.equal(run(["build", "--out", ".agent"], repo).status, 0);
    mutateManifest(repo, (manifest) => { manifest.provider.version = "repo-local-deterministic-v1"; });
    const doctor = run(["doctor", "--json", "--out", ".agent"], repo);
    const payload = JSON.parse(doctor.stdout);
    assert.equal(payload.state, "broken");
    assert.ok(payload.reasons.some((r) => r.providerMismatch === true));
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("doctor --full rejects Phase 1 alone and accepts current complete understanding", () => {
  const repo = copyFixture();
  try {
    const goDir = path.join(repo, "go-src");
    fs.mkdirSync(goDir, { recursive: true });
    fs.writeFileSync(path.join(goDir, "thing.go"), "package thing\n\nfunc hello() string { return \"world\" }\n");
    assert.equal(run(["build", "--out", ".agent"], repo).status, 0);

    const phase1Only = run(["doctor", "--full", "--json", "--out", ".agent"], repo);
    assert.equal(phase1Only.status, 2, phase1Only.stderr || phase1Only.stdout);
    const incomplete = JSON.parse(phase1Only.stdout);
    assert.equal(incomplete.completion.state, "incomplete");
    assert.ok(incomplete.reasons.some((reason) => reason.code === "missing_understanding"));

    const coldPlanResult = run(["phase2", "plan", "--json", "--out", ".agent"], repo);
    assert.equal(coldPlanResult.status, 0, coldPlanResult.stderr || coldPlanResult.stdout);
    const coldPlan = JSON.parse(coldPlanResult.stdout);
    assert.equal(coldPlan.complete, false);
    assert.deepEqual(
      coldPlan.dimensions.synthesize.map((item) => item.dimension),
      ["architecture", "interfaces", "health", "contract", "security", "solid"],
    );
    assert.ok(fs.existsSync(path.join(repo, ".agent/phase2-plan.json")));

    const graphManifest = readEnvelope(repo);
    fs.writeFileSync(
      path.join(repo, ".agent/understanding.json"),
      JSON.stringify({
        sourceGenerationId: graphManifest.generationId,
        architecture: {
          summary: "Fixture commerce flow.",
          stack: [],
          components: [
            { name: "API", evidence: "src/service.ts:1" },
            { name: "Store", evidence: "src/service.ts:10" },
          ],
          dataFlow: ["Request -> API -> Store -> Response"],
          entryPoints: [],
          stateStores: [],
          externalDeps: [],
          deployableUnits: [],
          externalServices: [],
          infrastructure: [],
          crossCutting: [],
          flows: [],
          coverageGaps: [],
          capabilityCoverage: [],
        },
        interfaces: {
          publicApi: [], moduleInterfaces: [], dataContracts: [], configKeys: [],
          extensionPoints: [], fragileContracts: [],
        },
        health: {
          oversized: [], slop: [], hotspots: [], duplication: [], coupling: [],
          untested: [], deadWeight: [], top10: [],
        },
        contract: {
          invariants: [], constraints: [], assumptions: [], entropy: [], decisions: [],
        },
        security: {
          trustBoundaries: [], secrets: [], injectionSurface: [], authz: [],
          dataProtection: [], dangerousPatterns: [], posture: [],
        },
        solid: { dimensions: [], scorecard: [], top5: [] },
        incremental: {
          dimensions: Object.fromEntries(
            ["architecture", "interfaces", "health", "contract", "security", "solid"].map((dimension) => [
              dimension,
              { inputFiles: ["src/service.ts"], inputVerdictIds: [] },
            ]),
          ),
        },
      }, null, 2),
    );
    fs.writeFileSync(
      path.join(repo, ".agent/verdicts.json"),
      JSON.stringify({ sourceGenerationId: "xxh128:stale-generation", verdicts: [] }, null, 2),
    );
    assert.equal(run(["build", "--out", ".agent"], repo).status, 0);

    const staleVerdicts = run(["doctor", "--full", "--json", "--out", ".agent"], repo);
    assert.equal(staleVerdicts.status, 2, staleVerdicts.stderr || staleVerdicts.stdout);
    assert.ok(JSON.parse(staleVerdicts.stdout).reasons.some((reason) => reason.code === "stale_verdicts"));

    const pendingDb = openStore(path.join(repo, ".agent/graph/graph.db"));
    try { markDomainPending(pendingDb, "doc"); } finally { closeStore(pendingDb); }
    const understandingPath = path.join(repo, ".agent/understanding.json");
    const validUnderstanding = fs.readFileSync(understandingPath, "utf8");
    const invalidUnderstanding = JSON.parse(validUnderstanding);
    delete invalidUnderstanding.architecture;
    fs.writeFileSync(understandingPath, JSON.stringify(invalidUnderstanding, null, 2));
    const failedSeal = run(["phase2", "seal", "--json", "--out", ".agent"], repo);
    assert.notEqual(failedSeal.status, 0);
    const retainedDb = openStore(path.join(repo, ".agent/graph/graph.db"));
    try { assert.deepEqual(readPendingDomains(retainedDb), ["doc"]); } finally { closeStore(retainedDb); }
    fs.writeFileSync(understandingPath, validUnderstanding);

    const sealResult = run(["phase2", "seal", "--json", "--out", ".agent"], repo);
    assert.equal(sealResult.status, 0, sealResult.stderr || sealResult.stdout);
    const sealed = JSON.parse(sealResult.stdout);
    assert.equal(sealed.state, "sealed");
    assert.equal(sealed.sourceGenerationId, graphManifest.generationId);
    const clearedDb = openStore(path.join(repo, ".agent/graph/graph.db"));
    try { assert.deepEqual(readPendingDomains(clearedDb), []); } finally { closeStore(clearedDb); }

    const full = run(["doctor", "--full", "--json", "--out", ".agent"], repo);
    assert.equal(full.status, 0, full.stderr || full.stdout);
    const complete = JSON.parse(full.stdout);
    assert.equal(complete.state, "ready");
    assert.equal(complete.completion.state, "complete");
    assert.deepEqual(complete.completion.phases, {
      phase1: "complete",
      phase2: "complete",
      phase3: "complete",
      phase4: "not_required",
    });

    const architecturePath = [
      path.join(repo, "docs/architecture.md"),
      path.join(repo, ".agent/docs/architecture.md"),
    ].find((candidate) =>
      fs.existsSync(candidate)
      && /<!-- generated by blueprint .* gen:[a-f0-9]{32} /.test(fs.readFileSync(candidate, "utf8")),
    );
    assert.ok(architecturePath, "no generated architecture doc carried a portable identity");
    const currentArchitecture = fs.readFileSync(architecturePath, "utf8");
    const currentDocGeneration = currentArchitecture.match(/gen:([a-f0-9]{32})/)?.[1];
    assert.ok(currentDocGeneration, "current generated architecture doc has no portable identity");
    fs.writeFileSync(
      architecturePath,
      currentArchitecture.replace(`gen:${currentDocGeneration}`, "gen:stale-generation"),
    );
    const staleDocs = run(["doctor", "--full", "--json", "--out", ".agent"], repo);
    assert.equal(staleDocs.status, 2, staleDocs.stderr || staleDocs.stdout);
    assert.ok(JSON.parse(staleDocs.stdout).reasons.some((reason) => reason.code === "stale_human_docs"));

    fs.writeFileSync(architecturePath, currentArchitecture);
    assert.equal(run(["doctor", "--full", "--json", "--out", ".agent"], repo).status, 0);

    fs.appendFileSync(path.join(repo, "go-src/thing.go"), "\n// unrelated implementation detail\n");
    assert.equal(run(["build", "--out", ".agent"], repo).status, 0);
    const incrementalPlanResult = run(["phase2", "plan", "--json", "--out", ".agent"], repo);
    assert.equal(incrementalPlanResult.status, 0, incrementalPlanResult.stderr || incrementalPlanResult.stdout);
    const incrementalPlan = JSON.parse(incrementalPlanResult.stdout);
    assert.equal(incrementalPlan.complete, true);
    assert.deepEqual(incrementalPlan.verdicts.verify, []);
    assert.deepEqual(
      incrementalPlan.dimensions.reuse,
      ["architecture", "interfaces", "health", "contract", "security", "solid"],
    );
    assert.equal(run(["phase2", "seal", "--json", "--out", ".agent"], repo).status, 0);
    assert.equal(run(["doctor", "--full", "--json", "--out", ".agent"], repo).status, 0);

    fs.appendFileSync(path.join(repo, "src/service.ts"), "\nexport const changedEvidence = true;\n");
    assert.equal(run(["build", "--out", ".agent"], repo).status, 0);
    const affectedPlan = JSON.parse(run(["phase2", "plan", "--json", "--out", ".agent"], repo).stdout);
    assert.equal(affectedPlan.complete, false);
    assert.ok(affectedPlan.dimensions.synthesize.length > 0);
    const affectedDoctor = run(["doctor", "--full", "--json", "--out", ".agent"], repo);
    assert.equal(affectedDoctor.status, 2, affectedDoctor.stderr || affectedDoctor.stdout);
    assert.ok(JSON.parse(affectedDoctor.stdout).reasons.some((reason) => reason.code === "incremental_phase2_pending"));
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("Phase 1 and phase2 plan recover from corrupt semantic cache artifacts", () => {
  const repo = copyFixture();
  try {
    assert.equal(run(["build", "--out", ".agent"], repo).status, 0);
    fs.writeFileSync(path.join(repo, ".agent/verdicts.json"), "{broken");
    fs.writeFileSync(path.join(repo, ".agent/understanding.json"), "{broken");
    const rebuild = run(["build", "--out", ".agent"], repo);
    assert.equal(rebuild.status, 0, rebuild.stderr || rebuild.stdout);
    const planResult = run(["phase2", "plan", "--json", "--out", ".agent"], repo);
    assert.equal(planResult.status, 0, planResult.stderr || planResult.stdout);
    const plan = JSON.parse(planResult.stdout);
    assert.equal(plan.complete, false);
    assert.deepEqual(plan.dimensions.reuse, []);
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});
