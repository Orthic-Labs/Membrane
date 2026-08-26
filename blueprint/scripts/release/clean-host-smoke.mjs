#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { createPrivateKey, createPublicKey, generateKeyPairSync, sign } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { DatabaseSync } from "node:sqlite";

import { verifyCandidate } from "./check-release.mjs";
import { deriveUpdateKeyId } from "./generate-update-keys.mjs";
import { npmCliArgs, pnpmCliArgs } from "./npm-cli.mjs";
import { signUpdateManifest } from "./sign-update-manifest.mjs";
import { loadTrustedUpdateKeys } from "../../src/lib/update/manifest.mjs";
import { verifyMcpInitialize } from "./mcp-client-smoke.mjs";

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: "utf8", timeout: 120000, ...options });
  if (result.status !== 0) throw new Error(`${command} ${args.join(" ")} failed: ${result.stderr || result.stdout}`);
  return result.stdout;
}

function runNode(script, args, options = {}) {
  return run(process.execPath, [script, ...args], options);
}

export function validateQueryEvidence(payload, needle = "releaseProof") {
  const results = Array.isArray(payload?.results) ? payload.results : [];
  const file = `file:${needle}.mjs`, symbol = `symbol:${needle}.mjs::${needle}`;
  const refs = results.map((result) => result?.id).filter((id) => id === file || id === symbol);
  if (!refs.includes(file) || !refs.includes(symbol)) throw new Error(`query returned no exact evidence for ${needle}`);
  return { matchCount: refs.length, refs };
}

// CX-B4: a shipped trust root must never be weakened by the smoke lifecycle.
// The manifest is signed with the UPDATE_SIGNING_KEY_PEM key and the derived
// keyId must already exist in the shipped root; a missing or mismatched key
// fails clearly instead of silently degrading to the ephemeral path.
export function resolveShippedSigningKey(shippedKeys, privateKeyPem) {
  if (!privateKeyPem) throw new Error("shipped trust root requires UPDATE_SIGNING_KEY_PEM to sign the smoke manifest");
  const keyId = deriveUpdateKeyId(createPublicKey(createPrivateKey(privateKeyPem)));
  if (!shippedKeys?.[keyId]) throw new Error(`signing key ${keyId} is not present in the shipped trust root; refusing to degrade the smoke lifecycle`);
  return keyId;
}

// CX-B4: the trust-root update lifecycle shared by the full rehearsal and the
// focused reproduction. Stage shape is fixed: exact update_trust_root_missing
// while the installed root is moved aside (restored in finally), then full
// apply + rollback against the provisioned ephemeral root or the untouched
// shipped root, with store evidence checked at every step.
export async function runUpdateTrustLifecycle({ blueprint, packageRoot, repo, app, prior, update, manifestPath, manifest, ephemeralKeys, useShippedRoot, shippedKeys, shippedRootPath, signingKeyPem } = {}) {
  const stages = { updateTrustMissing: false, updateApply: false, rollback: false };
  const graph = join(repo, ".agent", "graph", "graph.db");
  let db = new DatabaseSync(graph); db.exec("CREATE TABLE IF NOT EXISTS clean_host_update_proof (value TEXT); DELETE FROM clean_host_update_proof; INSERT INTO clean_host_update_proof VALUES ('before')"); db.close();
  // Exact missing-root regression: temporarily move only the installed trust
  // root aside so the first apply must report update_trust_root_missing,
  // then restore it before trust provisioning or the apply/rollback
  // lifecycle. The restore runs in finally so a failed assertion can never
  // leave the installed root absent for later stages.
  const trustRootBackup = `${shippedRootPath}.smoke-absent`;
  const shippedRootExisted = existsSync(shippedRootPath);
  if (shippedRootExisted) renameSync(shippedRootPath, trustRootBackup);
  try {
    const missingTrust = spawnSync(process.execPath, [blueprint, "update", "apply", "--manifest", manifestPath, "--artifact", update, "--artifact-name", "local", "--app-dir", app, "--prior-dir", prior, "--repo-root", repo, "--json"], { cwd: repo, encoding: "utf8" });
    const missingTrustReason = missingTrust.status === 0 ? null : JSON.parse(missingTrust.stdout).reason;
    if (missingTrustReason !== "update_trust_root_missing") throw new Error(`installed update did not fail closed with update_trust_root_missing (${missingTrustReason ?? "applied"}): ${missingTrust.stderr || missingTrust.stdout}`);
    stages.updateTrustMissing = true;
  } finally {
    if (shippedRootExisted) renameSync(trustRootBackup, shippedRootPath);
  }
  const { canonicalManifestPayload } = await import(pathToFileURL(join(packageRoot, "src", "lib", "update", "manifest.mjs")));
  let shippedRootBefore = null, signingKeyId = null;
  if (useShippedRoot) {
    // The shipped root is never overwritten. The manifest is signed with the
    // test/release signing key (UPDATE_SIGNING_KEY_PEM) using the same
    // sign-update-manifest.mjs behavior, and the derived keyId must already
    // exist in the shipped root before the full apply/rollback lifecycle runs.
    shippedRootBefore = readFileSync(shippedRootPath, "utf8");
    const pem = signingKeyPem ?? process.env.UPDATE_SIGNING_KEY_PEM;
    signingKeyId = resolveShippedSigningKey(shippedKeys, pem);
    signUpdateManifest(manifest, { privateKeyPem: pem });
  } else {
    // Empty shipped root: retain the generated ephemeral key path, provision
    // the installed temp trust root with it, then run the full lifecycle.
    manifest.keyId = "ephemeral";
    manifest.signatureAlgorithm = "Ed25519";
    manifest.signature = sign(null, Buffer.from(canonicalManifestPayload(manifest)), ephemeralKeys.privateKey).toString("base64");
    writeFileSync(shippedRootPath, JSON.stringify({ schemaVersion: 1, keys: [{ keyId: "ephemeral", algorithm: "Ed25519", publicKey: ephemeralKeys.publicKey.export({ type: "spki", format: "pem" }) }] }));
  }
  writeFileSync(manifestPath, JSON.stringify(manifest));
  const apply = JSON.parse(runNode(blueprint, ["update", "apply", "--manifest", manifestPath, "--artifact", update, "--artifact-name", "local", "--app-dir", app, "--prior-dir", prior, "--repo-root", repo, "--json"], { cwd: repo }));
  if (!apply.ok) throw new Error(`installed update apply failed: ${apply.reason}`);
  if (readFileSync(join(app, "version.txt"), "utf8") !== "after\n") throw new Error("staged update did not apply");
  db = new DatabaseSync(graph); db.exec("UPDATE clean_host_update_proof SET value = 'after'"); db.close();
  stages.updateApply = true;
  const rolled = JSON.parse(runNode(blueprint, ["update", "rollback", "--app-dir", app, "--prior-dir", prior, "--repo-root", repo, "--json"], { cwd: repo }));
  if (!rolled.ok || readFileSync(join(app, "version.txt"), "utf8") !== "before\n") throw new Error("packaged rollback did not restore prior app");
  db = new DatabaseSync(graph); const restored = db.prepare("SELECT value FROM clean_host_update_proof").get().value; db.close();
  if (restored !== "before") throw new Error("packaged rollback did not restore store");
  stages.rollback = true;
  if (useShippedRoot && readFileSync(shippedRootPath, "utf8") !== shippedRootBefore) throw new Error("shipped trust root was overwritten by the smoke lifecycle");
  return { stages, restored, rootUntouched: useShippedRoot ? true : null, keyId: signingKeyId };
}

export async function runCleanHostSmoke({ candidate } = {}) {
  const candidateDir = resolve(candidate ?? "");
  const verified = verifyCandidate(candidateDir);
  if (!verified.ok) throw new Error(`candidate verification failed: ${verified.problems.join("; ")}`);
  const tarballs = verified.compatibility.artifacts.filter((artifact) => artifact.name.endsWith(".tgz"));
  if (tarballs.length !== 1) throw new Error("candidate must contain one npm tarball");
  const temp = mkdtempSync(join(tmpdir(), "blueprint-clean-host-"));
  const stages = { verify: true, init: false, query: false, mcp: false, updateCheck: false, updateTrustMissing: false, updateApply: false, rollback: false, uninstall: false };
  try {
    const prefix = join(temp, "prefix");
    const tarball = join(candidateDir, tarballs[0].name);
    mkdirSync(prefix, { recursive: true });
    const installArgs = process.platform === "win32"
      ? pnpmCliArgs(["--dir", prefix, "add", "--prod", "--ignore-scripts", tarball])
      : npmCliArgs(["install", "--prefix", prefix, "--omit=dev", "--no-audit", "--no-fund", tarball]);
    run(process.execPath, installArgs);
    const packageRoot = join(prefix, "node_modules", ...verified.compatibility.packageName.split("/"));
    if (!existsSync(packageRoot)) throw new Error("local tarball was not installed");
    // The packaged trust root is authoritative. When it carries at least one
    // key, verification must run against it and the ephemeral smoke key must
    // never replace it; only an empty (or absent) shipped root falls back to
    // installing the ephemeral key, and only after the fail-closed stage.
    let shippedKeys;
    try { shippedKeys = loadTrustedUpdateKeys(join(packageRoot, "src", "lib", "update", "trusted-update-keys.json")); }
    catch { throw new Error("shipped trust root is corrupt"); }
    const useShippedRoot = !!shippedKeys && Object.keys(shippedKeys).length >= 1;
    const trustRoot = { source: useShippedRoot ? "shipped" : "ephemeral", keyCount: useShippedRoot ? Object.keys(shippedKeys).length : 0 };
    const repo = join(temp, "repo");
    mkdirSync(repo);
    run("git", ["init", "-q", repo]);
    writeFileSync(join(repo, "BLUEPRINT-AGENT.md"), "# original\n", "utf8");
    writeFileSync(join(repo, "releaseProof.mjs"), "export function releaseProof() { return true; }\n", "utf8");
    run("git", ["-C", repo, "add", "releaseProof.mjs"]);
    const blueprint = join(packageRoot, "scripts", "blueprint.mjs");
    const mcp = join(packageRoot, "scripts", "blueprint-mcp.mjs");
    JSON.parse(runNode(blueprint, ["init", "--host", "generic", "--mcp", "off", "--watch", "off", "--json"], { cwd: repo }));
    stages.init = true;
    const query = JSON.parse(runNode(blueprint, ["search", "--query", "release", "--json"], { cwd: repo }));
    const queryEvidence = validateQueryEvidence(query);
    stages.query = true;
    await verifyMcpInitialize({ script: mcp, root: repo });
    stages.mcp = true;
    JSON.parse(runNode(blueprint, ["update", "check", "--offline", "--json"], { cwd: repo }));
    stages.updateCheck = true;
    const { treeDigest } = await import(pathToFileURL(join(packageRoot, "src", "lib", "update", "manifest.mjs")));
    const app = join(repo, "app"), prior = join(repo, "app.prior");
    const update = join(temp, "update");
    mkdirSync(app); mkdirSync(update);
    writeFileSync(join(app, "version.txt"), "before\n");
    writeFileSync(join(update, "version.txt"), "after\n");
    writeFileSync(join(app, "package.json"), JSON.stringify({ name: "@membrane/blueprint-target", version: "0.2.0" }));
    writeFileSync(join(update, "package.json"), JSON.stringify({ name: "@membrane/blueprint-target", version: "0.3.0" }));
    const keys = generateKeyPairSync("ed25519"), manifestPath = join(temp, "manifest.json");
    const manifest = { schemaVersion: 1, channel: "stable", version: "0.3.0", commit: "a".repeat(40), publishedAt: "2026-08-08T00:00:00Z", artifacts: [{ name: "local", packageName: "@membrane/blueprint-target", platform: process.platform, arch: process.arch, sha256: treeDigest(update) }], signatureAlgorithm: "Ed25519", keyId: "ephemeral", signature: "" };
    const shippedRootPath = join(packageRoot, "src", "lib", "update", "trusted-update-keys.json");
    const lifecycle = await runUpdateTrustLifecycle({
      blueprint, packageRoot, repo, app, prior, update, manifestPath, manifest,
      ephemeralKeys: keys, useShippedRoot, shippedKeys, shippedRootPath,
      signingKeyPem: process.env.UPDATE_SIGNING_KEY_PEM,
    });
    Object.assign(stages, lifecycle.stages);
    if (lifecycle.rootUntouched) { trustRoot.rootUntouched = true; trustRoot.keyId = lifecycle.keyId; }
    const uninstalled = JSON.parse(runNode(blueprint, ["uninstall", "--root", repo, "--json"], { cwd: repo }));
    if (!uninstalled.ok || readFileSync(join(repo, "BLUEPRINT-AGENT.md"), "utf8") !== "# original\n") throw new Error("uninstall did not restore host file");
    stages.uninstall = true;
    return { schemaVersion: 1, ok: true, stages, trustRoot, queryEvidence, storeEvidence: { before: "before", after: "after", restored: lifecycle.restored } };
  } finally {
    rmSync(temp, { recursive: true, force: true });
  }
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  const index = process.argv.indexOf("--candidate");
  runCleanHostSmoke({ candidate: index < 0 ? null : process.argv[index + 1] })
    .then((report) => console.log(JSON.stringify(report)))
    .catch((error) => { console.error(error.message); process.exitCode = 1; });
}
