#!/usr/bin/env node
// Installed-path CTX-001 qualification.  This runner owns no runtime,
// installer, storage, or protocol authority: every executable, endpoint, and
// restart action is supplied by the installed host under qualification.

import { mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { execFileSync, spawnSync } from "node:child_process";
import { dirname, isAbsolute, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const nonEmpty = (value) => typeof value === "string" && value.trim().length > 0;
const asJson = (value, label) => {
  try { return JSON.parse(value); } catch (error) { throw new Error(`${label} is not valid JSON: ${error.message}`); }
};

function argsFromEnv(name, fallback = []) {
  const value = process.env[name];
  return value === undefined ? fallback : asJson(value, name);
}

function command(bin, args, label, json = true) {
  if (!nonEmpty(bin)) throw new Error(`${label} executable is required`);
  const output = execFileSync(bin, args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
  if (!json) return output.trim();
  const text = output.trim();
  try { return JSON.parse(text); } catch (error) { throw new Error(`${label} did not emit JSON: ${error.message}`); }
}

function failedCommand(bin, args, label) {
  if (!nonEmpty(bin)) throw new Error(`${label} executable is required`);
  const result = spawnSync(bin, args, { encoding: "utf8" });
  if (result.status === 0) throw new Error(`${label} unexpectedly succeeded`);
  const lines = `${String(result.stdout || "")}\n${String(result.stderr || "")}`.split(/\r?\n/u).map((line) => line.trim()).filter(Boolean).reverse();
  for (const line of lines) { try { return JSON.parse(line); } catch { /* inspect next emitted line */ } }
  throw new Error(`${label} emitted no JSON error envelope`);
}

function scoredRecall(text, label) {
  const hits = String(text).split(/\r?\n/u).filter((line) => /^\s*[+-]?\d+(?:\.\d+)?\s+\S+\s+\S+/u.test(line));
  if (!hits.length) throw new Error(`${label} recall returned no scored hit`);
  return { text, scored_hits: hits };
}

function getArg(name, fallback) {
  const index = process.argv.indexOf(name);
  return index < 0 ? fallback : process.argv[index + 1];
}

function required(label, value) {
  if (value === undefined || value === null || value === "") throw new Error(`${label} missing from installed evidence`);
  return value;
}

function identity(snapshot, label) {
  const envelope = ["identity", "runtime_receipt", "runtimeReceipt", "installed_identity"].map((key) => snapshot?.[key]).find((value) => value && typeof value === "object") || snapshot;
  const direct = (keys) => keys.map((key) => envelope?.[key]).find((value) => value !== undefined && value !== null && value !== "");
  const db = direct(["cortex_db", "cortexDb"]);
  const installation = direct(["installation_id", "installationId"]);
  const service = direct(["service_instance_id", "serviceInstanceId"]);
  const startup = direct(["startup_generation", "startupGeneration"]);
  const release = direct(["release_generation", "releaseGeneration"]);
  const origin = direct(["runtime_origin", "runtimeOrigin"]);
  const stable = direct(["stable_install_root", "stableInstallRoot"]);
  const version = direct(["resolved_version_root", "resolvedVersionRoot"]);
  return {
    source: label,
    cortex_db: required(`${label}.cortex_db`, db),
    installation_id: required(`${label}.installation_id`, installation),
    service_instance_id: required(`${label}.service_instance_id`, service),
    startup_generation: Number(required(`${label}.startup_generation`, startup)),
    release_generation: required(`${label}.release_generation`, release),
    runtime_origin: required(`${label}.runtime_origin`, origin),
    stable_install_root: required(`${label}.stable_install_root`, stable),
    resolved_version_root: required(`${label}.resolved_version_root`, version),
  };
}

function storageEvidence(snapshot, label, hub = false) {
  const envelope = ["runtimeReceipt", "runtime_receipt"].map((key) => snapshot?.[key]).find((value) => value && typeof value === "object") || snapshot;
  const database = snapshot?.database;
  const wal = database?.startupWal?.main;
  if (hub) return {
    cortex_db: required(`${label}.cortex_db`, envelope?.cortexDb ?? envelope?.cortex_db),
    hub_wal: String(required(`${label}.hub_effectiveJournalMode`, wal?.effectiveJournalMode)).toLowerCase(),
    hub_wal_status: required(`${label}.hub_wal_status`, wal?.status),
  };
  return {
    cortex_db: required(`${label}.configured_path`, snapshot?.configured_path),
    schema_version: Number(required(`${label}.user_version`, snapshot?.sqlite?.user_version)),
    wal: String(required(`${label}.journal_mode`, snapshot?.sqlite?.journal_mode)).toLowerCase(),
  };
}

async function health(url, headers) {
  if (!nonEmpty(url)) throw new Error("MEMBRANE_HEALTH_URL is required");
  const response = await fetch(url, { headers });
  const body = await response.text();
  if (!response.ok) throw new Error(`authenticated Hub health failed (${response.status})`);
  return asJson(body, "Hub health");
}

function validateRecall(snapshot, label) {
  const status = snapshot?.status ?? snapshot?.result;
  if (!/^(ok|success|succeeded|complete|passed)$/iu.test(String(status))) throw new Error(`${label} recall did not report success`);
  const payload = snapshot?.data ?? snapshot?.result_data ?? snapshot?.records ?? snapshot?.memories ?? snapshot?.items;
  if ((Array.isArray(payload) && payload.length === 0) || payload === undefined || payload === null || payload === "") throw new Error(`${label} recall returned no content`);
  required(`${label}.trace_id`, snapshot?.trace_id ?? snapshot?.traceId);
}

function validateDoctor(snapshot, label) {
  if (!/^(ok|success|complete|passed)$/iu.test(String(snapshot?.status))) throw new Error(`${label} doctor status is not healthy`);
  if (snapshot?.checks !== undefined && (!Array.isArray(snapshot.checks) || snapshot.checks.some((check) => check?.status && !/^(ok|passed|success)$/iu.test(String(check.status))))) throw new Error(`${label} doctor contains failed checks`);
}

function buildIdentity(snapshot, label) {
  return {
    release_generation: required(`${label}.release_generation`, snapshot?.release_generation ?? snapshot?.releaseGeneration),
    source: required(`${label}.source`, snapshot?.membrane_source_commit),
    target: required(`${label}.target`, snapshot?.target ?? snapshot?.target_triple ?? snapshot?.targetTriple),
  };
}

function sanctionedTarget(manifestTarget) {
  const mapping = { "windows-x86_64": "x86_64-pc-windows-msvc", "macos-arm64": "aarch64-apple-darwin", "macos-x86_64": "x86_64-apple-darwin" };
  return mapping[manifestTarget];
}

function atomicJson(path, value) {
  mkdirSync(dirname(path), { recursive: true });
  const temporary = `${path}.${process.pid}.tmp`;
  writeFileSync(temporary, `${JSON.stringify(value, null, 2)}\n`, { mode: 0o600 });
  renameSync(temporary, path);
}

const sleep = (ms) => new Promise((resolvePromise) => setTimeout(resolvePromise, ms));

function compare(left, right, fields) {
  for (const field of fields) if (left[field] !== right[field]) throw new Error(`${field} diverged: ${left[field]} != ${right[field]}`);
}

async function main() {
  const task = getArg("--task", "CTX-001");
  if (task !== "CTX-001") throw new Error(`unsupported task ${task}; this runner is CTX-001 only`);
  const manifestPath = getArg("--release-manifest");
  const outputPath = getArg("--output");
  if (!nonEmpty(manifestPath) || !nonEmpty(outputPath)) throw new Error("--release-manifest & --output are required");

  const verifier = await import(pathToFileURL(resolve(HERE, "../release/verify-release-evidence.mjs")).href);
  const manifest = JSON.parse(readFileSync(resolve(manifestPath), "utf8"));
  verifier.verifyReleaseEvidence(manifest, resolve(manifestPath, ".."));

  const membraneBin = getArg("--membrane-bin", process.env.MEMBRANE_BIN);
  const healthHeaders = process.env.MEMBRANE_HEALTH_HEADERS_JSON ? asJson(process.env.MEMBRANE_HEALTH_HEADERS_JSON, "MEMBRANE_HEALTH_HEADERS_JSON") : null;
  if (!healthHeaders || typeof healthHeaders !== "object" || Array.isArray(healthHeaders) || !Object.keys(healthHeaders).length) throw new Error("authenticated Hub health headers are required");
  const doctorArgs = argsFromEnv("MEMBRANE_DOCTOR_ARGS_JSON", ["doctor", "--json"]);
  const hygieneArgs = argsFromEnv("MEMBRANE_HYGIENE_ARGS_JSON", ["hygiene", "storage"]);
  const buildInfoArgs = argsFromEnv("MEMBRANE_BUILD_INFO_ARGS_JSON", ["build-info"]);
  const recallArgs = argsFromEnv("MEMBRANE_RECALL_ARGS_JSON");
  if (!recallArgs.length) throw new Error("MEMBRANE_RECALL_ARGS_JSON is required");
  const codeRightBin = process.env.CODERIGHT_MEMORY_BIN;
  const codeRightArgs = argsFromEnv("CODERIGHT_MEMORY_ARGS_JSON");
  const restartBin = process.env.MEMBRANE_HUB_RESTART_BIN;
  const restartArgs = argsFromEnv("MEMBRANE_HUB_RESTART_ARGS_JSON");
  const lossTestBin = process.env.MEMBRANE_HUB_LOSS_TEST_BIN;
  const lossTestArgs = argsFromEnv("MEMBRANE_HUB_LOSS_TEST_ARGS_JSON");
  if (!nonEmpty(codeRightBin) || !codeRightArgs.length) throw new Error("CodeRight public memory command is required");
  if (!nonEmpty(restartBin)) throw new Error("installed Hub restart command is required");
  if (!nonEmpty(lossTestBin)) throw new Error("installed Hub-loss CodeRight test command is required");

  const capture = async (phase, retries = 0) => {
    let hub;
    for (let attempt = 0; ; attempt += 1) {
      try { hub = await health(process.env.MEMBRANE_HEALTH_URL, healthHeaders); break; }
      catch (error) { if (attempt >= retries) throw error; await sleep(1000); }
    }
    const doctor = command(membraneBin, doctorArgs, `${phase} installed doctor`);
    const hygiene = command(membraneBin, hygieneArgs, `${phase} installed hygiene storage`);
    const buildInfo = command(membraneBin, buildInfoArgs, `${phase} installed build-info`);
    const recall = scoredRecall(command(membraneBin, recallArgs, `${phase} installed recall`, false), `${phase} installed`);
    const codeRight = command(codeRightBin, codeRightArgs, `${phase} CodeRight public memory read/recall`);
    validateRecall(codeRight, `${phase} CodeRight`);
    validateDoctor(doctor, `${phase} installed`);
    return { phase, hub, doctor, hygiene, build_info: buildInfo, recall, code_right: codeRight };
  };

  const before = await capture("before_restart");
  const beforeIdentity = identity(before.hub, "before_restart.hub");
  const codeRightIdentity = identity(before.code_right, "before_restart.code_right");
  const beforeBuild = buildIdentity(before.build_info, "before_restart.build_info");
  const sharedFields = ["cortex_db", "installation_id", "service_instance_id", "startup_generation", "release_generation", "runtime_origin", "stable_install_root", "resolved_version_root"];
  compare(beforeIdentity, codeRightIdentity, sharedFields);
  if (beforeIdentity.runtime_origin !== "installed") throw new Error("before-restart runtime origin is not installed");
  const cliPath = resolve(membraneBin);
  const versionRoot = resolve(beforeIdentity.resolved_version_root);
  const stableRoot = resolve(beforeIdentity.stable_install_root);
  const under = (root) => { const rel = relative(root, cliPath); return !isAbsolute(rel) && (rel === "" || (!rel.startsWith("..") && !rel.startsWith(`..${process.platform === "win32" ? "\\" : "/"}`))); };
  if (!(under(versionRoot) || under(stableRoot))) throw new Error("installed CLI is outside resolved version/current binding");
  const beforeWal = storageEvidence(before.hub, "before_restart.hub", true);
  const beforeCliStorage = storageEvidence(before.hygiene, "before_restart.hygiene");
  if (beforeWal.cortex_db !== beforeIdentity.cortex_db || beforeCliStorage.cortex_db !== beforeIdentity.cortex_db || beforeWal.hub_wal !== beforeCliStorage.wal || beforeWal.hub_wal !== "wal" || beforeCliStorage.schema_version !== 27 || !/^(ok|passed|healthy)$/iu.test(String(beforeWal.hub_wal_status))) throw new Error("Hub WAL/storage evidence is not canonical");
  if (String(beforeBuild.release_generation) !== String(beforeIdentity.release_generation) || String(beforeBuild.release_generation) !== String(manifest.release?.generation)) throw new Error("CLI build-info release generation mismatch");
  if (beforeBuild.source !== required("manifest.release.commit", manifest.release?.commit)) throw new Error("CLI build-info source commit mismatch");
  if (beforeBuild.target !== sanctionedTarget(required("manifest.release.target", manifest.release?.target))) throw new Error("CLI build-info target does not match sanctioned manifest target mapping");

  const loss = failedCommand(lossTestBin, lossTestArgs, "installed Hub-loss typed-unavailability test");
  const lossCode = loss?.error_code ?? loss?.errorCode ?? loss?.code;
  const fallbackUsed = loss?.fallback_used ?? loss?.fallbackUsed;
  const noFallback = loss?.no_fallback ?? loss?.noFallback;
  if (!/^(service_unavailable|resident_service_unavailable)$/u.test(String(lossCode))) throw new Error("Hub-loss test did not return service_unavailable");
  if (!(fallbackUsed === false || noFallback === true)) throw new Error("Hub-loss test did not explicitly prove no fallback");

  command(restartBin, restartArgs, "installed Hub restart", false);
  const after = await capture("after_restart", 10);
  const afterIdentity = identity(after.hub, "after_restart.hub");
  const afterCodeRightIdentity = identity(after.code_right, "after_restart.code_right");
  const afterBuild = buildIdentity(after.build_info, "after_restart.build_info");
  compare(afterIdentity, afterCodeRightIdentity, sharedFields);
  compare(beforeIdentity, afterIdentity, ["cortex_db", "installation_id", "release_generation", "runtime_origin", "stable_install_root", "resolved_version_root"]);
  const afterWal = storageEvidence(after.hub, "after_restart.hub", true);
  const afterCliStorage = storageEvidence(after.hygiene, "after_restart.hygiene");
  if (afterWal.cortex_db !== afterIdentity.cortex_db || afterCliStorage.cortex_db !== afterIdentity.cortex_db || afterWal.hub_wal !== afterCliStorage.wal || afterWal.hub_wal !== "wal" || afterCliStorage.schema_version !== 27 || !/^(ok|passed|healthy)$/iu.test(String(afterWal.hub_wal_status))) throw new Error("after-restart Hub WAL/storage evidence is not canonical");
  compare(beforeBuild, afterBuild, ["release_generation", "source", "target"]);
  if (String(afterBuild.release_generation) !== String(afterIdentity.release_generation) || String(afterBuild.release_generation) !== String(manifest.release?.generation)) throw new Error("after-restart CLI build-info release generation mismatch");
  if (afterIdentity.runtime_origin !== "installed") throw new Error("after-restart runtime origin is not installed");
  if (afterIdentity.service_instance_id === beforeIdentity.service_instance_id) throw new Error("Hub service_instance_id did not rotate after restart");
  if (!(afterIdentity.startup_generation > beforeIdentity.startup_generation)) throw new Error("Hub startup_generation did not advance after restart");
  if (String(manifest.release?.generation) !== String(beforeIdentity.release_generation)) throw new Error("manifest release generation does not match installed evidence");

  const receipt = {
    schema: "membrane.ctx001-installed-receipt.v1",
    task,
    status: "passed",
    artifact_sha256: manifest.release?.artifact_sha256 ?? null,
    release_generation: manifest.release?.generation ?? null,
    manifest_commit: manifest.release?.commit ?? null,
    manifest_target: manifest.release?.target ?? null,
    installed_manifest: resolve(manifestPath),
    installed_current: beforeIdentity.stable_install_root,
    installed_version_root: beforeIdentity.resolved_version_root,
    before,
    after,
    recall_invocation: { installed_args: recallArgs, code_right_args: codeRightArgs },
    identities: { before: beforeIdentity, before_storage: beforeCliStorage, before_build: beforeBuild, code_right: codeRightIdentity, after: afterIdentity, after_storage: afterCliStorage, after_build: afterBuild, after_code_right: afterCodeRightIdentity },
    hub_loss: loss,
    assertions: { same_canonical_db: true, schema_27: true, wal_reported: true, restart_continuity: true, typed_hub_loss_no_fallback: true },
    generated_at: new Date().toISOString(),
  };
  atomicJson(resolve(outputPath), receipt);
  process.stdout.write(`${JSON.stringify({ status: receipt.status, output: resolve(outputPath) })}\n`);
}

main().catch((error) => { process.stderr.write(`CTX-001 qualification blocked: ${error.message}\n`); process.exitCode = 1; });
