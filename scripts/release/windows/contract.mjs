import { readFile } from "node:fs/promises";
import { basename, resolve } from "node:path";

export const INSTALLER_NAME = /^Membrane_[0-9]+\.[0-9]+\.[0-9]+_x64-setup\.exe$/;

export async function readJson(path) {
  try { return JSON.parse(await readFile(resolve(path), "utf8")); }
  catch (error) { throw new Error(`invalid JSON input: ${path}: ${error.message}`); }
}

export function validateReleaseManifest(manifest) {
  const required = ["product_version", "source_commit", "installer_path", "installer_sha256"];
  for (const key of required) if (typeof manifest?.[key] !== "string" || !manifest[key]) throw new Error(`missing manifest field: ${key}`);
  if (!INSTALLER_NAME.test(basename(manifest.installer_path))) throw new Error("installer must be Membrane_<version>_x64-setup.exe");
  if (basename(manifest.installer_path) !== `Membrane_${manifest.product_version}_x64-setup.exe`) throw new Error("installer version must match product_version");
  if (!/^[0-9a-f]{40}$/.test(manifest.source_commit)) throw new Error("source_commit must be a 40-character git SHA");
  if (!/^[0-9a-f]{64}$/.test(manifest.installer_sha256)) throw new Error("installer_sha256 must be SHA-256");
  if (manifest.signing?.provider !== "azure-artifact-signing") throw new Error("signing.provider must be azure-artifact-signing");
  if (manifest.signing?.trust !== "public") throw new Error("signing.trust must be public");
  if (manifest.signing?.timestamp !== "rfc3161") throw new Error("signing.timestamp must be rfc3161");
  return manifest;
}

export function signingPlan(manifest, inputPath) {
  validateReleaseManifest(manifest);
  if (basename(inputPath) !== basename(manifest.installer_path)) throw new Error("input installer does not match manifest identity");
  return { tool: "signtool", args: ["sign", "/fd", "SHA256", "/tr", "http://timestamp.acs.microsoft.com", "/td", "SHA256", "/dlib", "<Azure.CodeSigning.Dlib.dll>", "/dmdf", "<artifact-signing-metadata.json>", inputPath], requires: ["DefaultAzureCredential", "Azure.CodeSigning.Dlib.dll", "artifact-signing-metadata.json"], verify: ["signtool", "verify", "/pa", "/tw", inputPath] };
}

export function validateReceipt(receipt, manifest) {
  validateReleaseManifest(manifest);
  if (receipt?.schema !== "windows-installer-receipt.v1" || receipt.status !== "pass") throw new Error("receipt must be windows-installer-receipt.v1 with status pass");
  if (receipt.source_commit !== manifest.source_commit || receipt.installer_sha256 !== manifest.installer_sha256) throw new Error("receipt identity does not match manifest");
  for (const gate of ["signature", "install", "update", "uninstall"]) if (receipt.gates?.[gate] !== "pass") throw new Error(`receipt gate not passed: ${gate}`);
  return receipt;
}
