import { existsSync, lstatSync, realpathSync } from "node:fs";
import { dirname, isAbsolute, relative, resolve, sep } from "node:path";

export function resolvePhysicalPath(path) {
  const target = resolve(path);
  let ancestor = target;
  while (!existsSync(ancestor)) {
    const parent = dirname(ancestor);
    if (parent === ancestor) return null;
    ancestor = parent;
  }
  if (lstatSync(ancestor).isSymbolicLink()) return null;
  return resolve(realpathSync(ancestor), relative(ancestor, target));
}

export function isConfinedPath(root, path, { allowRoot = false } = {}) {
  if (!isAbsolute(path)) return false;
  const target = resolvePhysicalPath(path);
  if (!target) return false;
  const rel = relative(realpathSync(root), target);
  return (allowRoot || rel.length > 0)
    && rel !== ".."
    && !rel.startsWith(`..${sep}`)
    && !isAbsolute(rel);
}
