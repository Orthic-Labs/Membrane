import { isAbsolute, win32 } from "node:path";

// Ignore prefixes are repository-relative strings, never filesystem paths.
// Keep their prefix semantics (including a trailing slash) identical across
// Phase 1 and graph freshness, but reject input that could escape the repo.
export function normalizeIgnoredPrefixes(value) {
  if (!Array.isArray(value)) return [];
  const accepted = new Set();
  for (const raw of value) {
    if (typeof raw !== "string") continue;
    const prefix = raw.trim().replaceAll("\\", "/");
    if (!prefix || prefix.includes("\0") || prefix.startsWith("/") || isAbsolute(prefix) || win32.isAbsolute(prefix)) continue;
    const parts = prefix.split("/");
    if (parts.some((part) => part === "." || part === ".." || part === "" && part !== parts.at(-1))) continue;
    accepted.add(prefix);
  }
  return [...accepted].sort();
}

export function pathMatchesIgnoredPrefix(repoRelativePath, prefixes) {
  const path = String(repoRelativePath ?? "").replaceAll("\\", "/");
  return normalizeIgnoredPrefixes(prefixes).some((prefix) => path.startsWith(prefix));
}
