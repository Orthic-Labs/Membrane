#!/usr/bin/env node
// D14: stage a portable runtime bundle. Builds from the tested npm tarball,
// installs production dependencies into a staging directory, copies the
// runner Node LTS executable, app files, schemas, grammars, and
// platform watcher assets. The launcher computes its own install
// root and invokes the bundled runtime — never global Node/npm/cwd.

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, copyFileSync, readFileSync, writeFileSync, readdirSync, statSync, cpSync, chmodSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const pkg = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8"));

// Node executable discovery: prefer the active runtime; the CI release
// matrix supplies a pinned Node LTS via setup-node.
function nodeBinary() {
  return process.execPath;
}

function runPnpm(args, options) {
  if (process.platform === "win32") {
    execFileSync(process.env.ComSpec ?? "cmd.exe", ["/d", "/s", "/c", "pnpm.CMD", ...args], options);
  } else execFileSync("pnpm", args, options);
}

export function stageRuntime({ out = null } = {}) {
  if (!["darwin", "win32"].includes(process.platform)) throw new Error("Blueprint release packaging targets macOS & Windows only");
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
  runPnpm(["pack", "--pack-destination", temp], { cwd: ROOT, stdio: "ignore" });
  const tarball = readdirSync(temp).find((name) => name.endsWith(".tgz"));
  if (!tarball) throw new Error("npm pack produced no tarball");
  // GNU tar on Windows parses drive-letter archive paths as remote hosts.
  execFileSync("tar", ["-xzf", tarball], { cwd: temp, stdio: "ignore" });
  const packageDir = join(temp, "package");
  if (!existsSync(packageDir)) throw new Error("tarball has no package/");

  // 2. Production install inside the extracted package (grammars + watcher
  // assets resolve from the staged production install, not the source checkout).
  writeFileSync(join(packageDir, ".npmrc"), "package-lock=false\n");
  copyFileSync(join(ROOT, "pnpm-lock.yaml"), join(packageDir, "pnpm-lock.yaml"));
  writeFileSync(join(packageDir, "pnpm-workspace.yaml"), "packages:\n  - .\nallowBuilds:\n  '@parcel/watcher': true\n");
  runPnpm(["install", "--prod", "--frozen-lockfile"], { cwd: packageDir, stdio: "ignore", timeout: 240000 });
  rmSync(join(packageDir, "pnpm-workspace.yaml"), { force: true });

  // 3. Copy app files into app/package per the S-12 layout; schemas and
  // grammars live at app/schemas and app/grammars, resolved from the staged
  // production install (never the source checkout).
  const appPackageDir = join(appDir, "package");
  // pnpm deploy materializes portable dependencies instead of retaining
  // links into staging temp or machine-local content-addressable store.
  runPnpm(["--filter", ".", "deploy", "--prod", "--legacy", appPackageDir], { cwd: packageDir, stdio: "ignore", timeout: 240000 });
  // Runtime imports packages directly; npm command shims are not used.
  // Removing `.bin` prevents links into the temporary install root from
  // surviving after that root is deleted in `finally` below.
  rmSync(join(appPackageDir, "node_modules", ".bin"), { recursive: true, force: true });
  const schemasSrc = join(appPackageDir, "schemas");
  const schemas = join(appDir, "schemas");
  if (existsSync(schemasSrc)) cpSync(schemasSrc, schemas, { recursive: true });
  const grammarsSrc = join(appPackageDir, "node_modules", "tree-sitter-wasms", "out");
  const grammarsDst = join(appDir, "grammars");
  if (existsSync(grammarsSrc)) cpSync(grammarsSrc, grammarsDst, { recursive: true });

  // 4. Bundled Node runtime.
  const nodeSrc = nodeBinary();
  const nodeName = process.platform === "win32" ? "node.exe" : "node";
  const nodeDst = join(libDir, nodeName);
  copyFileSync(nodeSrc, nodeDst);

  // 5. Launchers.
  const launcherSrc = join(ROOT, "release", "launchers");
  const launchers = process.platform === "win32" ? ["blueprint.cmd", "blueprint-mcp.cmd"] : ["blueprint", "blueprint-mcp"];
  for (const name of launchers) {
    copyFileSync(join(launcherSrc, name), join(binDir, name));
    if (process.platform !== "win32") chmodSync(join(binDir, name), 0o755);
  }

  // 6. License + notices + readme.
  copyFileSync(join(ROOT, "LICENSE"), join(outDir, "LICENSE"));
  if (existsSync(join(ROOT, "release", "THIRD_PARTY_NOTICES.template"))) {
    copyFileSync(join(ROOT, "release", "THIRD_PARTY_NOTICES.template"), join(outDir, "THIRD_PARTY_NOTICES"));
  }
  if (existsSync(join(ROOT, "release", "README.txt"))) copyFileSync(join(ROOT, "release", "README.txt"), join(outDir, "README.txt"));

  return {
    root: outDir,
    layout: [...launchers.map((name) => `bin/${name}`), `lib/${nodeName}`, "app/package", "app/package/node_modules", "app/grammars", "app/schemas", "LICENSE", "THIRD_PARTY_NOTICES", "README.txt"],
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
