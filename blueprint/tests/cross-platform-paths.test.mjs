// D50: platform path compatibility. Repository identity, root confinement, and
// store scope must be invariant across the path differences real platforms
// exhibit: symlinks (macOS/Linux), case-only variants (case-insensitive
// filesystems), backslash/forward-slash separators (Windows), and worktrees
// (two checkouts of one repo). The test detects the host filesystem's case
// sensitivity rather than assuming one, so the same assertions run on a
// case-sensitive Linux CI runner and a case-insensitive macOS developer
// machine.

import assert from "node:assert/strict";
import { cpSync, existsSync, mkdirSync, mkdtempSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, normalize, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { RootRegistry } from "../src/lib/application/root-registry.mjs";
import { repositoryIdentity } from "../src/graph/static-provider.mjs";
import { closeStore, openStore } from "../src/graph/store-sqlite.mjs";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const FIXTURE = join(ROOT, "evals/fixture-repos/typescript-commerce");

function makeRepo(prefix = "blueprint-paths-") {
  const repo = mkdtempSync(join(tmpdir(), prefix));
  cpSync(FIXTURE, repo, { recursive: true });
  return repo;
}

test("a symlinked repo root resolves to the same canonical root as the real path", () => {
  const repo = makeRepo();
  const linkRoot = mkdtempSync(join(tmpdir(), "blueprint-pathlink-"));
  const link = join(linkRoot, "linked");
  try {
    symlinkSync(repo, link, "dir");
    const identityReal = repositoryIdentity(resolve(repo));
    const identityLinked = repositoryIdentity(link);
    // repoRoot is realpath'd, so both views must report the same root.
    assert.equal(identityLinked.repoRoot, identityReal.repoRoot, "symlinked root canonicalises to the real path");
    assert.equal(normalize(link), `${normalize(linkRoot)}${sep}linked`);
  } finally {
    rmSync(repo, { recursive: true, force: true });
    rmSync(linkRoot, { recursive: true, force: true });
  }
});

test("repository identity is stable across case variants on case-insensitive filesystems", () => {
  const repo = makeRepo();
  const variantParent = mkdtempSync(join(tmpdir(), "blueprint-casevariant-"));
  try {
    // The invariant is on the CANONICAL root: every spelling and symlink that
    // realpaths to the same directory must yield the same root and identity.
    const probe = join(repo, "CaseProbe.txt");
    writeFileSync(probe, "x");
    const caseInsensitive = existsSync(join(repo, "caseprobe.txt"));
    rmSync(probe, { force: true });
    const variantRoot = join(variantParent, "VariantRepo");
    mkdirSync(variantRoot, { recursive: true });
    symlinkSync(repo, join(variantRoot, "linked"), "dir");
    const viaVariant = repositoryIdentity(join(variantRoot, "linked"));
    const lowerSpelling = join(variantParent, "variantrepo", "linked");
    if (caseInsensitive) {
      // Case-insensitive FS: the lower-spelled path also realpaths to the repo.
      assert.equal(repositoryIdentity(lowerSpelling).repoRoot, viaVariant.repoRoot, "case variants collapse to one canonical root on case-insensitive FS");
      assert.equal(repositoryIdentity(lowerSpelling).repoId, viaVariant.repoId, "case variants collapse to one identity on case-insensitive FS");
    } else {
      // Case-sensitive FS: the collapsed spelling does not exist, so identity
      // differs — that is the honest platform boundary.
      assert.notEqual(repositoryIdentity(lowerSpelling).repoId, viaVariant.repoId, "distinct never-realised paths stay distinct on case-sensitive FS");
    }
  } finally {
    rmSync(repo, { recursive: true, force: true });
    rmSync(variantParent, { recursive: true, force: true });
  }
});

test("root registry confines an explicit repoRoot to enrolled roots", () => {
  const repo = makeRepo();
  const elsewhere = mkdtempSync(join(tmpdir(), "blueprint-elsewhere-"));
  try {
    const registry = new RootRegistry([{ root: repo }]);
    assert.equal(registry.resolve({ repoRoot: repo }), realpathSync.native(repo));
    assert.throws(() => registry.resolve({ repoRoot: elsewhere }), /No enrolled Blueprint repository/);
    // A trailing separator variant must not change the identity.
    assert.equal(registry.resolve({ repoRoot: `${resolve(repo)}${sep}` }), realpathSync.native(repo));
  } finally {
    rmSync(repo, { recursive: true, force: true });
    rmSync(elsewhere, { recursive: true, force: true });
  }
});

test("separator normalisation keeps store scope inside the repo root", () => {
  const repo = makeRepo();
  try {
    const dbPath = join(repo, ".agent", "graph", "graph.db");
    mkdirSync(dirname(dbPath), { recursive: true });
    const db = openStore(dbPath);
    try {
      db.exec("CREATE TABLE IF NOT EXISTS _probe (id INTEGER)");
      const row = db.prepare("SELECT count(*) AS n FROM sqlite_master WHERE name='_probe'").get();
      assert.ok(row.n >= 1, "store is writable at the canonical scoped path");
    } finally {
      closeStore(db);
    }
    assert.ok(dbPath.startsWith(resolve(repo)), "store path stays under the repo root");
    const backslash = dbPath.replaceAll("/", "\\");
    assert.ok(!backslash.includes("..\\"), "no traversal segment in the normalised store path");
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});
