import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

// The release generation is baked into the engine at compile time via
// `option_env!("MEMBRANE_SOURCE_TREE_SHA256")` (engine/crates/membrane-runtime/src/release_identity.rs).
// RightKit strips arbitrary ambient build environment, so build-frontend writes
// this identity outside the hashed engine subtree & membrane-runtime/build.rs
// validates it before exporting compile-time values. Without that input the binary reports `sha256:unknown` forever,
// the gateway refuses to fan out, and every packet ships empty. Computing the
// identity here — one implementation, two consumers (the sidecar build and the
// release manifest writer) — is what keeps the baked value and the manifest from
// disagreeing.
//
// Clean tree: the committed blob identity, byte-identical to the documented
// release procedure, so a release build is reproducible from the commit alone.
// Dirty tree: the working-tree content identity, because that is what actually
// got compiled. A development build must still be able to state what it is.

const SUBTREE = "engine";

function git(repoRoot, args) {
  return execFileSync("git", ["-C", repoRoot, ...args], {
    encoding: "utf8",
    maxBuffer: 256 * 1024 * 1024,
  });
}

function committedTreeDigest(repoRoot, commit) {
  const rows = [];
  for (const line of git(repoRoot, ["ls-tree", "-r", commit, "--", SUBTREE]).split("\n")) {
    if (!line) continue;
    const [metadata, path] = line.split("\t");
    rows.push([path.replace(/\\/g, "/"), metadata.trim().split(/\s+/)[2]]);
  }
  rows.sort((left, right) => (left[0] < right[0] ? -1 : left[0] > right[0] ? 1 : 0));
  const digest = createHash("sha256");
  for (const [path, blob] of rows) {
    digest.update(`${path}\0${blob}\n`, "utf8");
  }
  return { digest: digest.digest("hex"), fileCount: rows.length };
}

function workingTreeDigest(repoRoot) {
  const listed = git(repoRoot, [
    "ls-files", "--cached", "--others", "--exclude-standard", "--", SUBTREE,
  ]).split("\n").filter(Boolean);
  const paths = [...new Set(listed.map((path) => path.replace(/\\/g, "/")))];
  paths.sort((left, right) => (left < right ? -1 : left > right ? 1 : 0));
  const digest = createHash("sha256");
  let counted = 0;
  for (const path of paths) {
    let content;
    try {
      content = readFileSync(new URL(path, `file://${repoRoot}/`));
    } catch {
      continue; // deleted-but-tracked: absent from the compiled bytes
    }
    const blob = createHash("sha256").update(content).digest("hex");
    digest.update(`${path}\0${blob}\n`, "utf8");
    counted += 1;
  }
  return { digest: digest.digest("hex"), fileCount: counted };
}

export function engineReleaseIdentity(repoRoot) {
  const commit = git(repoRoot, ["rev-parse", "HEAD"]).trim();
  const dirty = git(repoRoot, [
    "status", "--porcelain", "--untracked-files=all", "--", SUBTREE,
  ]).trim().length > 0;
  const { digest, fileCount } = dirty
    ? workingTreeDigest(repoRoot)
    : committedTreeDigest(repoRoot, commit);
  return {
    commit,
    dirty,
    fileCount,
    sourceTreePath: SUBTREE,
    sourceTreeSha256: digest,
    releaseGeneration: `sha256:${digest}`,
  };
}
