// JS/TS module resolver — deterministic repository-local ESM/CJS/TypeScript
// semantics. Exact paths stay exact; modern package/config surfaces are resolved
// only when their on-disk declarations make the target unambiguous.

import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";

const TS_EXTENSIONS = [".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs", ".json"];
const JS_EXTENSIONS = [".js", ".mjs", ".cjs", ".json"];
const CONFIG_NAMES = ["tsconfig.json", "jsconfig.json"];

function isFile(path) {
  try { return statSync(path).isFile(); } catch { return false; }
}

function isDirectory(path) {
  try { return statSync(path).isDirectory(); } catch { return false; }
}

function resolvedCandidate(path, resolutionTier, reason = null, extra = {}) {
  return { resolved: path, status: "RESOLVED", candidates: [path], resolutionTier, ...(reason ? { reason } : {}), ...extra };
}

function ambiguousCandidates(candidates, reason, resolutionTier, extra = {}) {
  return {
    resolved: null,
    status: "AMBIGUOUS",
    reason,
    candidates: [...new Set(candidates)].sort((left, right) => left.localeCompare(right)),
    resolutionTier,
    ...extra,
  };
}

function unresolvedCandidate(reason = "missing", extra = {}) {
  return { resolved: null, status: "UNRESOLVED", reason, candidates: [], ...extra };
}

function externalCandidate(packageName, specifier, reason = "external_package") {
  return {
    resolved: null,
    status: "EXTERNAL",
    reason,
    candidates: [],
    externalPackage: { packageName, specifier },
  };
}

function tryResolveFile(baseDir, specifier, extensions) {
  const candidate = resolve(baseDir, specifier);
  if (isFile(candidate)) return resolvedCandidate(candidate, "exact");

  const extensionCandidates = extensions.map((ext) => `${candidate}${ext}`).filter(isFile);
  if (extensionCandidates.length === 1) return resolvedCandidate(extensionCandidates[0], "extension");
  if (extensionCandidates.length > 1) return ambiguousCandidates(extensionCandidates, "ambiguous_extension", "extension");

  if (isDirectory(candidate)) {
    const index = join(candidate, "index");
    const indexCandidates = extensions.map((ext) => `${index}${ext}`).filter(isFile);
    if (indexCandidates.length === 1) return resolvedCandidate(indexCandidates[0], "index");
    if (indexCandidates.length > 1) return ambiguousCandidates(indexCandidates, "ambiguous_index", "index");
  }
  return unresolvedCandidate();
}

function inside(root, target) {
  const rel = relative(root, target);
  return rel === "" || (!rel.startsWith("..") && !isAbsolute(rel));
}

function stripJsonComments(text) {
  let out = "";
  let inString = false;
  let quote = null;
  let escaped = false;
  for (let i = 0; i < text.length; i += 1) {
    const char = text[i];
    const next = text[i + 1];
    if (inString) {
      out += char;
      if (escaped) escaped = false;
      else if (char === "\\") escaped = true;
      else if (char === quote) { inString = false; quote = null; }
      continue;
    }
    if (char === '"' || char === "'") { inString = true; quote = char; out += char; continue; }
    if (char === "/" && next === "/") {
      while (i < text.length && text[i] !== "\n") i += 1;
      out += "\n";
      continue;
    }
    if (char === "/" && next === "*") {
      i += 2;
      while (i < text.length && !(text[i] === "*" && text[i + 1] === "/")) i += 1;
      i += 1;
      continue;
    }
    out += char;
  }
  return out;
}

function readJsonLike(path) {
  try {
    const text = stripJsonComments(readFileSync(path, "utf8")).replace(/,\s*([}\]])/g, "$1");
    return JSON.parse(text);
  } catch { return null; }
}

function findUp(startDir, names, stopDir = null) {
  let current = resolve(startDir);
  const stop = stopDir ? resolve(stopDir) : null;
  while (true) {
    for (const name of names) {
      const candidate = join(current, name);
      if (isFile(candidate)) return candidate;
    }
    if (stop && current === stop) return null;
    const parent = dirname(current);
    if (parent === current || (stop && !inside(stop, parent))) return null;
    current = parent;
  }
}

function splitPackageSpecifier(specifier) {
  const parts = specifier.split("/");
  const packageName = specifier.startsWith("@") ? `${parts[0]}/${parts[1]}` : parts[0];
  const offset = specifier.startsWith("@") ? 2 : 1;
  return { packageName, subpath: parts.slice(offset).join("/") };
}

function matchPattern(pattern, value) {
  if (!pattern.includes("*")) return pattern === value ? { matched: true, star: "" } : { matched: false, star: null };
  const [prefix, suffix] = pattern.split("*");
  if (!value.startsWith(prefix) || !value.endsWith(suffix)) return { matched: false, star: null };
  return { matched: true, star: value.slice(prefix.length, value.length - suffix.length) };
}

function substituteStar(target, star) {
  return typeof target === "string" ? target.replaceAll("*", star ?? "") : target;
}

function selectConditionalTarget(value, conditions, star = "") {
  if (typeof value === "string") return substituteStar(value, star);
  if (Array.isArray(value)) {
    for (const entry of value) {
      const selected = selectConditionalTarget(entry, conditions, star);
      if (selected) return selected;
    }
    return null;
  }
  if (!value || typeof value !== "object") return null;
  const active = new Set(conditions);
  for (const [condition, target] of Object.entries(value)) {
    if (condition !== "default" && !active.has(condition)) continue;
    const selected = selectConditionalTarget(target, conditions, star);
    if (selected) return selected;
  }
  return null;
}

function selectExportsTarget(exportsField, subpath, conditions) {
  const requestKey = subpath ? `./${subpath}` : ".";
  if (typeof exportsField === "string" || Array.isArray(exportsField)) {
    return subpath ? null : selectConditionalTarget(exportsField, conditions);
  }
  if (!exportsField || typeof exportsField !== "object") return null;
  const keys = Object.keys(exportsField);
  const isSubpathMap = keys.some((key) => key.startsWith("."));
  if (!isSubpathMap) return subpath ? null : selectConditionalTarget(exportsField, conditions);
  if (Object.hasOwn(exportsField, requestKey)) return selectConditionalTarget(exportsField[requestKey], conditions);
  const wildcardMatches = keys
    .filter((key) => key.includes("*"))
    .map((key) => ({ key, ...matchPattern(key, requestKey) }))
    .filter((item) => item.matched)
    .sort((left, right) => right.key.length - left.key.length);
  if (!wildcardMatches.length) return null;
  const match = wildcardMatches[0];
  return selectConditionalTarget(exportsField[match.key], conditions, match.star);
}

function selectImportsTarget(importsField, specifier, conditions) {
  if (!importsField || typeof importsField !== "object") return null;
  if (Object.hasOwn(importsField, specifier)) return selectConditionalTarget(importsField[specifier], conditions);
  const wildcardMatches = Object.keys(importsField)
    .filter((key) => key.includes("*"))
    .map((key) => ({ key, ...matchPattern(key, specifier) }))
    .filter((item) => item.matched)
    .sort((left, right) => right.key.length - left.key.length);
  if (!wildcardMatches.length) return null;
  const match = wildcardMatches[0];
  return selectConditionalTarget(importsField[match.key], conditions, match.star);
}

function resolveDeclaredTarget(packageDir, target, extensions, reason, resolutionTier) {
  if (typeof target !== "string" || !target.startsWith("./")) return unresolvedCandidate(`${reason}_invalid_target`);
  const result = tryResolveFile(packageDir, target, extensions);
  if (result.status === "RESOLVED") return { ...result, reason, resolutionTier };
  if (result.status === "AMBIGUOUS") return { ...result, reason: `${reason}_ambiguous`, resolutionTier };
  return unresolvedCandidate(`${reason}_missing_target`);
}

function resolvePackageDirectory(packageDir, subpath, extensions, conditions) {
  const packageJson = join(packageDir, "package.json");
  const pkg = isFile(packageJson) ? readJsonLike(packageJson) : null;
  if (pkg?.exports !== undefined) {
    const target = selectExportsTarget(pkg.exports, subpath, conditions);
    if (!target) return unresolvedCandidate("package_exports_unresolved");
    return resolveDeclaredTarget(packageDir, target, extensions, "package_exports", "package_exports");
  }
  if (subpath) {
    const direct = tryResolveFile(packageDir, `./${subpath}`, extensions);
    return direct.status === "RESOLVED" ? { ...direct, reason: "package_subpath" } : direct;
  }
  const entry = pkg?.module ?? pkg?.main ?? "index.js";
  const result = tryResolveFile(packageDir, entry, extensions);
  return result.status === "RESOLVED" ? { ...result, reason: "package" } : result;
}

function pathAliasCandidates(specifier, configPath, compilerOptions, extensions) {
  const paths = compilerOptions?.paths;
  if (!paths || typeof paths !== "object") return [];
  const configDir = dirname(configPath);
  const baseDir = resolve(configDir, compilerOptions.baseUrl ?? ".");
  const matches = Object.entries(paths)
    .map(([pattern, targets]) => ({ pattern, targets, ...matchPattern(pattern, specifier) }))
    .filter((item) => item.matched)
    .sort((left, right) => right.pattern.replace("*", "").length - left.pattern.replace("*", "").length);
  if (!matches.length) return [];
  const bestSpecificity = matches[0].pattern.replace("*", "").length;
  const resolved = [];
  for (const match of matches.filter((item) => item.pattern.replace("*", "").length === bestSpecificity)) {
    for (const target of Array.isArray(match.targets) ? match.targets : []) {
      const result = tryResolveFile(baseDir, substituteStar(target, match.star), extensions);
      if (result.status === "RESOLVED") resolved.push(result.resolved);
      else if (result.status === "AMBIGUOUS") resolved.push(...result.candidates);
    }
  }
  return [...new Set(resolved)];
}

function resolveTypeScriptConfig(specifier, fromFile, repoRoot, extensions) {
  const configPath = findUp(dirname(fromFile), CONFIG_NAMES, repoRoot);
  if (!configPath) return null;
  const config = readJsonLike(configPath);
  if (!config) return null;
  const compilerOptions = config.compilerOptions ?? {};
  const aliased = pathAliasCandidates(specifier, configPath, compilerOptions, extensions);
  if (aliased.length === 1) return resolvedCandidate(aliased[0], "tsconfig_paths", "tsconfig_paths", { configPath });
  if (aliased.length > 1) return ambiguousCandidates(aliased, "ambiguous_tsconfig_paths", "tsconfig_paths", { configPath });
  if (compilerOptions.baseUrl) {
    const baseDir = resolve(dirname(configPath), compilerOptions.baseUrl);
    const result = tryResolveFile(baseDir, specifier, extensions);
    if (result.status === "RESOLVED") return { ...result, reason: "tsconfig_base_url", resolutionTier: "tsconfig_base_url", configPath };
    if (result.status === "AMBIGUOUS") return { ...result, reason: "ambiguous_tsconfig_base_url", resolutionTier: "tsconfig_base_url", configPath };
  }
  return null;
}

function workspacePatterns(pkg) {
  if (Array.isArray(pkg?.workspaces)) return pkg.workspaces;
  if (Array.isArray(pkg?.workspaces?.packages)) return pkg.workspaces.packages;
  return [];
}

function expandWorkspacePattern(root, pattern) {
  const normalized = String(pattern).replaceAll("\\", "/").replace(/\/$/, "");
  if (!normalized.includes("*")) return isDirectory(join(root, normalized)) ? [join(root, normalized)] : [];
  const [prefix, suffix] = normalized.split("*");
  const parent = join(root, prefix);
  if (!isDirectory(parent)) return [];
  const directories = [];
  for (const entry of readdirSync(parent, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const candidate = join(parent, `${entry.name}${suffix}`);
    if (isDirectory(candidate)) directories.push(candidate);
  }
  return directories;
}

function findWorkspacePackage(repoRoot, packageName) {
  if (!repoRoot) return null;
  const rootPackageJson = join(resolve(repoRoot), "package.json");
  const rootPackage = isFile(rootPackageJson) ? readJsonLike(rootPackageJson) : null;
  for (const pattern of workspacePatterns(rootPackage)) {
    for (const directory of expandWorkspacePattern(resolve(repoRoot), pattern)) {
      const pkg = readJsonLike(join(directory, "package.json"));
      if (pkg?.name === packageName) return directory;
    }
  }
  return null;
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

function resolvePackageImport(specifier, fromFile, repoRoot, extensions, conditions) {
  const packageJson = findUp(dirname(fromFile), ["package.json"], repoRoot);
  if (!packageJson) return unresolvedCandidate("package_imports_no_package");
  const pkg = readJsonLike(packageJson);
  const target = selectImportsTarget(pkg?.imports, specifier, conditions);
  if (!target) return unresolvedCandidate("package_imports_unresolved");
  if (target.startsWith("./")) return resolveDeclaredTarget(dirname(packageJson), target, extensions, "package_imports", "package_imports");
  return resolveModuleSpecifier({ specifier: target, fromFile, repoRoot, isTypeScript: true, conditions });
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
      for (const match of line.matchAll(pattern)) found.push({ specifier: match[1], line: index + 1 });
    }
  }
  return found.filter((item, index) => found.findIndex((candidate) => candidate.specifier === item.specifier && candidate.line === item.line) === index);
}

export function resolveModuleSpecifier({
  specifier,
  fromFile,
  repoRoot = null,
  isTypeScript = true,
  conditions = null,
}) {
  if (!specifier || !fromFile) return unresolvedCandidate("missing_input");
  const extensions = isTypeScript ? TS_EXTENSIONS : JS_EXTENSIONS;
  const activeConditions = conditions ?? (isTypeScript ? ["types", "import", "node", "default"] : ["import", "node", "default"]);
  const root = repoRoot ? resolve(repoRoot) : null;

  if (isAbsolute(specifier)) {
    const absolute = resolve(specifier);
    if (root && !inside(root, absolute)) return unresolvedCandidate("outside_repo");
    return isFile(absolute) ? resolvedCandidate(absolute, "exact", "absolute") : unresolvedCandidate();
  }

  if (specifier.startsWith(".") || specifier.startsWith("/")) {
    const result = tryResolveFile(dirname(fromFile), specifier, extensions);
    if (root && result.candidates.some((candidate) => !inside(root, candidate))) return unresolvedCandidate("outside_repo");
    return result.status === "RESOLVED" ? { ...result, reason: "relative" } : result;
  }

  if (specifier.startsWith("#")) return resolvePackageImport(specifier, fromFile, root, extensions, activeConditions);

  if (isTypeScript) {
    const configResult = resolveTypeScriptConfig(specifier, fromFile, root, extensions);
    if (configResult) {
      if (root && configResult.candidates.some((candidate) => !inside(root, candidate))) return unresolvedCandidate("outside_repo");
      return configResult;
    }
  }

  const { packageName, subpath } = splitPackageSpecifier(specifier);
  const workspaceDir = findWorkspacePackage(root, packageName);
  if (workspaceDir) {
    const result = resolvePackageDirectory(workspaceDir, subpath, extensions, activeConditions);
    return result.status === "RESOLVED"
      ? { ...result, reason: result.reason === "package" ? "workspace_package" : `workspace_${result.reason}`, workspacePackage: packageName }
      : { ...result, workspacePackage: packageName };
  }

  const nodeModules = findNodeModules(dirname(fromFile));
  if (!nodeModules) return externalCandidate(packageName, specifier, "external_package_unavailable");
  const packageDir = join(nodeModules, packageName);
  if (!isDirectory(packageDir)) return externalCandidate(packageName, specifier, "external_package_unavailable");
  const result = resolvePackageDirectory(packageDir, subpath, extensions, activeConditions);
  if (result.status !== "RESOLVED") return result;

  // node_modules is deliberately outside Blueprint's indexed source universe.
  // With a repo scope available, represent it as an external package identity
  // instead of pretending the resolved file is a local graph node. Preserve
  // legacy path-return behavior for callers that do not provide repoRoot.
  if (root) return externalCandidate(packageName, specifier);
  return result;
}
