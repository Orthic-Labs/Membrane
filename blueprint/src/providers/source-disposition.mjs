import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

function normalizePath(value) {
  return String(value ?? "").replaceAll("\\", "/").replace(/^\.\//, "");
}

function trackedPaths(root) {
  const result = spawnSync("git", ["-C", root, "ls-files", "-z", "--cached", "--"], {
    encoding: "buffer",
    maxBuffer: 512 * 1024 * 1024,
    windowsHide: true,
  });
  if (result.status !== 0 || !Buffer.isBuffer(result.stdout)) return null;
  return result.stdout.toString("utf8").split("\0").map(normalizePath).filter(Boolean).sort();
}

function pathSegments(path) {
  return normalizePath(path).split("/").filter(Boolean);
}

const POLICY_SEGMENTS = new Set([
  ".git", ".agent", ".audit", ".cache", ".next", ".nuxt", ".output", ".parcel-cache", ".pytest_cache",
  ".svelte-kit", ".turbo", ".vercel", ".worktrees", "__pycache__", ".gradle", ".idea", ".mypy_cache",
  ".ruff_cache", ".tox", ".vscode", ".yarn", ".pnpm-store", "coverage", "htmlcov", "node_modules",
  "target", "dist", "build", "out", "vendor", ".serverless", "fixture-repos",
]);

function classifyAbsent(root, path) {
  if (pathSegments(path).some((segment) => POLICY_SEGMENTS.has(segment)) || /^\.agent(?:-|$)/.test(pathSegments(path)[0] ?? "")) {
    return { disposition: "ignored_policy", reason: "primary_scan_exclusion" };
  }
  const absolute = join(root, path);
  if (!existsSync(absolute)) return { disposition: "failed", reason: "tracked_path_missing" };
  try {
    const stat = statSync(absolute);
    if (!stat.isFile()) return { disposition: "ignored_policy", reason: "not_regular_file" };
    if (stat.size > 2 * 1024 * 1024) return { disposition: "ignored_policy", reason: "file_too_large", size: stat.size };
    const bytes = readFileSync(absolute);
    if (bytes.includes(0)) return { disposition: "rejected", reason: "binary_nul_in_text_source", size: stat.size };
  } catch (error) {
    return { disposition: "failed", reason: "source_read_failed", detail: String(error?.code ?? error?.message ?? error) };
  }
  return { disposition: "unsupported", reason: "not_admitted_by_primary_scan" };
}

/**
 * Audits the exact tracked source universe used by the default clean-clone
 * build and gives every tracked path a terminal outcome. Successful indexed
 * paths are summarized rather than duplicated; exceptions remain per-path and
 * inspectable. Non-git roots report admitted-only scope instead of claiming
 * exhaustive discovery they cannot prove.
 */
export function auditSourceDispositions(root, admittedFiles = []) {
  const admitted = new Set((admittedFiles ?? []).map((file) => normalizePath(file.path)).filter(Boolean));
  const tracked = trackedPaths(root);
  if (tracked === null) {
    return Object.freeze({
      schemaVersion: 1,
      scope: "admitted_only",
      complete: true,
      considered: admitted.size,
      indexed: admitted.size,
      exceptions: Object.freeze([]),
    });
  }

  const exceptions = [];
  let indexed = 0;
  for (const path of tracked) {
    if (admitted.has(path)) {
      indexed += 1;
      continue;
    }
    exceptions.push(Object.freeze({ path, ...classifyAbsent(root, path) }));
  }
  exceptions.sort((left, right) => left.path.localeCompare(right.path) || left.disposition.localeCompare(right.disposition));
  const terminal = indexed + exceptions.length;
  return Object.freeze({
    schemaVersion: 1,
    scope: "git_tracked",
    complete: terminal === tracked.length,
    considered: tracked.length,
    indexed,
    terminal,
    counts: Object.freeze(Object.fromEntries(
      ["ignored_policy", "unsupported", "rejected", "failed"].map((kind) => [kind, exceptions.filter((entry) => entry.disposition === kind).length]),
    )),
    exceptions: Object.freeze(exceptions),
  });
}
