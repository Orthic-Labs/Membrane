import { cpSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  collectReleaseAsset,
  materializeDirectRelease,
  planBootstrapPublication,
  renderPowerShellBootstrap,
  validatePowerShellBootstrap,
} from "@rightkit/release/direct-bootstrap.mjs";

if (process.platform !== "win32") throw new Error("portable Windows release finalization must run on Windows");

const hub = fileURLToPath(new URL("../", import.meta.url));
const repo = fileURLToPath(new URL("../../../", import.meta.url));
const pkg = JSON.parse(readFileSync(join(hub, "package.json"), "utf8"));
const output = join(hub, "dist", "portable");
const archiveName = `membrane-${pkg.version}-windows-x86_64.zip`;
const archivePath = join(output, archiveName);
const provenancePath = join(output, "provenance-windows-x86_64.intoto.jsonl");
const sbomPath = join(output, "sbom-windows-x86_64.cdx.json");
const git = spawnSync("git", ["rev-parse", "HEAD"], { cwd: repo, encoding: "utf8", windowsHide: true });
if (git.error || git.status !== 0) throw new Error("source commit resolution failed");
const sourceCommit = git.stdout.trim();
if (!/^[0-9a-f]{40}$/.test(sourceCommit)) throw new Error(`invalid source commit: ${sourceCommit}`);
const releaseBase = `https://github.com/Orthic-Labs/Membrane/releases/download/v${pkg.version}`;
const asset = collectReleaseAsset({
  target: "windows-x86_64",
  name: archiveName,
  url: `${releaseBase}/${archiveName}`,
  archivePath,
  executablePath: "membrane.exe",
  nativeSignaturePolicy: "authenticode-valid",
  provenancePath,
  sbomPath,
});
const directRelease = materializeDirectRelease({
  outputDir: output,
  manifestInput: {
    product: "membrane",
    version: pkg.version,
    sourceCommit,
    minimumBootstrapVersion: pkg.version,
    assets: [asset],
  },
});
writeFileSync(join(output, "release-manifest-signing.json"), `${JSON.stringify(directRelease.signing, null, 2)}\n`);
const bootstrap = renderPowerShellBootstrap({
  product: "membrane",
  repository: "Orthic-Labs/Membrane",
  bootstrapVersion: pkg.version,
  acceptedManifestSigners: [directRelease.signing.signer],
  installRootSubdir: "Orthic Labs\\Membrane",
  executablePath: "membrane.exe",
  activationArgs: ["activate", "--install-root", "{current}"],
  statusArgs: ["activate", "--install-root", "{current}", "--dry-run"],
  healthAssertions: [
    { path: "schemaVersion", equals: 1 },
    { path: "dryRun", equals: true },
    { path: "service.serviceId", equals: "membrane-hub" },
    { path: "service.releaseGeneration", nonempty: true },
    { path: "clients", minCount: 2 },
  ],
});
const bootstrapValidation = validatePowerShellBootstrap(bootstrap, {
  product: "membrane",
  acceptedSignerIds: [directRelease.signing.signer.id],
});
if (!bootstrapValidation.valid) throw new Error(`bootstrap invalid: ${bootstrapValidation.errors.join("; ")}`);
const bootstrapPath = join(output, "install.ps1");
writeFileSync(bootstrapPath, bootstrap);
cpSync(join(repo, "docs", "product", "legal", "THIRD-PARTY-NOTICES.txt"), join(output, "THIRD_PARTY_NOTICES.md"));
const publicationPlan = planBootstrapPublication({
  product: "membrane",
  bootstrapVersion: pkg.version,
  scriptPath: bootstrapPath,
});
writeFileSync(join(output, "bootstrap-publication-plan.json"), `${JSON.stringify(publicationPlan, null, 2)}\n`);
console.log(JSON.stringify({ manifest: directRelease.manifestPath, signature: directRelease.signaturePath, bootstrap: bootstrapPath }));
