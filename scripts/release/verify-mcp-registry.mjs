import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const platforms = ["darwin-arm64", "darwin-x64", "linux-arm64", "linux-x64", "win32-arm64", "win32-x64"];
const fail = message => { throw new Error(`MCP registry contract: ${message}`); };
const digest = value => typeof value === "string" && /^sha256:[0-9a-f]{64}$/.test(value);
const safeId = value => typeof value === "string" && /^[A-Za-z0-9._:-]{1,160}$/.test(value);

async function json(path) { return JSON.parse(await readFile(path, "utf8")); }

/** Validate source metadata. Published installation requires independent registry evidence. */
export async function verifyMcpRegistry({ directory = root, requirePublished = true } = {}) {
  const [server, npm] = await Promise.all([json(resolve(directory, "server.json")), json(resolve(directory, "dist/npm/package.json"))]);
  // MCP Registry namespace remains GitHub-authority-bound to the live
  // Orthic-Labs/Membrane remote; package identities below are Membrane-owned.
  const expectedName = "io.github.orthic-labs/membrane";
  const expectedPackage = "@membrane/membrane";
  const expectedRepository = "https://github.com/orthic-labs/membrane";
  if (server.name !== expectedName || server.npm?.mcpName !== expectedName || npm.mcpName !== expectedName) fail("server name and npm mcpName must match official name");
  if (npm.name !== expectedPackage || server.npm?.package !== expectedPackage) fail("npm package identity mismatch");
  if (server.version !== npm.version || !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(server.version)) fail("version must be exact immutable semver");
  if (server.repository !== expectedRepository || npm.repository !== expectedRepository) fail("repository identity mismatch");
  if (server.npm?.install !== `npm:${expectedPackage}@${server.version}`) fail("install route must pin exact package version");
  if (requirePublished && (server.publication?.namespaceStatus !== "published" || server.publication?.artifactStatus !== "published")) fail("registry namespace or native artifacts are unpublished");
  for (const platform of platforms) {
    const artifact = server.nativeArtifacts?.[platform];
    if (artifact?.package !== `${expectedPackage}-${platform}` || artifact.version !== server.version) fail("native package map mismatch");
    if (requirePublished) {
      if (!digest(artifact.sha256) || artifact.signature?.algorithm !== "ed25519" || !safeId(artifact.signature?.keyId) || !safeId(artifact.signature?.value) || artifact.signature.digest !== artifact.sha256) fail("published native artifact evidence invalid");
      if ((platform.startsWith("darwin-") || platform.startsWith("win32-")) && !safeId(artifact.platformReceipt)) fail("published platform receipt missing");
    }
  }
  if (server.signatureRequirements?.artifact?.algorithm !== "ed25519" || server.signatureRequirements.artifact.subject !== "sha256") fail("ed25519 artifact binding missing");
  if (server.signatureRequirements?.darwin?.platform !== "apple-notary" || server.signatureRequirements?.win32?.platform !== "windows-authenticode") fail("platform signature requirements missing");
  if (requirePublished && (!safeId(server.publication?.namespaceReceipt?.receiptId) || !digest(server.publication?.namespaceReceipt?.sha256))) fail("published namespace receipt invalid");
  return { name: expectedName, version: server.version, install: server.npm.install, publication: server.publication };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  verifyMcpRegistry().then(result => process.stdout.write(`${JSON.stringify(result)}\n`)).catch(error => { process.stderr.write(`${error.message}\n`); process.exitCode = 1; });
}
