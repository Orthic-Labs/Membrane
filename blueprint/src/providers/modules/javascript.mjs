// D28: JS/TS module resolvers — ESM/CJS/TS path resolution from repository
// files only. Deterministic and fixture-backed.

import { existsSync, readFileSync, statSync } from "node:fs";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";

const TS_EXTENSIONS = [".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs", ".json"];
const EXTENSIONS = [".js", ".mjs", ".cjs", ".json"];

function isFile(path) {
  try {
    return statSync(path).isFile();
  } catch {
    return false;
  }
}

function resolvedCandidate(path, resolutionTier) {
  return { resolved: path, status: "RESOLVED", candidates: [path], resolutionTier };
}

function ambiguousCandidates(candidates, reason, resolutionTier) {
  return {
    resolved: null,
    status: "AMBIGUOUS",
    reason,
    candidates: [...candidates].sort((left, right) => left.localeCompare(right)),
    resolutionTier,
  };
}

function unresolvedCandidate(reason = "missing") {
  return { resolved: null, status: "UNRESOLVED", reason, candidates: [] };
}

function tryResolveFile(baseDir, specifier, extensions) {
  const candidate = resolve(baseDir, specifier);
  if (isFile(candidate)) return resolvedCandidate(candidate, "exact");

  const extensionCandidates = extensions.map((ext) => `${candidate}${ext}`).filter(isFile);
  if (extensionCandidates.length === 1) return resolvedCandidate(extensionCandidates[0], "extension");
  if (extensionCandidates.length > 1) {
    return ambiguousCandidates(extensionCandidates, "ambiguous_extension", "extension");
  }

  const index = join(candidate, "index");
  const indexCandidates = extensions.map((ext) => `${index}${ext}`).filter(isFile);
  if (indexCandidates.length === 1) return resolvedCandidate(indexCandidates[0], "index");
  if (indexCandidates.length > 1) {
    return ambiguousCandidates(indexCandidates, "ambiguous_index", "index");
  }
  return unresolvedCandidate();
}

function inside(root, target) {
  const rel = relative(root, target);
  return rel === "" || (!rel.startsWith("..") && !isAbsolute(rel));
}

export function extractJavaScriptModuleSpecifiers(text) {
  const found = [];
  const lines = String(text ?? "").split(/\r?\n/);
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    const patterns = [
      /(?:import|export)(?:\s+type)?[\s\S]*?\sfrom\s+["']([^"']+)["']/g,
      /import\s*["']([^"']+)["']/g,
      /require\s*\(\s*["']([^"']+)["']\s*\)/g,
    ];
    for (const pattern of patterns) {
      for (const match of line.matchAll(pattern)) {
        found.push({ specifier: match[1], line: index + 1 });
      }
    }
  }
  return found.filter((item, index) => found.findIndex((candidate) => candidate.specifier === item.specifier && candidate.line === item.line) === index);
}

export function resolveModuleSpecifier({ specifier, fromFile, repoRoot = null, isTypeScript = true }) {
  if (!specifier || !fromFile) return unresolvedCandidate("missing_input");
  const absolute = resolve(specifier);
  if (absolute === specifier) {
    // Absolute path: only repo-confined resolution is permitted.
    if (repoRoot && !inside(resolve(repoRoot), absolute)) return unresolvedCandidate("outside_repo");
    return isFile(absolute)
      ? { ...resolvedCandidate(absolute, "exact"), reason: "absolute" }
      : unresolvedCandidate();
  }
  if (specifier.startsWith(".") || specifier.startsWith("/")) {
    const extensions = isTypeScript ? TS_EXTENSIONS : EXTENSIONS;
    const result = tryResolveFile(dirname(fromFile), specifier, extensions);
    if (repoRoot && result.candidates.some((candidate) => !inside(resolve(repoRoot), candidate))) {
      return unresolvedCandidate("outside_repo");
    }
    return result.status === "RESOLVED" ? { ...result, reason: "relative" } : result;
  }
  // Bare specifier: resolve through node_modules from the file's directory.
  const nodeModules = findNodeModules(dirname(fromFile));
  if (!nodeModules) return unresolvedCandidate("no_node_modules");
  const parts = specifier.split("/");
  const packageName = specifier.startsWith("@") ? `${parts[0]}/${parts[1]}` : parts[0];
  const packageDir = join(nodeModules, packageName);
  const packageJson = join(packageDir, "package.json");
  if (existsSync(packageJson)) {
    let pkg;
    try { pkg = JSON.parse(readFileSync(packageJson, "utf8")); }
    catch { return unresolvedCandidate("invalid_package_json"); }
    const entry = pkg.module ?? pkg.main ?? "index.js";
    const result = tryResolveFile(packageDir, entry, [".js", ".mjs", ".cjs", ".json"]);
    if (result.status === "AMBIGUOUS") return result;
    if (result.resolved) return repoRoot && !inside(resolve(repoRoot), result.resolved)
      ? unresolvedCandidate("outside_repo")
      : { ...result, reason: "package" };
  }
  const direct = tryResolveFile(nodeModules, specifier, [".js", ".mjs", ".cjs", ".json"]);
  if (repoRoot && direct.candidates.some((candidate) => !inside(resolve(repoRoot), candidate))) {
    return unresolvedCandidate("outside_repo");
  }
  return direct.status === "RESOLVED" ? { ...direct, reason: "bare" } : direct;
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
