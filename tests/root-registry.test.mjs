// D06: canonical root registry enrollment and resolution.

import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { RootRegistry } from "../lib/application/root-registry.mjs";
import { CortexError } from "../lib/application/errors.mjs";

function makeRepo(name = "repo-a") {
  const root = mkdtempSync(join(tmpdir(), `cortex-registry-${name}-`));
  mkdirSync(join(root, ".git"), { recursive: true });
  writeFileSync(join(root, "README.md"), `# ${name}\n`);
  return root;
}

test("registry enrolls entries with canonical roots", () => {
  const root = makeRepo();
  try {
    const registry = new RootRegistry([{ root }]);
    const listed = registry.list();
    assert.equal(listed.length, 1);
    assert.equal(listed[0].root, root);
    assert.equal(listed[0].enabled, true);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("resolve by repoId returns the canonical root", () => {
  const root = makeRepo();
  try {
    const registry = new RootRegistry([{ root, repoId: "repo-1" }]);
    assert.equal(registry.resolve({ repoId: "repo-1" }), root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("resolve by exact enrolled root works", () => {
  const root = makeRepo();
  try {
    const registry = new RootRegistry([{ root, repoId: "repo-1" }]);
    assert.equal(registry.resolve({ repoRoot: root }), root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("single enrolled repo resolves without explicit id", () => {
  const root = makeRepo();
  try {
    const registry = new RootRegistry([{ root }]);
    assert.equal(registry.resolve({}), root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("unregistered root raises root_not_enrolled", () => {
  const root = makeRepo();
  const other = makeRepo("other");
  try {
    const registry = new RootRegistry([{ root }]);
    assert.throws(
      () => registry.resolve({ repoRoot: other }),
      (error) => error instanceof CortexError && error.code === "root_not_enrolled",
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
    rmSync(other, { recursive: true, force: true });
  }
});

test("repoId and root pointing at different enrollments raise root_escape", () => {
  const a = makeRepo("a");
  const b = makeRepo("b");
  try {
    const registry = new RootRegistry([
      { root: a, repoId: "repo-a" },
      { root: b, repoId: "repo-b" },
    ]);
    assert.throws(
      () => registry.resolve({ repoId: "repo-a", repoRoot: b }),
      (error) => error instanceof CortexError && error.code === "root_escape",
    );
  } finally {
    rmSync(a, { recursive: true, force: true });
    rmSync(b, { recursive: true, force: true });
  }
});

test("disabled entries are never resolved", () => {
  const root = makeRepo();
  try {
    const registry = new RootRegistry([{ root, repoId: "repo-1", enabled: false }]);
    assert.throws(
      () => registry.resolve({ repoId: "repo-1" }),
      (error) => error instanceof CortexError && error.code === "root_not_enrolled",
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("macOS /var symlink aliases canonicalize to the same root", (t) => {
  if (process.platform !== "darwin") {
    t.skip("darwin-only /var canonicalization");
    return;
  }
  const root = makeRepo();
  try {
    // /var is a symlink to /private/var on macOS. Enrolling the /var alias and
    // resolving the real path (and vice versa) must land on the same root.
    const alias = root.replace(/^\/private\/var/, "/var");
    if (alias === root) {
      t.skip("tmpdir not under /private/var");
      return;
    }
    const registry = new RootRegistry([{ root: alias, repoId: "repo-1" }]);
    assert.equal(registry.resolve({ repoRoot: root }), root);
    const registry2 = new RootRegistry([{ root, repoId: "repo-2" }]);
    assert.equal(registry2.resolve({ repoRoot: alias }), root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("symlink escape is not enrolled", () => {
  const root = makeRepo();
  const outside = makeRepo("outside");
  try {
    const registry = new RootRegistry([{ root, repoId: "repo-1" }]);
    const link = join(root, "escape-link");
    symlinkSync(outside, link);
    // The symlinked path resolves outside the enrolled root -> not enrolled.
    assert.throws(
      () => registry.resolve({ repoRoot: link }),
      (error) => error instanceof CortexError && error.code === "root_not_enrolled",
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
    rmSync(outside, { recursive: true, force: true });
  }
});

test("nested repositories are distinct enrollments", () => {
  const outer = makeRepo("outer");
  const inner = mkdtempSync(join(outer, "inner-"));
  mkdirSync(join(inner, ".git"), { recursive: true });
  writeFileSync(join(inner, "README.md"), "# inner\n");
  try {
    const registry = new RootRegistry([
      { root: outer, repoId: "outer" },
      { root: inner, repoId: "inner" },
    ]);
    assert.equal(registry.resolve({ repoId: "outer" }), outer);
    assert.equal(registry.resolve({ repoId: "inner" }), inner);
    assert.notEqual(registry.resolve({ repoId: "inner" }), outer);
  } finally {
    rmSync(outer, { recursive: true, force: true });
  }
});

test("different worktrees of one repo are separate entries", () => {
  const main = makeRepo("main");
  const wt = mkdtempSync(join(tmpdir(), "cortex-wt-"));
  mkdirSync(join(wt, ".git"), { recursive: true });
  writeFileSync(join(wt, "README.md"), "# wt\n");
  try {
    const registry = new RootRegistry([
      { root: main, repoId: "repo-main", worktreeId: "main" },
      { root: wt, repoId: "repo-wt", worktreeId: "wt" },
    ]);
    assert.equal(registry.resolve({ repoId: "repo-wt" }), wt);
    // A worktree does not inherit the main repo's enrollment.
    assert.throws(
      () => registry.resolve({ repoId: "repo-wt", repoRoot: main }),
      (error) => error instanceof CortexError && error.code === "root_escape",
    );
  } finally {
    rmSync(main, { recursive: true, force: true });
    rmSync(wt, { recursive: true, force: true });
  }
});
