import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
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
  assets: [
    join(output, "install.ps1"),
    join(output, "THIRD_PARTY_NOTICES.md"),
  ],
});
const github = publishGitHubRelease(githubPlan, {
  repo: "Orthic-Labs/Membrane",
  dryRun,
});
console.log(JSON.stringify({ github, dryRun }));
