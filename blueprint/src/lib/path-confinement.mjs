import { existsSync, realpathSync } from "node:fs";
import { dirname, isAbsolute, relative, resolve, sep } from "node:path";

// Resolve `path` to its physical location, following symlinks in the nearest
// existing ancestor while keeping the (possibly not-yet-existing) tail intact.
// Returns null when no existing ancestor can be resolved (e.g. a dangling
// symlink), so callers fail closed rather than guessing a location.
export function resolvePhysicalPath(path) {
  const target = resolve(path);
  let ancestor = target;
  while (!existsSync(ancestor)) {
    const parent = dirname(ancestor);
    if (parent === ancestor) return null;
    ancestor = parent;
  }
  try {
    return resolve(realpathSync(ancestor), relative(ancestor, target));
  } catch {
    return null;
  }
}

// Containment check against the canonical (symlink-resolved) root. A path is
// confined only when its physical location stays inside the physical root.
// This accepts a symlinked repo root (canonicalised on both sides) while still
// rejecting symlink/junction targets that resolve outside the root.
export function isConfinedPath(root, path, { allowRoot = false } = {}) {
  if (!isAbsolute(path)) return false;
  try {
    const canonicalRoot = realpathSync(root);
    const target = resolvePhysicalPath(path);
    if (!target) return false;
    const rel = relative(canonicalRoot, target);
    return (allowRoot || rel.length > 0)
      && rel !== ".."
      && !rel.startsWith(`..${sep}`)
      && !isAbsolute(rel);
  } catch {
    return false;
  }
}
