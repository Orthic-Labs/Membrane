#!/usr/bin/env node
// MBR-904 — Homebrew Cask/Formula generator & validator.
//
// Source of truth: RightKit (@rightkit/release), already wired into
// apps/membrane-hub via package.json ("release:build:mac": "right-release
// build --platform mac") and apps/membrane-hub/right-release.config.mjs.
// This module never assumes a parallel/hand-rolled signing pipeline. It
// derives version, commit, URL, and SHA-256 from exactly two real inputs:
//
//   1. apps/membrane-hub/right-release.config.mjs — the committed build
//      config (app name, version, and the installer artifact -> R2 key map).
//      Read via dynamic import; never modified, never duplicated.
//   2. A RightKit sealed release manifest (.right-release/sealed/<releaseId>
//      /mac/release-manifest.json) — produced only by a real
//      `right-release build --platform mac` run. This module never runs that
//      build; it only reads an already-sealed manifest and re-verifies the
//      recorded SHA-256/size against the artifact bytes on disk, the same
//      defensive check @rightkit/release's own verifySealedRelease performs
//      (that function is not reused directly: @rightkit/release is only
//      installed under apps/membrane-hub/node_modules, not resolvable from a
//      repo-root script — confirmed empirically before writing this file).
//
// The public download URL is derived, never invented: RIGHTAPPS_PUBLIC_
// DOWNLOAD_BASE (same env var @rightkit/release/upload-release.mjs reads,
// same default) joined with the sealed manifest's own installer R2 key.
//
// Today's real config declares exactly one mac installer artifact
// (an aarch64/.dmg). No x86_64/Intel mac target exists anywhere in
// apps/membrane-hub/right-release.config.mjs, and no headless CLI/daemon
// tarball target exists for either architecture. Both are therefore
// resolved as an explicit, reported, fail-closed "blocked" state — never a
// silently-assumed or fabricated artifact. See resolveCaskArtifacts /
// resolveFormulaArtifacts below.

import { createHash } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, relative, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");

export const HOMEBREW_CONTRACT_SCHEMA = "orthic.membrane.homebrew-release.v1";
export const CASK_PATH = "packaging/homebrew/Casks/membrane.rb";
export const FORMULA_PATH = "packaging/homebrew/Formula/membrane.rb";
export const CONTRACT_PATH = "packaging/homebrew/release.v1.json";
export const MAC_ARCHES = Object.freeze(["mac-arm64", "mac-x64"]);
export const DEFAULT_PUBLIC_BASE = "https://pub-6c73208d46c245a9b4881d5e02f6b618.r2.dev";
export const PUBLIC_BASE_ENV = "RIGHTAPPS_PUBLIC_DOWNLOAD_BASE";

// Byte-identical to the committed placeholder files. Reused as the safe
// default render whenever real, both-architecture artifacts are not proven.
export const CASK_PLACEHOLDER = `cask "membrane" do
  raise <<~MESSAGE
    Membrane Homebrew Cask is intentionally unavailable.
    Generate it only from packaging/homebrew/release.v1.json after an immutable
    version, DMG URL, SHA-256, commit identity, codesign, notarization, & staple
    receipts exist for both Apple Silicon & Intel macOS.
  MESSAGE
end
`;
export const FORMULA_PLACEHOLDER = `class Membrane < Formula
  raise <<~MESSAGE
    Membrane Homebrew Formula is intentionally unavailable.
    Generate it only from packaging/homebrew/release.v1.json after an immutable
    version, CLI/daemon URL, SHA-256, commit identity, & signed release receipt
    exist for both Apple Silicon & Intel macOS.
  MESSAGE
end
`;

const SHA256_RE = /^[0-9a-f]{64}$/;
const COMMIT_RE = /^[0-9a-f]{40}$/;
const VERSION_RE = /^\d+\.\d+\.\d+$/;

const fail = (message) => { throw new Error(message); };

// ---------------------------------------------------------------------------
// Step 1 — read RightKit's real, committed build config. Read-only import;
// this module never edits apps/membrane-hub/right-release.config.mjs.
// ---------------------------------------------------------------------------
export async function readReleaseConfig(configPath) {
  const target = resolve(configPath);
  if (!existsSync(target)) fail(`right-release config missing: ${configPath}`);
  const mod = await import(pathToFileURL(target).href);
  const config = mod.default;
  if (!config || config.schema !== 1) fail("right-release.config.mjs schema invalid");
  if (typeof config.app !== "string" || !config.app) fail("config.app missing");
  if (typeof config.version !== "string" || !VERSION_RE.test(config.version)) fail("config.version invalid");
  return config;
}

// ---------------------------------------------------------------------------
// Step 2 — locate & re-verify a real RightKit sealed release manifest.
// ---------------------------------------------------------------------------
export function releaseIdFor({ app, version, commit }) {
  if (typeof app !== "string" || !app) fail("app is required");
  if (typeof version !== "string" || !VERSION_RE.test(version)) fail("version must be semver x.y.z");
  if (typeof commit !== "string" || !COMMIT_RE.test(commit)) fail("commit must be a 40-hex sha");
  return `${app}-${version}-${commit.slice(0, 8)}`;
}

function sha256File(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

export function readSealedManifest(sealedPlatformDir) {
  const manifestPath = resolve(sealedPlatformDir, "release-manifest.json");
  if (!existsSync(manifestPath)) fail(`sealed manifest missing: ${manifestPath} (run right-release build --platform mac first)`);
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  if (manifest.schema !== 1) fail("sealed manifest schema invalid");
  if (!Array.isArray(manifest.checkpoints) || !manifest.checkpoints.includes("sealed")) fail("release is not sealed");
  if (!Array.isArray(manifest.files) || !manifest.files.length) fail("sealed manifest has no files");
  for (const file of manifest.files) {
    const filePath = resolve(sealedPlatformDir, file.name);
    if (!existsSync(filePath)) fail(`sealed file missing: ${file.name}`);
    if (sha256File(filePath) !== file.sha256) fail(`sealed file hash mismatch: ${file.name}`);
    if (readFileSync(filePath).length !== file.sizeBytes) fail(`sealed file size mismatch: ${file.name}`);
  }
  return manifest;
}

// ---------------------------------------------------------------------------
// Step 3 — resolve per-architecture artifacts from real inputs only.
// ---------------------------------------------------------------------------
export function classifyMacArtifact(value) {
  if (typeof value !== "string" || !value) return null;
  if (/aarch64|arm64/i.test(value)) return "mac-arm64";
  if (/x86_64|x64/i.test(value)) return "mac-x64";
  return null;
}

const NO_INTEL_TARGET = "apps/membrane-hub/right-release.config.mjs declares no x86_64/Intel mac installer target (targets.mac.installer.artifacts has an aarch64 DMG only)";
const NO_ARM_ROUTE = "sealed manifest has no aarch64/arm64 installer route";

export function resolveCaskArtifacts({ config, manifest, publicBase = process.env[PUBLIC_BASE_ENV] || DEFAULT_PUBLIC_BASE }) {
  if (config.app !== manifest.app) fail(`config/manifest app mismatch: ${config.app} vs ${manifest.app}`);
  if (config.version !== manifest.version) fail(`config/manifest version mismatch: ${config.version} vs ${manifest.version}`);
  const declaredArtifacts = config.targets?.mac?.installer?.artifacts ?? [];
  const declaredKeys = new Set(declaredArtifacts.map((artifact) => artifact.key));
  const declaresIntel = declaredArtifacts.some((artifact) => classifyMacArtifact(artifact.key) === "mac-x64" || classifyMacArtifact(artifact.file) === "mac-x64");
  const filesByName = new Map(manifest.files.map((file) => [file.name, file]));
  const arches = {
    "mac-arm64": { blocked: true, reason: NO_ARM_ROUTE },
    "mac-x64": { blocked: true, reason: declaresIntel ? "x86_64/Intel mac target is declared but has no sealed release artifact yet" : NO_INTEL_TARGET },
  };
  for (const route of manifest.routes?.patch ?? []) {
    if (route.role !== "installer" || route.bucket !== "public") continue;
    const arch = classifyMacArtifact(route.name) ?? classifyMacArtifact(route.key);
    if (!arch) continue; // unrecognized naming — never guessed, left unclassified
    if (!declaredKeys.has(route.key)) fail(`sealed route key not declared in right-release.config.mjs: ${route.key}`);
    const file = filesByName.get(route.name);
    if (!file) fail(`sealed route references unsealed file: ${route.name}`);
    arches[arch] = { url: `${publicBase.replace(/\/$/, "")}/${route.key}`, sha256: file.sha256, filename: file.name, sizeBytes: file.sizeBytes };
  }
  const ready = !arches["mac-arm64"].blocked && !arches["mac-x64"].blocked;
  return { commit: manifest.commit, version: manifest.version, arches, ready };
}

// No headless CLI/daemon tarball target exists for mac at all today —
// apps/membrane-hub/right-release.config.mjs only builds/signs the Tauri Hub
// .app/.dmg (targets.mac.installer); there is no targets.mac.headless. This
// is therefore always a declared, reported gap, for both architectures,
// never an invented tarball URL/hash.
export function resolveFormulaArtifacts({ config }) {
  const reason = "apps/membrane-hub/right-release.config.mjs declares no headless CLI/daemon release target for mac (only the Tauri Hub app/dmg is built); Homebrew Formula has no real artifact to consume yet";
  const hasHeadlessTarget = Boolean(config.targets?.mac?.headless);
  if (hasHeadlessTarget) fail("targets.mac.headless is declared but this generator does not yet resolve it from a sealed manifest — extend resolveFormulaArtifacts before relying on it");
  return {
    version: config.version,
    arches: { "mac-arm64": { blocked: true, reason }, "mac-x64": { blocked: true, reason } },
    ready: false,
  };
}

// ---------------------------------------------------------------------------
// Renderers — both fall back to the exact committed placeholder unless every
// required field for both architectures is a real, validated value.
// ---------------------------------------------------------------------------
function assertSha256(value, label) { if (typeof value !== "string" || !SHA256_RE.test(value)) fail(`${label} must be a lowercase sha256`); }
function assertHttpsUrl(value, label) {
  if (typeof value !== "string" || !value) fail(`${label} must be a URL`);
  let parsed; try { parsed = new URL(value); } catch { fail(`${label} must be a URL`); }
  if (parsed.protocol !== "https:") fail(`${label} must be https`);
}
function assertVersion(value) { if (typeof value !== "string" || !VERSION_RE.test(value)) fail("version must be semver x.y.z"); }

export function renderCask(resolved) {
  if (!resolved?.ready) return CASK_PLACEHOLDER;
  const arm = resolved.arches["mac-arm64"];
  const intel = resolved.arches["mac-x64"];
  assertSha256(arm.sha256, "mac-arm64 sha256"); assertSha256(intel.sha256, "mac-x64 sha256");
  assertHttpsUrl(arm.url, "mac-arm64 url"); assertHttpsUrl(intel.url, "mac-x64 url");
  assertVersion(resolved.version);
  return `cask "membrane" do
  version "${resolved.version}"

  on_arm do
    url "${arm.url}"
    sha256 "${arm.sha256}"
  end
  on_intel do
    url "${intel.url}"
    sha256 "${intel.sha256}"
  end

  name "Membrane"
  desc "Membrane Hub — local-first context assembly"
  homepage "https://github.com/adrdsouza/claude"

  depends_on macos: ">= 12.0"

  app "Membrane Hub.app"

  uninstall quit: "com.membrane.hub"

  zap trash: [
    "~/Library/Application Support/Membrane",
    "~/Library/Caches/com.membrane.hub",
    "~/Library/Preferences/com.membrane.hub.plist",
    "~/Library/Saved Application State/com.membrane.hub.savedState",
  ]
end
`;
}

export function renderFormula(resolved) {
  if (!resolved?.ready) return FORMULA_PLACEHOLDER;
  const arm = resolved.arches["mac-arm64"];
  const intel = resolved.arches["mac-x64"];
  assertSha256(arm.sha256, "mac-arm64 sha256"); assertSha256(intel.sha256, "mac-x64 sha256");
  assertHttpsUrl(arm.url, "mac-arm64 url"); assertHttpsUrl(intel.url, "mac-x64 url");
  assertVersion(resolved.version);
  return `class Membrane < Formula
  desc "Membrane headless CLI and crypt-service daemon"
  homepage "https://github.com/adrdsouza/claude"
  version "${resolved.version}"

  on_macos do
    if Hardware::CPU.arm?
      url "${arm.url}"
      sha256 "${arm.sha256}"
    else
      url "${intel.url}"
      sha256 "${intel.sha256}"
    end
  end

  def install
    bin.install "membrane"
    bin.install "crypt-service"
  end

  service do
    run [opt_bin/"crypt-service"]
    keep_alive true
    log_path var/"log/membrane/crypt-service.log"
    error_log_path var/"log/membrane/crypt-service.err.log"
  end

  test do
    system "#{bin}/membrane", "--version"
  end
end
`;
}

// ---------------------------------------------------------------------------
// Validators — structural, never trust text without cross-checking the
// resolved data it claims to come from.
// ---------------------------------------------------------------------------
export function validateCaskSource(source, resolved) {
  if (!resolved?.ready) {
    if (!/intentionally unavailable/.test(source)) fail("cask source must declare intentional unavailability while not ready");
    if (/\burl\s+['"]/.test(source)) fail("cask source must not embed a url while not ready");
    if (/sha256\s+['"][0-9a-f]{64}['"]/.test(source)) fail("cask source must not embed a sha256 while not ready");
    if (/version\s+['"]\d/.test(source)) fail("cask source must not embed a real version while not ready");
    return true;
  }
  for (const [label, entry] of Object.entries(resolved.arches)) {
    if (!source.includes(entry.url)) fail(`cask source missing ${label} url`);
    if (!source.includes(entry.sha256)) fail(`cask source missing ${label} sha256`);
  }
  if (/:no_check/.test(source)) fail("cask source must not disable checksum verification");
  if (/:latest/.test(source)) fail("cask source must not pin :latest");
  if (!/on_arm do/.test(source) || !/on_intel do/.test(source)) fail("cask source must handle mac-arm64 and mac-x64 explicitly");
  return true;
}

export function validateFormulaSource(source, resolved) {
  if (!resolved?.ready) {
    if (!/intentionally unavailable/.test(source)) fail("formula source must declare intentional unavailability while not ready");
    if (/\burl\s+['"]/.test(source)) fail("formula source must not embed a url while not ready");
    if (/sha256\s+['"][0-9a-f]{64}['"]/.test(source)) fail("formula source must not embed a sha256 while not ready");
    if (/version\s+['"]\d/.test(source)) fail("formula source must not embed a real version while not ready");
    return true;
  }
  for (const [label, entry] of Object.entries(resolved.arches)) {
    if (!source.includes(entry.url)) fail(`formula source missing ${label} url`);
    if (!source.includes(entry.sha256)) fail(`formula source missing ${label} sha256`);
  }
  if (/:no_check/.test(source)) fail("formula source must not disable checksum verification");
  if (/:latest/.test(source)) fail("formula source must not pin :latest");
  if (!/Hardware::CPU\.arm\?/.test(source)) fail("formula source must handle mac-arm64 and mac-x64 explicitly");
  return true;
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------
const COMMANDS = ["plan", "render", "validate"];
const usage = `generate.mjs <${COMMANDS.join("|")}> [options]`;
const cliFail = (message) => { console.error(`homebrew-generate: ${message}`); process.exit(2); };
const confinedOutput = (path) => { const target = resolve(root, path); if (!target.startsWith(`${root}${sep}`)) cliFail("output path must stay inside repository"); return target; };

async function main() {
  const args = process.argv.slice(2);
  const command = args.shift();
  const value = (name) => { const i = args.indexOf(name); return i < 0 ? undefined : args[i + 1]; };
  if (!command || !COMMANDS.includes(command)) cliFail(usage);

  if (command === "plan") {
    const commit = value("--commit");
    if (!COMMIT_RE.test(commit || "")) cliFail("--commit must be a 40-hex sha");
    const configPath = value("--config") ?? "apps/membrane-hub/right-release.config.mjs";
    const sealedRoot = value("--sealed-root") ?? ".right-release/sealed";
    const publicBase = value("--public-base") ?? process.env[PUBLIC_BASE_ENV] ?? DEFAULT_PUBLIC_BASE;
    let config;
    try { config = await readReleaseConfig(resolve(root, configPath)); }
    catch (error) { cliFail(`config invalid: ${error.message}`); }
    const id = releaseIdFor({ app: config.app, version: config.version, commit });
    const sealedDir = resolve(root, sealedRoot, id, "mac");
    let manifest;
    try { manifest = readSealedManifest(sealedDir); }
    catch (error) { cliFail(`sealed manifest invalid: ${error.message}`); }
    if (manifest.commit !== commit) cliFail(`sealed manifest commit mismatch: expected ${commit}, found ${manifest.commit}`);
    let cask, formula;
    try { cask = resolveCaskArtifacts({ config, manifest, publicBase }); formula = resolveFormulaArtifacts({ config }); }
    catch (error) { cliFail(`resolution failed: ${error.message}`); }
    const summary = {
      schema: HOMEBREW_CONTRACT_SCHEMA,
      releaseId: id,
      sealedDir: relative(root, sealedDir),
      releaseGeneration: sha256File(resolve(sealedDir, "release-manifest.json")),
      cask, formula,
    };
    const text = `${JSON.stringify(summary, null, 2)}\n`;
    const out = value("--out");
    if (out) writeFileSync(confinedOutput(out), text, { flag: "wx" }); else process.stdout.write(text);
    process.exit(0);
  }

  if (command === "render") {
    const contractPath = value("--contract");
    if (!contractPath) cliFail("--contract is required");
    let resolved;
    try { resolved = JSON.parse(readFileSync(resolve(root, contractPath), "utf8")); }
    catch (error) { cliFail(`contract invalid: ${error.message}`); }
    let caskText, formulaText;
    try {
      caskText = renderCask(resolved.cask);
      formulaText = renderFormula(resolved.formula);
      validateCaskSource(caskText, resolved.cask);
      validateFormulaSource(formulaText, resolved.formula);
    } catch (error) { cliFail(`render failed: ${error.message}`); }
    const outCask = value("--out-cask");
    const outFormula = value("--out-formula");
    if (outCask) writeFileSync(confinedOutput(outCask), caskText, { flag: "wx" });
    if (outFormula) writeFileSync(confinedOutput(outFormula), formulaText, { flag: "wx" });
    if (!outCask && !outFormula) process.stdout.write(`--- Cask ---\n${caskText}\n--- Formula ---\n${formulaText}\n`);
    process.exit(0);
  }

  if (command === "validate") {
    const caskPath = value("--cask") ?? CASK_PATH;
    const formulaPath = value("--formula") ?? FORMULA_PATH;
    const contractPath = value("--contract");
    let caskSrc, formulaSrc;
    try { caskSrc = readFileSync(resolve(root, caskPath), "utf8"); formulaSrc = readFileSync(resolve(root, formulaPath), "utf8"); }
    catch (error) { cliFail(`unable to read cask/formula: ${error.message}`); }
    let resolved = { cask: { ready: false }, formula: { ready: false } };
    if (contractPath) {
      try { resolved = JSON.parse(readFileSync(resolve(root, contractPath), "utf8")); }
      catch (error) { cliFail(`contract invalid: ${error.message}`); }
    }
    try { validateCaskSource(caskSrc, resolved.cask); validateFormulaSource(formulaSrc, resolved.formula); }
    catch (error) { cliFail(error.message); }
    process.stdout.write("Homebrew Cask and Formula validated\n");
    process.exit(0);
  }
}

if (import.meta.url === `file://${process.argv[1]}`) await main();
