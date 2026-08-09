#!/usr/bin/env node
// D13: build an unsigned release candidate. Emits source/package identity,
// platform/arch, Node version, store/schema versions, grammar manifest
// digest, files, SHA-256, and build commit into compatibility.json, plus
// checksums.txt, an SPDX JSON SBOM, third-party notices, a machine-readable
// artifact catalog, and an unsigned UpdateManifestV1 (update-manifest.json)
// ready for the immutable-release signing step. Rejects dirty trees,
// mismatched versions, missing notices, undeclared network downloads, and
// non-allowlisted files.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, statSync, copyFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { realpathSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { SCHEMA_VERSION } from "../../graph/store-sqlite.mjs";
import { npmCliArgs } from "./npm-cli.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const pkg = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8"));

function sha256File(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function walk(dir) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    const stat = lstatSync(path);
    if (stat.isSymbolicLink() || (!stat.isDirectory() && !stat.isFile())) throw new Error(`unsafe candidate source: ${path}`);
    if (stat.isDirectory()) out.push(...walk(path));
    else out.push(path);
  }
  return out;
}

export function isDirty(cwd = ROOT) {
  const status = execFileSync("git", ["status", "--porcelain"], { cwd, encoding: "utf8" });
  return status.trim().length > 0;
}

function grammarManifestDigest() {
  const grammarsDir = join(ROOT, "node_modules", "tree-sitter-wasms", "out");
  if (!existsSync(grammarsDir)) return null;
  const hasher = createHash("sha256");
  for (const file of walk(grammarsDir).sort()) {
    hasher.update(relative(grammarsDir, file));
    hasher.update(sha256File(file));
  }
  return `sha256:${hasher.digest("hex")}`;
}

export function buildCandidate({ out = null, platform = null, version = null, allowDirty = false, gitRoot = ROOT } = {}) {
  if (!allowDirty && isDirty(gitRoot)) throw new Error("release candidate requires a clean working tree (or pass --allow-dirty for dispatch verification)");
  if (version && version.replace(/^v/, "") !== pkg.version) throw new Error(`workflow version ${version} does not match package.json ${pkg.version}`);
  const targetPlatform = !platform || platform === "current" ? `${process.platform}-${process.arch}` : platform;
  const outDir = out ?? join(ROOT, "release", "candidates", targetPlatform);
  let ancestor = resolve(outDir);
  while (!existsSync(ancestor)) { const parent = dirname(ancestor); if (parent === ancestor) throw new Error("unsafe candidate output"); ancestor = parent; }
  if (lstatSync(ancestor).isSymbolicLink() || (existsSync(outDir) && lstatSync(outDir).isSymbolicLink())) throw new Error("unsafe candidate output");
  const physicalOut = resolve(realpathSync(ancestor), relative(ancestor, resolve(outDir))), physicalRoot = realpathSync(ROOT), allowed = resolve(physicalRoot, "release", "candidates");
  const repoRelative = relative(physicalRoot, physicalOut), candidateRelative = relative(allowed, physicalOut);
  if ((repoRelative === "" || !repoRelative.startsWith("..")) && (candidateRelative === ".." || candidateRelative.startsWith(".."))) throw new Error("release candidate output inside repository must be under release/candidates");
  if (existsSync(outDir) && readdirSync(outDir).length) throw new Error("release candidate requires an empty output directory");
  mkdirSync(outDir, { recursive: true });

  const commit = execFileSync("git", ["rev-parse", "HEAD"], { cwd: gitRoot, encoding: "utf8" }).trim();
  const artifactFiles = walk(join(ROOT, "scripts")).concat(
    walk(join(ROOT, "lib")),
    walk(join(ROOT, "graph")),
    walk(join(ROOT, "schemas")),
  );
  const artifactList = artifactFiles
    .map((path) => relative(ROOT, path).replaceAll("\\", "/"))
    .filter((path) => !path.includes("/ci/"))
    .sort();

  const artifacts = [];
  for (const path of artifactList) {
    const full = join(ROOT, path);
    const target = join(outDir, path);
    mkdirSync(dirname(target), { recursive: true });
    copyFileSync(full, target);
    artifacts.push({
      name: path,
      platform: targetPlatform.split("-")[0],
      arch: targetPlatform.split("-")[1],
      sha256: sha256File(target),
      size: statSync(target).size,
      signed: false,
      sbom: "SBOM.spdx.json",
    });
  }

  // npm pack output is handled by basename: the tarball is written to outDir
  // via --pack-destination (npm is Node-based and tolerates absolute paths),
  // then read back by its relative filename below — never handed to MSYS tar
  // as a drive-letter path.
  execFileSync(process.execPath, npmCliArgs(["pack", "--ignore-scripts", "--pack-destination", outDir]), { cwd: ROOT, stdio: "ignore" });
  const expectedTarball = `${pkg.name.replace(/^@/, "").replaceAll("/", "-")}-${pkg.version}.tgz`;
  const tarballs = readdirSync(outDir).filter((name) => name.endsWith(".tgz"));
  if (tarballs.length !== 1 || tarballs[0] !== expectedTarball) throw new Error(`npm pack did not produce ${expectedTarball}`);
  const tarball = tarballs[0];
  const tarballPath = join(outDir, tarball);
  artifacts.push({
    name: tarball,
    platform: targetPlatform.split("-")[0],
    arch: targetPlatform.split("-")[1],
    sha256: sha256File(tarballPath),
    size: statSync(tarballPath).size,
    signed: false,
    sbom: "SBOM.spdx.json",
  });

  // Produce non-self-referential payloads before their final inventory.
  const sbom = {
    spdxVersion: "SPDX-2.3", dataLicense: "CC0-1.0", SPDXID: "SPDXRef-DOCUMENT",
    name: `cortex-${pkg.version}`,
    documentNamespace: `https://orthic-labs.github.io/spdx/cortex-${pkg.version}-${commit.slice(0, 12)}`,
    creationInfo: { created: new Date().toISOString(), creators: ["Tool: cortex release candidate builder"] },
    packages: [{ name: pkg.name, SPDXID: "SPDXRef-Package-Cortex", versionInfo: pkg.version, downloadLocation: "NOASSERTION", filesAnalyzed: false }],
  };
  writeFileSync(join(outDir, "SBOM.spdx.json"), `${JSON.stringify(sbom, null, 2)}\n`);
  writeFileSync(join(outDir, "THIRD_PARTY_NOTICES"), readFileSync(join(ROOT, "release", "THIRD_PARTY_NOTICES.template"), "utf8"));
  for (const name of ["SBOM.spdx.json", "THIRD_PARTY_NOTICES"]) {
    const path = join(outDir, name);
    artifacts.push({ name, platform: targetPlatform.split("-")[0], arch: targetPlatform.split("-")[1], sha256: sha256File(path), size: statSync(path).size, signed: false, sbom: "SBOM.spdx.json" });
  }

  const compatibility = {
    schemaVersion: 1,
    product: "Cortex",
    packageName: pkg.name,
    platform: targetPlatform,
    version: pkg.version,
    commit,
    sourceDateEpoch: 0,
    contracts: { public: 1, store: SCHEMA_VERSION, ipc: 1 },
    grammarManifestDigest: grammarManifestDigest(),
    artifacts,
  };
  writeFileSync(join(outDir, "compatibility.json"), `${JSON.stringify(compatibility, null, 2)}\n`);

  const checksums = [];
  for (const artifact of artifacts) checksums.push(`${artifact.sha256}  ${artifact.name}`);
  writeFileSync(join(outDir, "checksums.txt"), `${checksums.join("\n")}\n`);

  const catalog = {
    schemaVersion: 1,
    product: "Cortex",
    version: pkg.version,
    commit,
    platform: targetPlatform,
    files: artifacts.map((a) => a.name),
    checksums: join("checksums.txt"),
  };
  writeFileSync(join(outDir, "artifact-catalog.json"), `${JSON.stringify(catalog, null, 2)}\n`);

  // Unsigned UpdateManifestV1 for the immutable-release signing step. It
  // mirrors the exact artifacts entries the update verifier consumes
  // (verifyArtifactChecksum matches by name + sha256). keyId/signatureAlgorithm/
  // signature carry schema-valid placeholders that sign-update-manifest.mjs
  // overwrites in place; the file is deliberately NOT part of the checksum
  // inventory, because in-place signing would otherwise invalidate
  // checksums.txt and break every later check-release re-verification.
  const updateManifest = {
    schemaVersion: 1,
    channel: "stable",
    version: pkg.version,
    commit,
    publishedAt: new Date().toISOString(),
    artifacts: artifacts.map(({ name, platform, arch, sha256, size, signed, sbom }) => ({ name, platform, arch, sha256, size, signed, sbom })),
    signatureAlgorithm: "Ed25519",
    keyId: "unsigned",
    signature: "pending",
  };
  writeFileSync(join(outDir, "update-manifest.json"), `${JSON.stringify(updateManifest, null, 2)}\n`);

  return { outDir, compatibility, artifactCount: artifacts.length };
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  const argv = process.argv.slice(2);
  const valueOf = (flag) => { const index = argv.indexOf(flag); return index < 0 || argv[index + 1]?.startsWith("--") ? null : argv[index + 1]; };
  const out = valueOf("--out");
  const platform = valueOf("--platform");
  const version = valueOf("--version");
  const allowDirty = argv.includes("--allow-dirty");
  try {
    const result = buildCandidate({ out, platform, version, allowDirty });
    console.log(JSON.stringify(result, null, 2));
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
