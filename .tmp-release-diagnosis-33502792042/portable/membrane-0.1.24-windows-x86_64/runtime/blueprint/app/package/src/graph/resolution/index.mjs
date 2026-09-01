// Exact-first resolution owner — the ONLY place scanned-file resolution lives.
//
// Closed-surface rule: a negative finding (BP001/BP002) is emitted ONLY when
// the target module's export surface is provably CLOSED. Every other case —
// unsupported extension, ambiguous candidates, partial parse, dynamic import,
// generated file, or external/bare specifier — collapses to a typed omission
// {code:"resolution_unsupported", detail, reason} rather than a finding. This
// keeps "0 findings" distinguishable from "nothing could be checked".
//
// Reasons that force a typed omission (all map to code "resolution_unsupported"):
//   unsupported  — file extension outside the JS/TS+asset set
//   ambiguous    — multiple candidates match (caller must not guess)
//   partial      — parseStatus !== "ok" (open surface)
//   dynamic      — dynamic import(), require(), or non-literal specifier
//   generated    — path matches a generated/generated-tree prefix
//   external     — bare/package specifier (no leading ".")
//
// Scanned-file resolution is exact-first: the specifier as written is tried
// first, then the TypeScript `.js -> source` rewrite + extension completion,
// then directory index files. First match wins; alternatives are counted for
// evidence but never preferred.

const SOURCE_EXTENSIONS = ["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];
const ASSET_EXTENSIONS = ["json", "vue", "astro"];

export const RESOLUTION_OMISSION_CODE = "resolution_unsupported";
export const SUPPORTED_RESOLUTION_EXTENSIONS = Object.freeze([...SOURCE_EXTENSIONS, ...ASSET_EXTENSIONS].sort());

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

/**
 * Build a typed omission for any case where the surface is not closed.
 * Closed-surface rule: negatives only when surface closed; unsupported/
 * ambiguous/partial/dynamic/generated/external → typed omission.
 *
 * @param {{detail?: string|null, reason?: string|null, path?: string|null, specifier?: string|null, line?: number|null}} input
 * @returns {{code: "resolution_unsupported", detail: string|null, reason: string|null, path: string|null, specifier: string|null, line: number|null}}
 */
export function resolutionUnsupportedOmission({ detail = null, reason = null, path = null, specifier = null, line = null } = {}) {
  const validReasons = new Set(["unsupported", "ambiguous", "partial", "dynamic", "generated", "external"]);
  const normalizedReason = reason != null && validReasons.has(String(reason)) ? String(reason) : (reason != null ? String(reason) : null);
  return {
    code: RESOLUTION_OMISSION_CODE,
    detail: detail != null ? String(detail) : null,
    reason: normalizedReason,
    path: path != null ? String(path) : null,
    specifier: specifier != null ? String(specifier) : null,
    line: line != null ? Number(line) : null,
  };
}

/**
 * Classify a specifier + target surface for closed-surface gating.
 * Returns null when the surface is closed (caller may emit a finding);
 * otherwise returns a typed omission.
 */
export function classifyResolution({ specifier, targetSurface, parseStatus, isGenerated = false } = {}) {
  if (isGenerated) return resolutionUnsupportedOmission({ detail: "generated_source", reason: "generated", specifier });
  if (specifier != null && !isRelativeSpecifier(specifier)) return resolutionUnsupportedOmission({ detail: "bare_specifier", reason: "external", specifier });
  if (parseStatus === "partial" || parseStatus === "failed") return resolutionUnsupportedOmission({ detail: String(parseStatus), reason: "partial", specifier });
  if (targetSurface && typeof targetSurface === "object" && Array.isArray(targetSurface.open) && targetSurface.open.length > 0) {
    return resolutionUnsupportedOmission({ detail: targetSurface.open.map((e) => e.reason).join(","), reason: "partial", specifier });
  }
  // Unsupported extension: not in SOURCE+ASSET set and not a directory index case already handled
  if (specifier != null && /\.[a-z0-9]+$/i.test(specifier)) {
    const ext = specifier.slice(specifier.lastIndexOf(".") + 1).toLowerCase();
    if (!SUPPORTED_RESOLUTION_EXTENSIONS.includes(ext) && ext !== "js" && ext !== "jsx" && ext !== "mjs" && ext !== "cjs") {
      // Keep as typed omission — caller will map to unsupported_language if needed
      return resolutionUnsupportedOmission({ detail: `unsupported_extension:${ext}`, reason: "unsupported", specifier });
    }
  }
  return null;
}
