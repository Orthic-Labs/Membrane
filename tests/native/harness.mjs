#!/usr/bin/env node
// O5 native lifecycle matrix producer — self-checkable harness.
//
// Owns `tests/native/**` (OR-NATIVE-HARNESS). Produces the qualification
// matrix and evidence schema that N-MAC / N-WIN execute later from identical
// source/contract/add-on digests. This file performs the harness self-check
// and can emit a per-platform runner skeleton; it never runs a native install
// and never claims a native receipt (that is N-MAC / N-WIN, phase N).
//
// The self-check does not merely count case IDs: it validates every command's
// entry point, rejects fabricated Hub CLI flags, rejects the nonexistent
// `right-release rollback` subcommand, rejects a nonexistent mac `uninstall.sh`,
// requires each assert to name a typed outcome/census/rejection, requires
// supervised cases to LAUNCH THE HUB (not run the fixture child directly),
// validates each `setup` stage, validates `fixtureScenario` against the
// shipped fixture's actual scenario set, forbids a `wrong-digest` fixture
// scenario (digest is manifest-level), and behaviourally proves the fixture
// exits 0 with zero bytes when run without a Hub (headless carve-out).
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const here = dirname(fileURLToPath(import.meta.url));

const REQUIRED_CASE_IDS = [
  "first-run", "selection-cortex-only", "selection-both", "membrane-implies-cortex",
  "headless-carve-out", "ready", "degraded", "incompatible", "off", "quit",
  "child-crash", "restart", "fence-loss", "update-handoff", "rollback", "uninstall",
  "zero-orphan-census", "stale-fence", "old-child", "wrong-digest",
  "incompatible-range", "rogue-endpoint",
];
const CLOSED_PHASES = ["first-run", "selection", "headless", "lifecycle", "crash", "update", "uninstall", "adversarial"];
const ADMITTED_PLATFORMS = ["mac", "win"];
const HUB_LAUNCHERS = new Set(["open", "powershell"]);
const ALLOWED_FIRST_TOKENS = new Set([
  "node", "open", "osascript", "kill", "pgrep", "pkill", "rm", "cp", "hdiutil",
  "installer", "powershell",
]);
const FORBIDDEN_HUB_FLAGS = new Set(["--select", "--handoff"]);
const FORBIDDEN_SUBCOMMAND_SEQ = ["right-release", "rollback"];
const FORBIDDEN_MAC_UNINSTALL = /Orthic\.app\/[^ ]*uninstall\.sh/;
const TYPED_OUTCOME = /\b(ready|degraded|incompatible|unavailable|crash_loop|stale_fence|endpoint_not_loopback|artifact_digest_mismatch|census|live|descendants|orphan|fence|owner|drain|stop|installed bytes|version supervised|non-deselectable)\b/i;

// Scenarios the shipped lifecycle-fixture.mjs must implement. Mirrored from
// the fixture so drift between matrix and fixture fails self-check.
const REQUIRED_FIXTURE_SCENARIOS = [
  "normal", "degraded", "incompatible-range", "stale-fence",
  "old-instance", "rogue-endpoint", "update-handoff",
];

// Behavioural expectations: for each scenario the fixture-child must, when fed
// a Hub `hello` frame on stdin, emit a `register` (and for update-handoff an
// `ack`) frame on stdout whose state/fence/endpoint/capability match the
// scenario contract. This is a *behavioural* check — it executes every
// scenario and verifies the emitted state — not a regex presence test, so a
// broken implementation (e.g. `degraded` emitting `state:"ready"`) fails.
const FIXTURE_HELLO = {
  kind: "hello", lifecycleVersion: 1, installationId: "i",
  productId: "cortex", instanceId: "i:1", fence: 7,
  artifactDigest: "sha256:" + "a".repeat(64),
  declaredDataRoot: "/x", secret: "0".repeat(64),
};
const FIXTURE_SCENARIO_EXPECT = {
  "normal": { frames: [{ kind: "register", state: "ready", fence: 7, endpoint: { host: "127.0.0.1", port: 9 }, capability: "cap" }] },
  "degraded": { frames: [{ kind: "register", state: "degraded", fence: 7, capability: "cap-degraded" }] },
  "incompatible-range": { frames: [{ kind: "register", state: "incompatible", fence: 7 }] },
  "stale-fence": { frames: [{ kind: "register", state: "ready", fence: 6 }] },
  "old-instance": { frames: [
    { kind: "register", state: "ready", fence: 7, capability: "cap" },
    { kind: "register", state: "ready", fence: 6, capability: "cap-old" },
  ] },
  "rogue-endpoint": { frames: [{ kind: "register", state: "ready", endpoint: { host: "10.0.0.1", port: 9 } }] },
  "update-handoff": { frames: [
    { kind: "register", state: "ready", fence: 7, capability: "cap" },
    { kind: "ack", command: "update_handoff", fence: 7 },
  ], extraIn: [{ kind: "command", command: "update_handoff", fence: 7 }] },
};

// Behaviourally drive the fixture-child for every required scenario and verify
// the emitted frames match the contract. Returns an array of error strings.
function behavioralFixtureScenarioErrors(fixtureChild) {
  const errs = [];
  // First confirm the fixture declares exactly the required scenario set via
  // its `--scenario` guard: an unknown scenario must exit non-zero with usage.
  const unknown = spawnSync(process.execPath, [fixtureChild, "--scenario", "this-scenario-does-not-exist"], { input: "", timeout: 2000 });
  if (unknown.status === 0 || unknown.status === null) errs.push(`fixture-child accepted an unknown scenario (exit ${unknown.status}); --scenario guard missing`);
  for (const sc of REQUIRED_FIXTURE_SCENARIOS) {
    const expect = FIXTURE_SCENARIO_EXPECT[sc];
    const inLines = [JSON.stringify(FIXTURE_HELLO), ...(expect.extraIn ?? []).map(JSON.stringify)];
    const res = spawnSync(process.execPath, [fixtureChild, "--scenario", sc], { input: inLines.join("\n") + "\n", timeout: 3000, encoding: "utf8" });
    if (res.status !== 0) { errs.push(`scenario '${sc}' exited ${res.status} (stderr: ${String(res.stderr).trim()})`); continue; }
    const outLines = (res.stdout ?? "").split("\n").map((l) => l.trim()).filter((l) => l.length > 0);
    if (outLines.length !== expect.frames.length) { errs.push(`scenario '${sc}' emitted ${outLines.length} frame(s), expected ${expect.frames.length}: ${JSON.stringify(outLines)}`); continue; }
    for (let i = 0; i < expect.frames.length; i++) {
      let frame;
      try { frame = JSON.parse(outLines[i]); } catch { errs.push(`scenario '${sc}' frame ${i} is not JSON: ${outLines[i]}`); continue; }
      const exp = expect.frames[i];
      if (frame.kind !== exp.kind) { errs.push(`scenario '${sc}' frame ${i} kind ${frame.kind} != ${exp.kind}`); continue; }
      if (frame.state !== exp.state) { errs.push(`scenario '${sc}' frame ${i} state ${frame.state} != ${exp.state}`); continue; }
      if (exp.fence !== undefined && frame.fence !== exp.fence) { errs.push(`scenario '${sc}' frame ${i} fence ${frame.fence} != ${exp.fence}`); }
      if (exp.command !== undefined && frame.command !== exp.command) { errs.push(`scenario '${sc}' frame ${i} command ${frame.command} != ${exp.command}`); }
      if (exp.capability !== undefined && frame.capability !== exp.capability) { errs.push(`scenario '${sc}' frame ${i} capability ${frame.capability} != ${exp.capability}`); }
      if (exp.endpoint) {
        const okHost = !exp.endpoint.host || frame.endpoint?.host === exp.endpoint.host;
        if (!okHost) errs.push(`scenario '${sc}' frame ${i} endpoint.host ${frame.endpoint?.host} != ${exp.endpoint.host}`);
        if (exp.endpoint.host === "10.0.0.1" && !(frame.endpoint && typeof frame.endpoint.host === "string" && !/^(127\.0\.0\.1|::1|localhost)$/.test(frame.endpoint.host))) {
          errs.push(`scenario '${sc}' frame ${i} endpoint must be non-loopback, got ${frame.endpoint?.host}`);
        }
        if (exp.endpoint.host === "127.0.0.1" && !(frame.endpoint && /^(127\.0\.0\.1|::1|localhost)$/.test(frame.endpoint.host))) {
          errs.push(`scenario '${sc}' frame ${i} endpoint must be loopback, got ${frame.endpoint?.host}`);
        }
      }
      if (sc === "incompatible-range" && frame.endpoint !== undefined) errs.push(`scenario '${sc}' frame ${i} must carry NO endpoint (incompatible)`);
    }
  }
  return errs;
}

function loadJson(rel) {
  return JSON.parse(readFileSync(resolve(here, rel), "utf8"));
}

// A placeholder like `<staged-fixture-manifest:wrong-digest>` is admitted if its
// declared base `<staged-fixture-manifest>` exists; the `:variant` suffix names
// a per-case staged manifest variant.
function isDeclaredPlaceholder(tok, declared) {
  if (declared.has(tok)) return true;
  const inner = /^<([^>]+)>$/.exec(tok)?.[1] ?? "";
  if (!inner.includes(":")) return false;
  return declared.has(`<${inner.slice(0, inner.indexOf(":"))}>`);
}

function checkCommand(command, platform, caseId, where, declared) {
  const errs = [];
  const cmd = command ?? [];
  if (cmd.length === 0) return [`case ${caseId}: ${where} empty command`];
  if (cmd.some((tok) => FORBIDDEN_HUB_FLAGS.has(tok))) errs.push(`case ${caseId}: ${where} fabricated Hub flag not accepted by Orthic (${cmd.join(" ")})`);
  for (let i = 0; i < cmd.length - 1; i++) {
    if (cmd[i] === FORBIDDEN_SUBCOMMAND_SEQ[0] && cmd[i + 1] === FORBIDDEN_SUBCOMMAND_SEQ[1]) errs.push(`case ${caseId}: ${where} nonexistent right-release rollback subcommand (${cmd.join(" ")})`);
  }
  if (platform === "mac" && cmd.some((tok) => FORBIDDEN_MAC_UNINSTALL.test(String(tok)))) errs.push(`case ${caseId}: ${where} references nonexistent mac uninstall.sh (${cmd.join(" ")})`);
  const first = String(cmd[0] ?? "");
  if (!ALLOWED_FIRST_TOKENS.has(first) && !/^<[^>]+>$/.test(first)) errs.push(`case ${caseId}: ${where} command[0] '${first}' is not a real entrypoint or declared placeholder`);
  for (const tok of cmd) {
    for (const match of String(tok).matchAll(/<[^>]+>/g)) {
      if (!isDeclaredPlaceholder(match[0], declared)) errs.push(`case ${caseId}: ${where} undeclared placeholder ${match[0]} in '${tok}'`);
    }
  }
  return errs;
}

function selfCheck() {
  const matrix = loadJson("matrix.json");
  const receipt = loadJson("receipt.schema.json");
  const errors = [];

  if (matrix.schemaVersion !== 1) errors.push("matrix.schemaVersion must be 1");
  if (!Array.isArray(matrix.cases) || matrix.cases.length === 0) errors.push("matrix.cases must be non-empty");
  const declaredPlaceholders = new Set((matrix.placeholders ?? []).map((p) => p.token));
  if (declaredPlaceholders.size === 0) errors.push("matrix must declare runtime placeholders");
  for (const p of matrix.placeholders ?? []) {
    if (!p.token || !/^<[^>]+>$/.test(p.token) || !p.description) errors.push(`placeholder ${p.token ?? "?"} malformed`);
  }

  const byId = new Map((matrix.cases ?? []).map((c) => [c.id, c]));
  for (const id of REQUIRED_CASE_IDS) if (!byId.has(id)) errors.push(`missing required case: ${id}`);
  for (const id of byId.keys()) if (!REQUIRED_CASE_IDS.includes(id)) errors.push(`unlisted case id (assign owner before adding): ${id}`);

  const fixtureScenarios = new Set(REQUIRED_FIXTURE_SCENARIOS);
  for (const c of matrix.cases ?? []) {
    if (!c.id || !c.summary) errors.push(`case ${c.id ?? "<no id>"} missing id/summary`);
    if (!CLOSED_PHASES.includes(c.phase)) errors.push(`case ${c.id}: unknown phase ${c.phase}`);
    if (!c.obligation || !c.obligation.startsWith("O")) errors.push(`case ${c.id}: missing obligation`);
    if (!Array.isArray(c.platforms) || c.platforms.length === 0) errors.push(`case ${c.id}: no platforms`);
    for (const p of c.platforms ?? []) if (!ADMITTED_PLATFORMS.includes(p)) errors.push(`case ${c.id}: unknown platform ${p}`);
    if (c.fixtureScenario !== undefined && !fixtureScenarios.has(c.fixtureScenario)) errors.push(`case ${c.id}: fixtureScenario '${c.fixtureScenario}' not implemented by lifecycle-fixture.mjs`);
    if (c.id === "wrong-digest" && c.fixtureScenario !== undefined) errors.push(`case ${c.id}: wrong-digest is manifest-level; must not reference a fixture scenario`);
    if (!Array.isArray(c.steps) || c.steps.length === 0) errors.push(`case ${c.id}: no steps`);
    for (const setup of c.setup ?? []) {
      for (const p of ADMITTED_PLATFORMS) if (setup[p] !== undefined) errors.push(...checkCommand(setup[p], p, c.id, `setup(${p})`, declaredPlaceholders));
    }
    for (const s of c.steps ?? []) {
      if (!ADMITTED_PLATFORMS.includes(s.platform)) errors.push(`case ${c.id}: step unknown platform ${s.platform}`);
      if (!Array.isArray(s.command) || s.command.length === 0) errors.push(`case ${c.id}: step missing command`);
      if (!s.assert) errors.push(`case ${c.id}: step missing assert`);
      if (s.assert && !TYPED_OUTCOME.test(s.assert)) errors.push(`case ${c.id}: assert must name a typed outcome/census/rejection (${s.assert})`);
      errors.push(...checkCommand(s.command, s.platform, c.id, `step(${s.platform})`, declaredPlaceholders));
      // A fixture-driven case (stages a fixture manifest or declares a
      // fixtureScenario) must LAUNCH THE HUB to observe supervision; it must not
      // run the fixture child directly. headless-carve-out runs the fixture
      // directly on purpose (it proves the child never self-registers). Control
      // cases (off/quit/fence-loss/crash/restart/rollback/uninstall/census) have
      // no fixtureScenario and no staged manifest; they act on the running Hub.
      const first = String(s.command?.[0] ?? "");
      const stagesFixture = (c.setup ?? []).some((su) => su[s.platform] !== undefined && String(JSON.stringify(su[s.platform])).includes("<staged-fixture-manifest"));
      const fixtureDriven = c.fixtureScenario !== undefined || stagesFixture;
      if (fixtureDriven && c.id !== "headless-carve-out" && !HUB_LAUNCHERS.has(first)) errors.push(`case ${c.id}: fixture-driven ${c.phase} case must launch the Hub (open/powershell); '${first}' runs the fixture directly`);
    }
    const stepPlatforms = new Set((c.steps ?? []).map((s) => s.platform));
    for (const p of c.platforms ?? []) if (!stepPlatforms.has(p)) errors.push(`case ${c.id}: declares ${p} but has no ${p} step`);
  }

  // The shipped fixture-child must exist, parse, implement every required
  // scenario, and NOT implement wrong-digest (digest is manifest-level).
  const fixtureChild = resolve(here, "lifecycle-fixture.mjs");
  if (!existsSync(fixtureChild)) errors.push("<fixture-child> placeholder resolves to missing tests/native/lifecycle-fixture.mjs");
  else {
    const check = spawnSync(process.execPath, ["--check", fixtureChild], { stdio: "pipe" });
    if (check.status !== 0) errors.push(`<fixture-child> (lifecycle-fixture.mjs) fails node --check: ${String(check.stderr)}`);
    const src = readFileSync(fixtureChild, "utf8");
    if (/"wrong-digest"/.test(src)) errors.push("fixture-child must NOT implement wrong-digest (digest is manifest-level, checked before spawn)");
    // BEHAVIOURAL scenario proof: drive every required scenario through the
    // fixture-child and verify the emitted register/ack frames match the
    // scenario contract. Replaces a previous regex presence check that passed a
    // broken `degraded` implementation emitting `state:"ready"`.
    errors.push(...behavioralFixtureScenarioErrors(fixtureChild));
  }
  const fixtureManifest = resolve(here, "fixtures/fixture-product-manifest.template.json");
  if (!existsSync(fixtureManifest)) errors.push("missing fixture-product-manifest.template.json");
  else {
    const fm = JSON.parse(readFileSync(fixtureManifest, "utf8"));
    if (!Array.isArray(fm.serviceStart) || fm.serviceStart[0] !== "<fixture-child>") errors.push("fixture manifest serviceStart[0] must be <fixture-child> (digest-stamped executable)");
  }
  // Behavioural proof: the fixture run directly (no Hub, no hello) must emit ZERO
  // bytes and exit 0 — a product child never self-registers without a Hub hello.
  const direct = spawnSync(process.execPath, [fixtureChild, "--scenario", "normal"], { input: "", timeout: 3000 });
  if (direct.status !== 0) errors.push(`fixture-child direct (headless) run exited ${direct.status} (expected 0; child must not hang)`);
  if (direct.stdout && direct.stdout.length > 0) errors.push(`fixture-child direct (headless) run emitted ${direct.stdout.length} bytes (a child must never self-register without a Hub hello)`);

  const receiptRequired = receipt.required ?? [];
  if (!receiptRequired.includes("schemaVersion")) errors.push("receipt must require schemaVersion");
  if (!receiptRequired.includes("platform")) errors.push("receipt must require platform");
  const props = receipt.properties ?? {};
  if (!props.cases || !props.zeroOrphanProof) errors.push("receipt must require cases and zeroOrphanProof");
  const zp = props.zeroOrphanProof?.properties ?? {};
  if (!zp.tracked || !zp.live || !zp.descendants) errors.push("receipt zeroOrphanProof must model tracked/live/descendants");
  const outcome = props.cases?.additionalProperties?.properties?.outcome?.enum ?? [];
  if (outcome.includes("pass") && outcome.length === 1) errors.push("receipt must not allow pass-only outcomes");
  if (!outcome.includes("fail") || !outcome.includes("blocked")) errors.push("receipt outcomes must include fail and blocked");

  return { errors, matrix };
}

function emitRunner(platform) {
  const matrix = loadJson("matrix.json");
  const cases = matrix.cases.filter((c) => c.platforms.includes(platform));
  const lines = [
    `# Orthic O5 native lifecycle runner — ${platform}`,
    "# Emitted by tests/native/harness.mjs. Execute on a native host; record",
    "# each case outcome into a receipt conforming to receipt.schema.json.",
    "",
  ];
  for (const c of cases) {
    const step = c.steps.find((s) => s.platform === platform);
    lines.push(`# ${c.id} [${c.phase}]${c.adversarial ? " (adversarial)" : ""}`);
    for (const setup of c.setup ?? []) {
      const cmd = setup[platform];
      if (cmd) lines.push(`# setup: ${cmd.map((tok) => (/\s/.test(tok) ? `'${tok}'` : tok)).join(" ")}`);
    }
    lines.push(`#   assert: ${step.assert}`);
    lines.push(`${step.command.map((tok) => (/\s/.test(tok) ? `'${tok}'` : tok)).join(" ")}`);
    lines.push("");
  }
  return lines.join("\n");
}

const arg = process.argv[2] ?? "--self-check";
if (arg === "--self-check") {
  const { errors, matrix } = selfCheck();
  if (errors.length) {
    for (const e of errors) console.error(`self-check FAIL: ${e}`);
    process.exit(1);
  }
  console.log(`native harness self-check PASS: ${matrix.cases.length} cases cover ${REQUIRED_CASE_IDS.length} required O5 lifecycle + adversarial identity cases`);
} else if (arg === "--emit") {
  const platform = process.argv[3];
  if (!ADMITTED_PLATFORMS.includes(platform)) {
    console.error(`usage: node tests/native/harness.mjs --emit <mac|win>`);
    process.exit(2);
  }
  const out = resolve(here, `runner.${platform}.txt`);
  writeFileSync(out, emitRunner(platform));
  console.log(`wrote ${out}`);
} else {
  console.error("usage: node tests/native/harness.mjs [--self-check | --emit <mac|win>]");
  process.exit(2);
}