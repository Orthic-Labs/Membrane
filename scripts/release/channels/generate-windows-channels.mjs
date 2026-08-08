#!/usr/bin/env node
// Generates the WinGet + Scoop channel manifests from the artifacts RightKit
// actually produces for this app -- never a parallel, hand-authored notion
// of a release. Two real inputs are required before anything is generated:
//
//   1. RightKit's sealed Windows release manifest (read via
//      ./right-release-source.mjs, written by
//      tools/rightkit/packages/release/build-release.mjs to
//      .right-release/sealed/<app>-<version>-<commit8>/windows/release-manifest.json)
//      -- the real version, commit, installer name, SHA-256, and public R2
//      download URL.
//   2. The MBR-902 clean-machine receipt (validated via the unmodified
//      ../windows/contract.mjs#validateReceipt this task reuses read-only)
//      -- real signtool /pa /tw plus install/update/uninstall proof for
//      that exact sealed hash.
//
// Neither input is fabricated here. When either is missing, this script
// reports why and writes nothing -- matching the true state of this
// checkout today (no Windows release has been sealed yet).

import { readFile, writeFile, mkdir } from "node:fs/promises";
import { existsSync, readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { dirname, join, resolve } from "node:path";
import { loadWindowsChannelArtifact } from "./right-release-source.mjs";
import { validateReceipt } from "../windows/contract.mjs";
import { verifyWindowsChannels } from "../verify-windows-channels.mjs";
import { buildWingetManifests, validateWingetManifests, renderWingetYaml, PACKAGE_IDENTIFIER } from "./winget-manifest.mjs";
import { buildScoopManifest, validateScoopManifest } from "./scoop-manifest.mjs";

export const CONTRACT_PATH = "packaging/winget/release.v1.json";
export const SCOOP_PATH = "packaging/scoop/membrane.json";

/**
 * Builds the "ready" windows-channels contract (packaging/winget/release.v1.json
 * shape) from a verified RightKit artifact and a validated MBR-902 receipt.
 * Every field is copied from those two already-proven inputs; this function
 * adds no independent source of truth.
 */
export function buildReadyContract({ base, artifact, receiptRef }) {
  const contract = {
    schema: base.schema,
    state: "ready",
    publish: false,
    sourceIdentity: { commit: artifact.commit, version: `v${artifact.version}`, releaseGeneration: createHash("sha256").update(artifact.releaseGeneration).digest("hex") },
    artifact: {
      name: artifact.installerName,
      url: artifact.installerUrl,
      sha256: artifact.installerSha256,
      type: "nsis",
      silentInstall: "/S",
      silentUninstall: "/S",
    },
    trust: { receipt: receiptRef },
    upgrade: { detectInstalledVersion: true, allowDowngrade: false },
    channels: {
      winget: { packageIdentifier: base.channels.winget.packageIdentifier, installerUrl: artifact.installerUrl, installerSha256: artifact.installerSha256 },
      scoop: { bucket: base.channels.scoop.bucket, url: artifact.installerUrl, sha256: artifact.installerSha256 },
    },
  };
  return contract;
}

/**
 * End-to-end, side-effect-free: given a repo root, resolves the RightKit
 * sealed artifact and a receipt reference, and returns either
 * `{ status: "unavailable", reasons: [...] }` (nothing to generate yet) or
 * `{ status: "ready", contract, winget, scoop }` with every manifest already
 * validated. Never writes to disk -- see writeGeneratedChannels for that.
 */
export async function generateWindowsChannels({ root, version, commit, receiptPath, receiptSha256Override }) {
  const base = JSON.parse(await readFile(join(root, CONTRACT_PATH), "utf8"));
  const reasons = [];

  let artifact = null;
  try {
    artifact = loadWindowsChannelArtifact({ root, version, commit });
    if (!artifact) reasons.push("no sealed RightKit Windows release found (.right-release/sealed/<app>-<version>-<commit8>/windows/release-manifest.json)");
  } catch (error) {
    reasons.push(`sealed RightKit Windows release failed verification: ${error.message}`);
  }

  let receiptRef = null;
  let receipt = null;
  if (receiptPath) {
    const absolute = resolve(root, receiptPath);
    if (!existsSync(absolute)) {
      reasons.push(`receipt not found: ${receiptPath}`);
    } else {
      const bytes = readFileSync(absolute);
      const sha256 = createHash("sha256").update(bytes).digest("hex");
      if (receiptSha256Override && receiptSha256Override !== sha256) reasons.push("receipt sha256 override does not match file bytes");
      receiptRef = { path: receiptPath, sha256 };
      receipt = JSON.parse(bytes.toString("utf8"));
    }
  } else {
    reasons.push("no MBR-902 clean-machine receipt supplied (--receipt)");
  }

  if (reasons.length > 0) return { status: "unavailable", reasons };

  try {
    const receiptManifest = {
      product_version: artifact.version,
      source_commit: artifact.commit,
      installer_path: artifact.installerName,
      installer_sha256: artifact.installerSha256,
      signing: { provider: "azure-artifact-signing", trust: "public", timestamp: "rfc3161" },
    };
    validateReceipt(receipt, receiptManifest); // reused unmodified from ../windows/contract.mjs

    const contract = buildReadyContract({ base, artifact, receiptRef });
    const channelResult = await verifyWindowsChannels(contract, root); // reused unmodified from ../verify-windows-channels.mjs
    if (channelResult.status !== "ready") return { status: "unavailable", reasons: ["generated contract failed verifyWindowsChannels"] };

    const winget = validateWingetManifests(buildWingetManifests(contract));
    const scoop = validateScoopManifest(buildScoopManifest(contract));

    return { status: "ready", contract, winget, scoop };
  } catch (error) {
    return { status: "unavailable", reasons: [error.message] };
  }
}

/**
 * Writes the real, installable manifest files -- only ever called after
 * generateWindowsChannels has returned status "ready" (every field
 * validated). Refuses to write an unavailable result, so this function
 * cannot itself be used to plant a fake artifact.
 */
export async function writeGeneratedChannels(root, result) {
  if (result.status !== "ready") throw new Error("refusing to write: channel generation is not ready");
  const version = result.contract.sourceIdentity.version.replace(/^v/, "");
  const manifestDir = join(root, "packaging", "winget", "manifests", "o", "Orthic", "Membrane", version);
  await mkdir(manifestDir, { recursive: true });
  await writeFile(join(manifestDir, `${PACKAGE_IDENTIFIER}.yaml`), renderWingetYaml(result.winget.version));
  await writeFile(join(manifestDir, `${PACKAGE_IDENTIFIER}.installer.yaml`), renderWingetYaml(result.winget.installer));
  await writeFile(join(manifestDir, `${PACKAGE_IDENTIFIER}.locale.en-US.yaml`), renderWingetYaml(result.winget.locale));
  await mkdir(dirname(join(root, CONTRACT_PATH)), { recursive: true });
  await writeFile(join(root, CONTRACT_PATH), `${JSON.stringify(result.contract, null, 2)}\n`);
  await mkdir(dirname(join(root, SCOOP_PATH)), { recursive: true });
  await writeFile(join(root, SCOOP_PATH), `${JSON.stringify(result.scoop, null, 2)}\n`);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const args = process.argv.slice(2);
  const opt = (name) => { const i = args.indexOf(name); return i === -1 ? undefined : args[i + 1]; };
  const root = resolve(opt("--root") ?? process.cwd());
  const version = opt("--version") ?? JSON.parse(readFileSync(join(root, "apps", "membrane-hub", "package.json"), "utf8")).version;
  const commit = opt("--commit");
  const receiptPath = opt("--receipt");
  const write = args.includes("--write");
  if (!commit) {
    console.error("usage: generate-windows-channels.mjs --commit <40-char-sha> [--version <semver>] [--receipt <path>] [--root <repo>] [--write]");
    process.exit(2);
  }
  const result = await generateWindowsChannels({ root, version, commit, receiptPath });
  if (result.status === "ready" && write) await writeGeneratedChannels(root, result);
  console.log(JSON.stringify({ status: result.status, reasons: result.reasons, wrote: result.status === "ready" && write }, null, 2));
  process.exit(result.status === "ready" ? 0 : 1);
}
