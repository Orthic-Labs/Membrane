#!/usr/bin/env node
// D14: build reusable native archives from staged runtime bundles. Creates
// blueprint-<platform>-<arch>.tar.gz (macOS or Windows) plus checksum.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { basename, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { stageRuntime } from "./stage-runtime.mjs";

export function buildRuntimeArchive({ out = null, stage = null } = {}) {
  const staged = stage ?? stageRuntime({ out: null }).root;
  const outDir = resolve(out ?? join(staged, ".."));
  const platform = process.platform;
  if (!["darwin", "win32"].includes(platform)) throw new Error("Blueprint release packaging targets macOS & Windows only");
  const arch = process.arch;
  const archiveName = `blueprint-${platform}-${arch}.tar.gz`;
  mkdirSync(outDir, { recursive: true });
  const archivePath = join(outDir, archiveName);
  const cwd = resolve(staged, "..");
  const dirName = basename(staged);

  // Pass archive path relative to tar's cwd. GNU tar on Windows treats an
  // absolute drive-letter path as a remote host, while relative paths work on
  // both native Windows tar.exe and macOS tar.
  const archiveTarget = relative(cwd, archivePath).replaceAll("\\", "/");
  execFileSync("tar", ["-czf", archiveTarget, dirName], { cwd, stdio: "ignore" });

  const hash = createHash("sha256").update(readFileSync(archivePath)).digest("hex");
  writeFileSync(`${archivePath}.sha256`, `${hash}  ${archiveName}\n`);
  return {
    archive: archivePath,
    name: archiveName,
    sha256: hash,
    size: statSync(archivePath).size,
    platform,
    arch,
  };
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  const argv = process.argv.slice(2);
  const out = argv[argv.indexOf("--out") + 1] ?? null;
  const stage = argv[argv.indexOf("--stage") + 1] ?? null;
  try {
    const result = buildRuntimeArchive({ out, stage });
    console.log(JSON.stringify(result, null, 2));
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
