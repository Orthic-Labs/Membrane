// Findings detection — BP001/BP002/BP003 over a repository file set.
//
// Deterministic: no compiler, no language server, no network, no model. Given
// the same file set it returns the same findings in the same order.
//
// The controlling rule is fail-closed: a finding is emitted only when the
// evidence proves it. Everything else becomes an omission with a named reason,
// so "0 findings" and "nothing could be checked" are never the same answer.

import { createHash } from "node:crypto";

import { extractModuleSurface, surfaceIsClosed, surfaceLanguageFor } from "../../graph/module-surface.mjs";
import { FINDING_RULES } from "./registry.mjs";
import { isRelativeSpecifier, normalizeRepoPath, resolveSpecifier } from "./specifier.mjs";

const DEFAULT_STAR_DEPTH = 8;

// Line numbers move whenever anything above them changes. Fingerprinting on
// (rule, file, name, specifier) instead keeps a finding's identity stable
// across unrelated edits, so a baseline stays meaningful and an agent is not
// re-shown the same finding because a comment shifted it down two lines.
function fingerprintOf({ ruleId, path, name, specifier }) {
  return createHash("sha256")
    .update(`${ruleId} ${path} ${name ?? ""} ${specifier ?? ""}`)
    .digest("hex")
    .slice(0, 16);
}

function makeFinding({ rule, path, startLine, message, name, specifier, generationId, evidencePath }) {
  return {
    ruleId: rule.id,
    ruleName: rule.name,
    ruleDescription: rule.description,
    severity: rule.severity,
    class: rule.class,
    confidenceTier: "EXACT_RESOLUTION",
    precisionTier: rule.precisionFloor,
    path,
    startLine,
    endLine: startLine,
    message,
    name,
    specifier,
    evidencePath,
    generationId,
    fingerprint: fingerprintOf({ ruleId: rule.id, path, name, specifier }),
    remediation: rule.remediation,
    helpUri: null,
  };
}

function omission({ path, reason, detail = null, specifier = null, line = null }) {
  return { path, reason, detail, specifier, line };
}

/**
 * Resolve a module's effective export names, following repository-local
 * `export *` chains. Returns null when the surface cannot be enumerated —
 * the caller must then omit rather than report.
 */
function makeEffectiveExports({ surfaces, fileSet, maxStarDepth, omissions }) {
  const cache = new Map();

  function resolve(path, seen, depth) {
    if (cache.has(path)) return cache.get(path);
    const surface = surfaces.get(path);
    if (!surface || !surfaceIsClosed(surface)) return null;

    const names = new Set(surface.exports.map((entry) => entry.name));
    for (const star of surface.starReexports) {
      if (!isRelativeSpecifier(star.specifier)) {
        // `export * from "some-package"` — the surface now includes names this
        // repository cannot see.
        omissions.push(omission({ path, reason: "open_export_surface", detail: "unfollowable_star_reexport", specifier: star.specifier, line: star.line }));
        return null;
      }
      if (depth >= maxStarDepth) {
        omissions.push(omission({ path, reason: "star_depth_exceeded", specifier: star.specifier, line: star.line }));
        return null;
      }
      const { resolved } = resolveSpecifier(path, star.specifier, fileSet);
      if (!resolved) {
        omissions.push(omission({ path, reason: "open_export_surface", detail: "unresolved_star_target", specifier: star.specifier, line: star.line }));
        return null;
      }
      if (seen.has(resolved)) {
        omissions.push(omission({ path, reason: "star_cycle", specifier: star.specifier, line: star.line }));
        return null;
      }
      const inherited = resolve(resolved, new Set([...seen, resolved]), depth + 1);
      if (!inherited) {
        omissions.push(omission({ path, reason: "open_export_surface", detail: "star_target_open", specifier: star.specifier, line: star.line }));
        return null;
      }
      // ESM: `export *` re-exports every name EXCEPT default.
      for (const name of inherited) if (name !== "default") names.add(name);
    }
    cache.set(path, names);
    return names;
  }

  return (path) => resolve(path, new Set([path]), 0);
}

/**
 * @param {{
 *   files: Array<{path: string, text: string}>,
 *   generationId?: string|null,
 *   existsOutsideScan?: ((fromPath: string, specifier: string) => boolean)|null,
 *   maxStarDepth?: number,
 * }} input
 */
export async function detectFindings({ files, generationId = null, existsOutsideScan = null, maxStarDepth = DEFAULT_STAR_DEPTH }) {
  const normalized = (files ?? []).map((file) => ({ path: normalizeRepoPath(file.path), text: String(file.text ?? "") }));
  const fileSet = new Set(normalized.map((file) => file.path));
  const omissions = [];
  const surfaces = new Map();

  // The tree-sitter parser instance is shared and cached per language, so
  // extraction is sequential by construction — concurrent parse() calls on one
  // parser would interleave.
  let parsed = 0;
  for (const file of normalized) {
    if (!surfaceLanguageFor(file.path)) {
      omissions.push(omission({ path: file.path, reason: "unsupported_language" }));
      continue;
    }
    const surface = await extractModuleSurface(file);
    surfaces.set(file.path, surface);
    if (surface.parseStatus === "failed") omissions.push(omission({ path: file.path, reason: "parse_failed" }));
    else parsed += 1;
  }

  const effectiveExports = makeEffectiveExports({ surfaces, fileSet, maxStarDepth, omissions });
  const findings = [];

  for (const file of normalized) {
    const surface = surfaces.get(file.path);
    if (!surface || surface.parseStatus !== "ok") continue;

    for (const request of surface.requests) {
      if (!isRelativeSpecifier(request.specifier)) {
        omissions.push(omission({ path: file.path, reason: "package_specifier", specifier: request.specifier, line: request.line }));
        continue;
      }
      const { resolved } = resolveSpecifier(file.path, request.specifier, fileSet);
      if (!resolved) {
        // A specifier that resolves on disk but was never scanned (ignored
        // prefix, submodule, generated tree) is a gap in coverage, not a
        // broken import. Claiming BP002 there would be a false positive.
        if (existsOutsideScan?.(file.path, request.specifier)) {
          omissions.push(omission({ path: file.path, reason: "outside_scanned_set", specifier: request.specifier, line: request.line }));
          continue;
        }
        findings.push(makeFinding({
          rule: FINDING_RULES.BP002,
          path: file.path,
          startLine: request.line,
          name: null,
          specifier: request.specifier,
          generationId,
          evidencePath: [file.path],
          message: `"${request.specifier}" resolves to no file in the repository`,
        }));
        continue;
      }

      const targetSurface = surfaces.get(resolved);
      if (!targetSurface || targetSurface.parseStatus === "unsupported") {
        omissions.push(omission({ path: file.path, reason: "unsupported_language", specifier: request.specifier, detail: resolved, line: request.line }));
        continue;
      }
      if (targetSurface.parseStatus !== "ok") {
        omissions.push(omission({ path: file.path, reason: "parse_failed", specifier: request.specifier, detail: resolved, line: request.line }));
        continue;
      }
      const exported = effectiveExports(resolved);
      if (!exported) {
        omissions.push(omission({
          path: file.path,
          reason: "open_export_surface",
          specifier: request.specifier,
          detail: `${resolved}: ${[...new Set(targetSurface.open.map((entry) => entry.reason))].sort().join(",") || "star_reexport"}`,
          line: request.line,
        }));
        continue;
      }
      if (exported.has(request.name)) continue;

      const rule = request.kind === "reexport" ? FINDING_RULES.BP003 : FINDING_RULES.BP001;
      const available = [...exported].sort();
      const shown = available.slice(0, 6).join(", ");
      const suffix = available.length > 6 ? `, +${available.length - 6} more` : "";
      const verb = request.kind === "reexport" ? "re-exports" : "imports";
      findings.push(makeFinding({
        rule,
        path: file.path,
        startLine: request.line,
        name: request.name,
        specifier: request.specifier,
        generationId,
        evidencePath: [file.path, resolved],
        message: available.length
          ? `${verb} { ${request.name} } from "${request.specifier}" — that module exports { ${shown}${suffix} } only`
          : `${verb} { ${request.name} } from "${request.specifier}" — that module exports nothing`,
      }));
    }
  }

  findings.sort((left, right) =>
    left.path.localeCompare(right.path)
    || left.startLine - right.startLine
    || left.ruleId.localeCompare(right.ruleId)
    || String(left.name).localeCompare(String(right.name)));

  return {
    schemaVersion: 1,
    generationId,
    findings,
    omissions,
    coverage: {
      filesScanned: normalized.length,
      filesParsed: parsed,
      surfacesClosed: [...surfaces.values()].filter((surface) => surfaceIsClosed(surface)).length,
      omissionCount: omissions.length,
    },
  };
}
