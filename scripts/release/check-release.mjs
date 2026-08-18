#!/usr/bin/env node
// D13: verify a release candidate directory. Every artifact must be
// checksummed, inventoried, and SBOM-backed; publishing must remain
// impossible in this workflow.

import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { existsSync, lstatSync, readFileSync, readdirSync } from "node:fs";
import { dirname, isAbsolute, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

import { loadUpdateManifest } from "../../src/lib/update/manifest.mjs";

export function verifyCandidate(candidateDir) {
  const dir = resolve(candidateDir);
  const problems = [];
  const compatibilityPath = join(dir, "compatibility.json");
  const checksumsPath = join(dir, "checksums.txt");
  const sbomPath = join(dir, "SBOM.spdx.json");
  const catalogPath = join(dir, "artifact-catalog.json");
  const updateManifestPath = join(dir, "update-manifest.json");
  for (const [name, path] of [["compatibility.json", compatibilityPath], ["checksums.txt", checksumsPath], ["SBOM.spdx.json", sbomPath], ["artifact-catalog.json", catalogPath], ["update-manifest.json", updateManifestPath]]) {
    if (!existsSync(path) || !lstatSync(path).isFile()) problems.push(`missing ${name} or unsafe`);
  }
  if (problems.length) return { ok: false, problems };

  const compatibility = JSON.parse(readFileSync(compatibilityPath, "utf8"));
  const checksumLines = readFileSync(checksumsPath, "utf8").trim().split("\n").filter(Boolean);
  const checksums = checksumLines.map((line) => /^([a-f0-9]{64})  ([^\r\n]+)$/.exec(line)).filter(Boolean);
  if (checksums.length !== checksumLines.length) problems.push("checksums.txt has invalid entries");
  const byName = new Map(checksums.map(([, hash, name]) => [name, hash]));
  const names = (compatibility.artifacts ?? []).map((artifact) => artifact.name);
  const safe = (name) => typeof name === "string" && name.length > 0 && !isAbsolute(name) && name !== ".." && !name.startsWith(`..${sep}`) && !name.includes("../") && !name.includes("\\..\\");
  if (!compatibility.packageName || !names.length || new Set(names).size !== names.length || names.some((name) => !safe(name))) problems.push("unsafe or duplicate artifact inventory");
  if (new Set(checksums.map(([, , name]) => name)).size !== checksums.length || new Set(byName.keys()).size !== names.length || names.some((name) => !byName.has(name)) || [...byName.keys()].some((name) => !names.includes(name))) problems.push("checksums.txt does not exactly inventory artifacts");

  for (const artifact of compatibility.artifacts ?? []) {
    if (!safe(artifact.name)) continue;
    const path = join(dir, artifact.name);
    if (!existsSync(path) || !lstatSync(path).isFile()) { problems.push(`artifact missing or unsafe on disk: ${artifact.name}`); continue; }
    if (!/^[a-f0-9]{64}$/.test(artifact.sha256 ?? "")) { problems.push(`invalid checksum: ${artifact.name}`); continue; }
    const actual = createHash("sha256").update(readFileSync(path)).digest("hex");
    if (actual !== artifact.sha256) problems.push(`checksum mismatch: ${artifact.name}`);
    if (byName.get(artifact.name) !== artifact.sha256) problems.push(`checksums.txt mismatch: ${artifact.name}`);
  }
  for (const [, , name] of checksums) {
    if (!existsSync(join(dir, name))) problems.push(`checksums.txt references missing file: ${name}`);
  }

  const sbom = JSON.parse(readFileSync(sbomPath, "utf8"));
  if (sbom.spdxVersion !== "SPDX-2.3") problems.push("SBOM is not SPDX-2.3");
  const catalog = JSON.parse(readFileSync(catalogPath, "utf8"));
  if (!Array.isArray(catalog.files) || new Set(catalog.files).size !== catalog.files.length || catalog.checksums !== "checksums.txt" || catalog.version !== compatibility.version || catalog.platform !== compatibility.platform || catalog.files.length !== names.length || catalog.files.some((name) => !names.includes(name))) problems.push("artifact catalog does not exactly inventory artifacts");
  let updateManifest = null;
  try {
    updateManifest = loadUpdateManifest(updateManifestPath);
  } catch (error) {
    problems.push(`update-manifest.json is not a valid UpdateManifestV1: ${error.message}`);
  }
  if (updateManifest) {
    if (updateManifest.version !== compatibility.version || updateManifest.commit !== compatibility.commit) problems.push("update manifest does not match candidate identity");
    const manifestEntries = updateManifest.artifacts ?? [];
    const manifestNames = manifestEntries.map((entry) => entry.name);
    if (manifestNames.length !== names.length || new Set(manifestNames).size !== manifestNames.length || manifestNames.some((name) => !names.includes(name)) || names.some((name) => !manifestNames.includes(name))) problems.push("update manifest does not inventory the candidate artifacts");
    if (manifestEntries.some((entry) => compatibility.artifacts.find((artifact) => artifact.name === entry.name)?.sha256 !== entry.sha256)) problems.push("update manifest artifact hash mismatch");
  }
  const tarballs = names.filter((name) => name.endsWith(".tgz"));
  const expectedTarball = `${String(compatibility.packageName).replace(/^@/, "").replaceAll("/", "-")}-${compatibility.version}.tgz`;
  if (tarballs.length !== 1 || tarballs[0] !== expectedTarball) problems.push("candidate must contain exactly one package tarball");
  else {
    try {
      // Reference the archive by relative basename with cwd set to the archive
      // directory: MSYS GNU tar on Windows reads drive-letter paths as remote hosts.
      const entries = execFileSync("tar", ["-tf", tarballs[0]], { cwd: dir, encoding: "utf8" }).trim().split(/\r?\n/).filter(Boolean);
      const types = execFileSync("tar", ["-tvf", tarballs[0]], { cwd: dir, encoding: "utf8" }).trim().split(/\r?\n/).filter(Boolean);
      const safeEntry = (entry) => entry === "package/" || (entry.startsWith("package/") && !entry.includes("\\") && !entry.split("/").includes(".."));
      if (!entries.includes("package/package.json") || new Set(entries).size !== entries.length || entries.some((entry) => !safeEntry(entry)) || types.length !== entries.length || types.some((entry) => !/^[\-d]/.test(entry))) throw new Error("unsafe tar entries");
      const packaged = JSON.parse(execFileSync("tar", ["-xOf", tarballs[0], "package/package.json"], { cwd: dir, encoding: "utf8" }));
      if (packaged.name !== compatibility.packageName || packaged.version !== compatibility.version) problems.push("tarball package identity mismatch");
    } catch { problems.push("tarball package metadata unreadable"); }
  }
  const disk = walkFiles(dir);
  const expected = new Set(["compatibility.json", "checksums.txt", "artifact-catalog.json", "update-manifest.json", ...names]);
  if (disk.some((name) => !expected.has(name)) || [...expected].some((name) => !disk.includes(name))) problems.push("candidate contains unlisted or missing files");

  return { ok: problems.length === 0, problems, compatibility, catalog, updateManifest };
}

function walkFiles(root, dir = root) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    const stat = lstatSync(path);
    if (stat.isSymbolicLink() || (!stat.isDirectory() && !stat.isFile())) return ["__unsafe__"];
    if (stat.isDirectory()) out.push(...walkFiles(root, path));
    else out.push(relative(root, path).replaceAll("\\", "/"));
  }
  return out;
}

// U60: wire Hub installer pin-check (byte-identical, no rebuild)
// The catalog template must carry the Hub version + checksum placeholders
// that release-candidate.yml verifies via verify-hub-installer.mjs.
export function verifyCatalogTemplate() {
  const templatePath = join(resolve(dirname(fileURLToPath(import.meta.url)), "../.."), "release", "catalog.template.json");
  if (!existsSync(templatePath)) return { ok: false, problems: ["catalog.template.json missing"] };
  const template = JSON.parse(readFileSync(templatePath, "utf8"));
  const problems = [];
  if (!("hubAppVersion" in template)) problems.push("catalog.template.json missing hubAppVersion (U60)");
  if (!("hubInstallerChecksum" in template)) problems.push("catalog.template.json missing hubInstallerChecksum (U60)");
  return { ok: problems.length === 0, problems, template };
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  const dir = process.argv[2];
  if (!dir) { console.error("usage: check-release.mjs <candidate-dir>"); process.exit(1); }
  const result = verifyCandidate(dir);
  if (!result.ok) {
    console.error("check-release FAILED:");
    for (const problem of result.problems) console.error(`  ${problem}`);
    process.exit(1);
  }
  console.log(`check-release OK: ${result.compatibility.artifacts.length} artifacts verified against checksums and SBOM`);
  // U60 pin-check: fail if catalog template is missing the Hub fields (even when candidate itself is OK)
  const pin = verifyCatalogTemplate();
  if (!pin.ok) {
    console.error("check-release FAILED (U60 pin):");
    for (const p of pin.problems) console.error(`  ${p}`);
    process.exit(1);
  }
  console.log(`check-release OK (U60 pin): catalog.template.json carries hubAppVersion + hubInstallerChecksum`);
}
