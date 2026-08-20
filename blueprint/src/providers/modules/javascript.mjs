// D28: JS/TS module resolvers — ESM/CJS/TS path resolution from repository
// files only. Deterministic and fixture-backed.

import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

const TS_EXTENSIONS = [".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs", ".json"];
const EXTENSIONS = [".js", ".mjs", ".cjs", ".json"];

function tryResolveFile(baseDir, specifier, extensions) {
  const candidate = resolve(baseDir, specifier);
  if (existsSync(candidate)) return candidate;
  for (const ext of extensions) {
    const withExt = `${candidate}${ext}`;
    if (existsSync(withExt)) return withExt;
  }
  const index = join(candidate, "index");
  for (const ext of extensions) {
    const withIndex = `${index}${ext}`;
    if (existsSync(withIndex)) return withIndex;
  }
  return null;
}

export function resolveModuleSpecifier({ specifier, fromFile, isTypeScript = true }) {
  if (!specifier || !fromFile) return { resolved: null, reason: "missing_input" };
  const absolute = resolve(specifier);
  if (absolute === specifier) {
    // Absolute path: only repo-confined resolution is permitted.
    return existsSync(absolute) ? { resolved: absolute, reason: "absolute" } : { resolved: null, reason: "missing" };
  }
  if (specifier.startsWith(".") || specifier.startsWith("/")) {
    const extensions = isTypeScript ? TS_EXTENSIONS : EXTENSIONS;
    const resolved = tryResolveFile(dirname(fromFile), specifier, extensions);
    return resolved ? { resolved, reason: "relative" } : { resolved: null, reason: "missing" };
  }
  // Bare specifier: resolve through node_modules from the file's directory.
  const nodeModules = findNodeModules(dirname(fromFile));
  if (!nodeModules) return { resolved: null, reason: "no_node_modules" };
  const parts = specifier.split("/");
  const packageName = specifier.startsWith("@") ? `${parts[0]}/${parts[1]}` : parts[0];
  const packageDir = join(nodeModules, packageName);
  const packageJson = join(packageDir, "package.json");
  if (existsSync(packageJson)) {
    const pkg = JSON.parse(readFileSync(packageJson, "utf8"));
    const entry = pkg.module ?? pkg.main ?? "index.js";
    const resolved = tryResolveFile(packageDir, entry, [".js", ".mjs", ".cjs", ".json"]);
    if (resolved) return { resolved, reason: "package" };
  }
  const direct = tryResolveFile(nodeModules, specifier, [".js", ".mjs", ".cjs", ".json"]);
  return direct ? { resolved: direct, reason: "bare" } : { resolved: null, reason: "missing" };
}

function findNodeModules(dir) {
  let current = dir;
  while (true) {
    const candidate = join(current, "node_modules");
    if (existsSync(candidate)) return candidate;
    const parent = dirname(current);
    if (parent === current) return null;
    current = parent;
  }
}
