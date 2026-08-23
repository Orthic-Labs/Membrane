// Repository-relative specifier resolution for findings.
//
// Mirrors the candidate ladder already used by graph/language-extractors.mjs's
// extractJavaScriptImports so a findings verdict can never disagree with the
// graph's own file-level import edges. Kept separate because findings resolve
// against an explicit file set and must report WHY a specifier missed, not
// merely that it did.

const SOURCE_EXTENSIONS = ["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];
const ASSET_EXTENSIONS = ["json", "vue", "astro"];

export function isRelativeSpecifier(specifier) {
  return typeof specifier === "string" && specifier.startsWith(".");
}

export function normalizeRepoPath(value) {
  return String(value ?? "").replaceAll("\\", "/");
}

/**
 * Ordered resolution candidates for a relative specifier.
 * Priority: exact path, then TypeScript's `.js` -> source rewrite and bare
 * extension completion, then directory index files.
 */
export function candidatePaths(fromPath, specifier) {
  const baseDir = normalizeRepoPath(fromPath).split("/").slice(0, -1);
  const raw = [...baseDir, ...specifier.split("/")].filter((part) => part && part !== ".");
  const parts = [];
  for (const part of raw) part === ".." ? parts.pop() : parts.push(part);
  const exact = parts.join("/");
  // TS resolves `./m.js` to `m.ts`; stripping a JS extension reproduces that
  // without needing tsconfig, and leaves non-JS extensions (`.json`) intact.
  const stem = exact.replace(/\.(js|jsx|mjs|cjs)$/, "");
  const candidates = [exact];
  for (const extension of [...SOURCE_EXTENSIONS, ...ASSET_EXTENSIONS]) candidates.push(`${stem}.${extension}`);
  for (const extension of SOURCE_EXTENSIONS) candidates.push(`${stem}/index.${extension}`);
  return [...new Set(candidates)];
}

/**
 * @param {string} fromPath repository-relative path of the importing file
 * @param {string} specifier the import specifier as written
 * @param {Set<string>} fileSet repository-relative paths that were scanned
 * @returns {{resolved: string|null, candidates: string[], alternatives: number}}
 */
export function resolveSpecifier(fromPath, specifier, fileSet) {
  const candidates = candidatePaths(fromPath, specifier);
  const matches = candidates.filter((candidate) => fileSet.has(candidate));
  return { resolved: matches[0] ?? null, candidates, alternatives: Math.max(0, matches.length - 1) };
}
