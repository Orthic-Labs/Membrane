import { existsSync, linkSync, renameSync, rmSync } from "node:fs";

function backupPathFor(target) {
  return `${target}.previous-${process.pid}-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

/** Replace one closed file, retaining an immediately recoverable prior inode until validation passes. */
export function adoptFileAtomically(source, target, validate, options = {}) {
  const platform = options.platform ?? process.platform;
  const exists = options.exists ?? existsSync;
  const link = options.link ?? linkSync;
  const rename = options.rename ?? renameSync;
  const remove = options.remove ?? ((path) => rmSync(path, { force: true }));
  const backup = backupPathFor(target);
  const hadTarget = exists(target);
  let targetMoved = false;
  let backupCreated = false;

  try {
    if (hadTarget) {
      if (platform === "win32") {
        rename(target, backup);
        targetMoved = true;
      } else {
        link(target, backup);
        backupCreated = true;
      }
    }
    rename(source, target);
    validate(target);
    if (hadTarget) remove(backup);
    return Object.freeze({ target, replaced: hadTarget });
  } catch (error) {
    if (hadTarget && (backupCreated || targetMoved) && exists(backup)) {
      if (exists(target)) remove(target);
      rename(backup, target);
    }
    throw error;
  } finally {
    if (exists(backup)) remove(backup);
  }
}
