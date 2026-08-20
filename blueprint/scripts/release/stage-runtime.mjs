#!/usr/bin/env node
// D14: stage a portable runtime bundle. Builds from the tested npm tarball,
// installs production dependencies into a staging directory, copies the
// runner Node LTS executable, app files, schemas, grammars, and
// macOS watcher assets. The launcher computes its own install
// root and invokes the bundled runtime — never global Node/npm/cwd.

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, copyFileSync, readFileSync, writeFileSync, readdirSync, statSync, cpSync, chmodSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { npmCliArgs } from "./npm-cli.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const pkg = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8"));

// Node executable discovery: prefer the active runtime; the CI release
// matrix supplies a pinned Node LTS via setup-node.
function nodeBinary() {
  return process.execPath;
}

export function stageRuntime({ out = null } = {}) {
  if (process.platform !== "darwin") throw new Error("Blueprint release packaging currently targets macOS only");
  const outDir = resolve(out ?? join(ROOT, "release", "runtime", `${process.platform}-${process.arch}`));
  if (existsSync(outDir) && (!statSync(outDir).isDirectory() || readdirSync(outDir).length)) {
    throw new Error(`output must be an empty directory: ${outDir}`);
  }
  const appDir = join(outDir, "app");
  const binDir = join(outDir, "bin");
  const libDir = join(outDir, "lib");
  mkdirSync(appDir, { recursive: true });
  mkdirSync(binDir, { recursive: true });
  mkdirSync(libDir, { recursive: true });

  // 1. Build the npm tarball and extract it as the app payload.
  const temp = mkdtempSync(join(tmpdir(), "blueprint-runtime-stage-"));
  try {
  execFileSync(process.execPath, npmCliArgs(["pack", "--pack-destination", temp]), { cwd: ROOT, stdio: "ignore" });
  const tarball = readdirSync(temp).find((name) => name.endsWith(".tgz"));
  if (!tarball) throw new Error("npm pack produced no tarball");
  execFileSync("tar", ["-xzf", join(temp, tarball), "-C", temp], { stdio: "ignore" });
  const packageDir = join(temp, "package");
  if (!existsSync(packageDir)) throw new Error("tarball has no package/");

  // 2. Production install inside the extracted package (grammars + watcher
  // assets resolve from the staged production install, not the source checkout).
  writeFileSync(join(packageDir, ".npmrc"), "package-lock=false\n");
  execFileSync(process.execPath, npmCliArgs(["install", "--omit=dev", "--no-audit", "--no-fund"]), { cwd: packageDir, stdio: "ignore", timeout: 240000 });

  // 3. Copy app files into app/package per the S-12 layout; schemas and
  // grammars live at app/schemas and app/grammars, resolved from the staged
  // production install (never the source checkout).
  const appPackageDir = join(appDir, "package");
  cpSync(packageDir, appPackageDir, { recursive: true });
  const schemasSrc = join(appPackageDir, "schemas");
  const schemas = join(appDir, "schemas");
  if (existsSync(schemasSrc)) cpSync(schemasSrc, schemas, { recursive: true });
  const grammarsSrc = join(appPackageDir, "node_modules", "tree-sitter-wasms", "out");
  const grammarsDst = join(appDir, "grammars");
  if (existsSync(grammarsSrc)) cpSync(grammarsSrc, grammarsDst, { recursive: true });

  // 4. Bundled Node runtime.
  const nodeSrc = nodeBinary();
  const nodeDst = join(libDir, "node");
  copyFileSync(nodeSrc, nodeDst);

  // 5. Launchers.
  const launcherSrc = join(ROOT, "release", "launchers");
  for (const name of ["blueprint", "blueprint-mcp"]) {
    copyFileSync(join(launcherSrc, name), join(binDir, name));
    chmodSync(join(binDir, name), 0o755);
  }

  // 6. License + notices + readme.
  copyFileSync(join(ROOT, "LICENSE"), join(outDir, "LICENSE"));
  if (existsSync(join(ROOT, "release", "THIRD_PARTY_NOTICES.template"))) {
    copyFileSync(join(ROOT, "release", "THIRD_PARTY_NOTICES.template"), join(outDir, "THIRD_PARTY_NOTICES"));
  }
  if (existsSync(join(ROOT, "release", "README.txt"))) copyFileSync(join(ROOT, "release", "README.txt"), join(outDir, "README.txt"));

  return {
    root: outDir,
    layout: ["bin/blueprint", "bin/blueprint-mcp", "lib/node", "app/package", "app/package/node_modules", "app/grammars", "app/schemas", "LICENSE", "THIRD_PARTY_NOTICES", "README.txt"],
    version: pkg.version,
    platform: `${process.platform}-${process.arch}`,
  };
  } finally {
    rmSync(temp, { recursive: true, force: true });
  }
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  const argv = process.argv.slice(2);
  const out = argv[argv.indexOf("--out") + 1] ?? null;
  try {
    const result = stageRuntime({ out });
    console.log(JSON.stringify(result, null, 2));
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
