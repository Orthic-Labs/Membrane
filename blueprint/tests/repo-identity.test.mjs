import assert from "node:assert/strict";
import { mkdtempSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { normalizeGitOrigin, repositoryIdentity } from "../src/graph/static-provider.mjs";

test("normalizeGitOrigin accepts HTTPS, SSH shorthand, and ssh://git@ URLs", () => {
  assert.deepEqual(normalizeGitOrigin("https://github.com/owner/repo.git"), { host: "github.com", owner: "owner", repo: "repo" });
  assert.deepEqual(normalizeGitOrigin("https://github.com/owner/repo"), { host: "github.com", owner: "owner", repo: "repo" });
  assert.deepEqual(normalizeGitOrigin("git@github.com:owner/repo.git"), { host: "github.com", owner: "owner", repo: "repo" });
  assert.deepEqual(normalizeGitOrigin("ssh://git@github.com/owner/repo.git"), { host: "github.com", owner: "owner", repo: "repo" });
  assert.deepEqual(normalizeGitOrigin("git://github.com/owner/repo.git"), { host: "github.com", owner: "owner", repo: "repo" });
  // Multi-segment owner paths survive normalization.
  assert.deepEqual(normalizeGitOrigin("https://gitlab.com/group/sub/repo.git"), { host: "gitlab.com", owner: "group/sub", repo: "repo" });
});

test("normalizeGitOrigin returns null on unparseable or empty input", () => {
  assert.equal(normalizeGitOrigin(""), null);
  assert.equal(normalizeGitOrigin("not a url"), null);
  assert.equal(normalizeGitOrigin(null), null);
});

test("repositoryIdentity derives repoId from host/owner/repo, not from the absolute path", () => {
  // Two clones of the SAME remote repo at different absolute paths MUST
  // produce the same repoId. This is the regression the path-derived
  // xxh128(root+origin) form had.
  const rootA = mkdtempSync(join(tmpdir(), "blueprint-identity-a-"));
  const rootB = mkdtempSync(join(tmpdir(), "blueprint-identity-b-"));
  const origin = "https://example.com/acme/widgets.git";
  try {
    initFakeRepo(rootA, origin);
    initFakeRepo(rootB, origin);
    const idA = repositoryIdentity(rootA);
    const idB = repositoryIdentity(rootB);
    assert.equal(idA.repoId, idB.repoId, "same origin at different paths must produce the same repoId");
    // repoRoot is canonicalized, so compare against the resolved path: on macOS
    // `tmpdir()` is the /var -> /private/var symlink and the raw mkdtemp path never matches.
    assert.equal(idA.repoRoot, realpathSync(rootA).replaceAll("\\", "/"));
    assert.equal(idA.originHost, "example.com");
    assert.equal(idA.originOwner, "acme");
    assert.equal(idA.originRepo, "widgets");
  } finally {
    rmSync(rootA, { recursive: true, force: true });
    rmSync(rootB, { recursive: true, force: true });
  }
});

test("repositoryIdentity synthesizes a stable local id for repos without a remote", () => {
  const root = mkdtempSync(join(tmpdir(), "blueprint-identity-local-"));
  try {
    // No git remote configured.
    const id = repositoryIdentity(root);
    assert.ok(id.repoId.startsWith("xxh128:"));
    // A second call on the same repo without an environment override
    // produces the same id (process-stable local fallback).
    const id2 = repositoryIdentity(root);
    assert.equal(id.repoId, id2.repoId);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("repositoryIdentity honours BLUEPRINT_LOCAL_REPO_ID as a stable local identity override", () => {
  const root = mkdtempSync(join(tmpdir(), "blueprint-identity-env-"));
  const previous = process.env.BLUEPRINT_LOCAL_REPO_ID;
  try {
    initFakeRepo(root, "https://example.com/acme/widgets.git");
    process.env.BLUEPRINT_LOCAL_REPO_ID = "synthetic-tenant";
    const idA = repositoryIdentity(root);
    const idB = repositoryIdentity(root);
    assert.equal(idA.repoId, idB.repoId);
    // Origin-derived id must NOT change because the env override is the
    // fallback path; with an origin present the host/owner/repo id wins.
    assert.notEqual(idA.repoId, `xxh128:${Buffer.from("synthetic-tenant").toString("hex")}`);
  } finally {
    if (previous === undefined) delete process.env.BLUEPRINT_LOCAL_REPO_ID;
    else process.env.BLUEPRINT_LOCAL_REPO_ID = previous;
    rmSync(root, { recursive: true, force: true });
  }
});

function initFakeRepo(root, origin) {
  spawnSync("git", ["init", "--quiet", "--initial-branch=main", root]);
  spawnSync("git", ["-C", root, "remote", "add", "origin", origin]);
  writeFileSync(join(root, "README.md"), "# fake\n");
  // Make sure canonical paths are realpath-resolvable (mkdtempSync already is).
  spawnSync("git", ["-C", root, "add", "README.md"]);
  spawnSync("git", ["-C", root, "-c", "user.email=test@example.com", "-c", "user.name=test", "commit", "--quiet", "-m", "init"]);
}
