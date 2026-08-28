import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  createWranglerR2Client,
  planBootstrapPublication,
  publishBootstrapPlan,
} from "@rightkit/release/direct-bootstrap.mjs";
import {
  prepareGitHubDirectRelease,
  publishGitHubRelease,
} from "@rightkit/release/github-release.mjs";

if (process.platform !== "win32") throw new Error("portable Windows publication must run on Windows");

const hub = fileURLToPath(new URL("../", import.meta.url));
const repo = fileURLToPath(new URL("../../../", import.meta.url));
const pkg = JSON.parse(readFileSync(join(hub, "package.json"), "utf8"));
const output = join(hub, "dist", "portable");
const dryRun = process.argv.includes("--dry-run");
const signing = JSON.parse(readFileSync(join(output, "release-manifest-signing.json"), "utf8"));
const githubPlan = prepareGitHubDirectRelease({
  repoRoot: repo,
  repo: "Orthic-Labs/Membrane",
  product: "membrane",
  version: pkg.version,
  manifestPath: join(output, "release-manifest.json"),
  signaturePath: join(output, "release-manifest.cat"),
  signing,
  checksumsPath: join(output, "checksums.json"),
  archivePaths: [join(output, `membrane-${pkg.version}-windows-x86_64.zip`)],
  provenancePaths: [join(output, "provenance-windows-x86_64.intoto.jsonl")],
  sbomPaths: [join(output, "sbom-windows-x86_64.cdx.json")],
  assets: [join(output, "THIRD_PARTY_NOTICES.md")],
});
const github = publishGitHubRelease(githubPlan, {
  repo: "Orthic-Labs/Membrane",
  dryRun,
});
const bootstrapPlan = planBootstrapPublication({
  product: "membrane",
  bootstrapVersion: pkg.version,
  scriptPath: join(output, "install.ps1"),
});
if (dryRun) {
  console.log(JSON.stringify({ github, bootstrap: bootstrapPlan, dryRun: true }));
  process.exit(0);
}

const verificationRoot = mkdtempSync(join(tmpdir(), "membrane-r2-verify-"));
try {
  const r2 = publishBootstrapPlan(bootstrapPlan, createWranglerR2Client(), {
    verificationPath: join(verificationRoot, "install.ps1"),
  });
  console.log(JSON.stringify({ github, r2 }));
} finally {
  rmSync(verificationRoot, { recursive: true, force: true });
}
