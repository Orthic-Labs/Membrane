import { readFileSync, statSync } from "node:fs";
import { contentDigest } from "./generation-identity.mjs";

// The largest file the graph will treat as source. The full-build readers have
// enforced exactly this bound for as long as they have existed
// (sources/_shared.mjs, sources/live-overlay.mjs, sources/task-anchor.mjs,
// graph/static-provider.mjs); the incremental watcher path was the one reader
// that never did, which is why `blueprint build` succeeded on a tree whose
// watcher crashed. Reading an arbitrarily large file is not merely slow: past
// ~512 MB the mandatory Buffer→utf8 conversion throws
// "Cannot create a string longer than 0x1fffffe8 characters", and that throw
// lands inside the journal-apply write transaction, leaving the store
// "malformed database schema" — a corrupt graph, not just a skipped file.
export const MAX_SOURCE_FILE_BYTES = 2 * 1024 * 1024;

function fileIdentity(stat) {
  return Number.isFinite(Number(stat?.dev)) && Number.isFinite(Number(stat?.ino))
    ? `${stat.dev}:${stat.ino}`
    : null;
}

function changed(before, after) {
  return before.size !== after.size || before.mtimeMs !== after.mtimeMs;
}

export function stableRead(absPath, fs = { readFileSync, statSync }) {
  let statBefore = fs.statSync(absPath);
  let bytes = fs.readFileSync(absPath);
  let statAfter = fs.statSync(absPath);
  let unstable = changed(statBefore, statAfter);
  if (unstable) {
    statBefore = fs.statSync(absPath);
    bytes = fs.readFileSync(absPath);
    statAfter = fs.statSync(absPath);
    unstable = changed(statBefore, statAfter);
  }
  return {
    bytes,
    contentDigest: contentDigest(bytes),
    fileIdentity: fileIdentity(statAfter),
    statBefore,
    statAfter,
    unstable,
  };
}
