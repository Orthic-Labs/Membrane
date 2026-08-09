#!/usr/bin/env node
// D14: build a platform archive from a staged runtime bundle. Creates
// cortex-<platform>-<arch>.tar.gz (macOS/Linux) or .zip (Windows) plus a
// checksum, and registers it in the release catalog shape.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync, statSync, writeFileSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { stageRuntime } from "./stage-runtime.mjs";

export function buildRuntimeArchive({ out = null, stage = null } = {}) {
  const staged = stage ?? stageRuntime({ out: null }).root;
  const outDir = out ?? join(staged, "..");
  const platform = process.platform;
  const arch = process.arch;
  const archiveName = `cortex-${platform}-${arch}.${platform === "win32" ? "zip" : "tar.gz"}`;
  const archivePath = join(outDir, archiveName);
  const cwd = resolve(staged, "..");
  const dirName = basename(staged);

  if (platform === "win32") {
    execFileSync("powershell", ["-NoProfile", "-Command", `Compress-Archive -Path '${staged}\\*' -DestinationPath '${archivePath}' -Force`], { stdio: "ignore" });
  } else {
    execFileSync("tar", ["-czf", archivePath, dirName], { cwd, stdio: "ignore" });
  }

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
