// Canonical git base-commit + working-tree observation, shared by the build
// path (scripts/blueprint.mjs — records what a generation was indexed AT) and
// the freshness-receipt path (src/graph/freshness-receipt.mjs — observes what
// is on disk NOW). Both sides MUST run the identical `git status` invocation
// and hash construction, or an unchanged worktree could still report
// `changed_since_generation` purely from a formatting drift between two
// independently-written git observers. There is exactly one implementation.

import { execFileSync } from "node:child_process";
import { createXXHash128 } from "hash-wasm";

const xxhasher = await createXXHash128();

function xxh3Hex(value) {
  xxhasher.init();
  xxhasher.update(value);
  return xxhasher.digest("hex");
}

/** HEAD commit, or null when unavailable (not a git repo, no commits, git missing). */
export function gitBaseCommit(root) {
  try {
    const value = execFileSync("git", ["rev-parse", "HEAD"], {
      cwd: root,
      encoding: "utf8",
      timeout: 5000,
      windowsHide: true,
      stdio: ["ignore", "pipe", "ignore"],
    }).trim();
    return /^[0-9a-f]{40,64}$/i.test(value) ? value.toLowerCase() : null;
  } catch {
    return null;
  }
}

/**
 * { head, dirty, statusDigest } for `root`, or null when git is unavailable.
 * `statusDigest` is an xxh3-128 hex digest of the exact porcelain status
 * bytes below — the bounded, cheap worktree fingerprint every freshness
 * comparison in Blueprint is built on.
 */
export function gitSourceObservation(root) {
  const head = gitBaseCommit(root);
  if (!head) return null;
  try {
    const status = execFileSync("git", [
      "status", "--porcelain=v1", "-z", "--untracked-files=all", "--", ".",
      ":(exclude).agent", ":(exclude).agent/**",
      ":(exclude)docs/product.md", ":(exclude)docs/architecture.md",
    ], {
      cwd: root,
      timeout: 5000,
      windowsHide: true,
      stdio: ["ignore", "pipe", "ignore"],
      maxBuffer: 16 * 1024 * 1024,
    });
    return {
      head,
      dirty: status.length > 0,
      statusDigest: xxh3Hex(status),
    };
  } catch {
    return null;
  }
}
