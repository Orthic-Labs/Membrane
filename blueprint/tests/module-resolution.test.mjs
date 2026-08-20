import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { pythonProjectInfo, resolvePythonModule } from "../src/providers/modules/python-resolver.mjs";

function withRepo(run) {
  const root = mkdtempSync(join(tmpdir(), "blueprint-pyresolve-"));
  const write = (relative, content = "") => {
    const target = join(root, relative);
    mkdirSync(join(target, ".."), { recursive: true });
    writeFileSync(target, content);
    return target;
  };
  try { run({ root, write, path: (...parts) => join(root, ...parts) }); }
  finally { rmSync(root, { recursive: true, force: true }); }
}

function expectResolved(result, path, reason) {
  assert.equal(result.status, "RESOLVED");
  assert.equal(result.resolved, path);
  if (reason) assert.equal(result.reason, reason);
}

function expectUnresolved(result, reason) {
  assert.equal(result.status, "UNRESOLVED");
  assert.equal(result.resolved, null);
  assert.match(result.reason, reason instanceof RegExp ? reason : new RegExp(`^${reason}$`));
}

test("python-resolver: package __init__.py resolves", () => withRepo((repo) => {
  const init = repo.write("pkg/__init__.py");
  const nested = repo.write("pkg/sub/__init__.py");
  const from = repo.write("app.py", "import pkg");
  expectResolved(resolvePythonModule({ specifier: "pkg", fromFile: from, repoRoot: repo.root }), init, "package");
  expectResolved(resolvePythonModule({ specifier: "pkg.sub", fromFile: from, repoRoot: repo.root }), nested);
}));

test("python-resolver: top-level module.py resolves", () => withRepo((repo) => {
  const mod = repo.write("pkg/mod.py", "VALUE = 1");
  const from = repo.write("app.py", "from pkg.mod import VALUE");
  expectResolved(resolvePythonModule({ specifier: "pkg.mod", fromFile: from, repoRoot: repo.root }), mod, "package");
  expectUnresolved(resolvePythonModule({ specifier: "pkg.nope", fromFile: from, repoRoot: repo.root }), /^not_repo_module:/);
}));

test("python-resolver: pyproject.toml package-dir src root", () => withRepo((repo) => {
  repo.write("pyproject.toml", `[tool.setuptools]\npackage-dir = {"" = "src"}\n`);
  const init = repo.write("src/mypkg/__init__.py");
  const mod = repo.write("src/mypkg/util.py");
  const from = repo.write("src/mypkg/main.py");
  assert.deepEqual(pythonProjectInfo(repo.root).roots, [repo.path("src")]);
  expectResolved(resolvePythonModule({ specifier: "mypkg", fromFile: from, repoRoot: repo.root }), init);
  expectResolved(resolvePythonModule({ specifier: "mypkg.util", fromFile: from, repoRoot: repo.root }), mod);
}));

test("python-resolver: pyproject.toml packages.find where src root", () => withRepo((repo) => {
  repo.write("pyproject.toml", `[tool.setuptools.packages.find]\nwhere = ["src"]\n`);
  const init = repo.write("src/acme/__init__.py");
  const from = repo.write("src/acme/main.py");
  assert.deepEqual(pythonProjectInfo(repo.root).roots, [repo.path("src")]);
  expectResolved(resolvePythonModule({ specifier: "acme", fromFile: from, repoRoot: repo.root }), init);
}));

test("python-resolver: setup.cfg package_dir src root", () => withRepo((repo) => {
  repo.write("setup.cfg", `[options]\npackage_dir =\n    = src\n`);
  const init = repo.write("src/box/__init__.py");
  const from = repo.write("src/box/main.py");
  assert.deepEqual(pythonProjectInfo(repo.root).roots, [repo.path("src")]);
  expectResolved(resolvePythonModule({ specifier: "box", fromFile: from, repoRoot: repo.root }), init);
}));

test("python-resolver: setup.cfg packages.find where src root", () => withRepo((repo) => {
  repo.write("setup.cfg", `[options]\npackages = find:\n\n[options.packages.find]\nwhere = src\n`);
  const init = repo.write("src/crate/__init__.py");
  const from = repo.write("src/crate/main.py");
  assert.deepEqual(pythonProjectInfo(repo.root).roots, [repo.path("src")]);
  expectResolved(resolvePythonModule({ specifier: "crate", fromFile: from, repoRoot: repo.root }), init);
}));

test("python-resolver: relative imports resolve & never escape root", () => withRepo((repo) => {
  const pkg = repo.write("pkg/__init__.py");
  const sub = repo.write("pkg/sub/__init__.py");
  const from = repo.write("pkg/sub/inner.py");
  const helper = repo.write("pkg/pkg_helper.py");
  expectResolved(resolvePythonModule({ specifier: "..", fromFile: from, repoRoot: repo.root }), pkg, "relative_package");
  expectResolved(resolvePythonModule({ specifier: "..pkg_helper", fromFile: from, repoRoot: repo.root }), helper, "relative_module");
  expectResolved(resolvePythonModule({ specifier: "..sub", fromFile: from, repoRoot: repo.root }), sub, "relative_package");
  expectUnresolved(resolvePythonModule({ specifier: "..ghost", fromFile: from, repoRoot: repo.root }), /^missing_relative:/);
  expectUnresolved(resolvePythonModule({ specifier: "....up", fromFile: from, repoRoot: repo.root }), "relative_above_root");
}));

test("python-resolver: from . import x resolves named module", () => withRepo((repo) => {
  repo.write("pkg/__init__.py");
  const helper = repo.write("pkg/helper.py");
  const from = repo.write("pkg/mod.py");
  expectResolved(resolvePythonModule({ specifier: ".", importedName: "helper", fromFile: from, repoRoot: repo.root }), helper, "package_submodule");
}));

test("python-resolver: __init__.py re-export target", () => withRepo((repo) => {
  const impl = repo.write("pkg/impl.py", "def public_api(): pass");
  repo.write("pkg/__init__.py", "from .impl import public_api");
  const from = repo.write("app.py");
  expectResolved(resolvePythonModule({ specifier: "pkg", importedName: "public_api", fromFile: from, repoRoot: repo.root }), impl, "init_reexport");
}));

test("python-resolver: dotted __init__.py re-export target", () => withRepo((repo) => {
  const impl = repo.write("pkg/sub/impl.py", "def thing(): pass");
  repo.write("pkg/__init__.py", "from .sub.impl import thing");
  const from = repo.write("app.py");
  expectResolved(resolvePythonModule({ specifier: "pkg", importedName: "thing", fromFile: from, repoRoot: repo.root }), impl, "init_reexport");
}));

test("python-resolver: __init__.py re-export never guesses", () => withRepo((repo) => {
  repo.write("pkg/__init__.py", "from .impl import public_api");
  repo.write("pkg/impl.py");
  const from = repo.write("app.py");
  expectUnresolved(resolvePythonModule({ specifier: "pkg", importedName: "something_else", fromFile: from, repoRoot: repo.root }), /^not_repo_module:/);
}));

test("python-resolver: stdlib & third-party remain unresolved", () => withRepo((repo) => {
  const from = repo.write("app.py");
  for (const name of ["os", "pathlib", "sys", "json", "typing"]) {
    expectUnresolved(resolvePythonModule({ specifier: name, fromFile: from, repoRoot: repo.root }), `stdlib:${name}`);
  }
  expectUnresolved(resolvePythonModule({ specifier: "requests.auth", fromFile: from, repoRoot: repo.root }), "not_repo_module:requests");
}));

test("python-resolver: ignores site-packages outside repository", () => withRepo((repo) => {
  const from = repo.write("app.py");
  const outside = mkdtempSync(join(tmpdir(), "blueprint-pyresolve-outside-"));
  try {
    mkdirSync(join(outside, "site-packages/numpy"), { recursive: true });
    writeFileSync(join(outside, "site-packages/numpy/__init__.py"), "");
    expectUnresolved(resolvePythonModule({ specifier: "numpy", fromFile: from, repoRoot: repo.root }), "not_repo_module:numpy");
  } finally { rmSync(outside, { recursive: true, force: true }); }
}));

test("python-resolver: missing inputs degrade typed", () => withRepo((repo) => {
  const from = repo.write("app.py");
  for (const args of [{ specifier: "", fromFile: from, repoRoot: repo.root }, { specifier: "pkg", fromFile: "", repoRoot: repo.root }, { specifier: "pkg", fromFile: from, repoRoot: "" }]) {
    expectUnresolved(resolvePythonModule(args), "missing_input");
  }
}));

test("python-resolver: exact self import resolves", () => withRepo((repo) => {
  const from = repo.write("pkg/mod.py");
  expectResolved(resolvePythonModule({ specifier: "pkg.mod", fromFile: from, repoRoot: repo.root }), from);
}));

test("python-resolver: pythonProjectInfo reports config source", () => withRepo((repo) => {
  repo.write("pyproject.toml", `[tool.setuptools]\npackage-dir = {"" = "src"}\n`);
  mkdirSync(repo.path("src"));
  const info = pythonProjectInfo(repo.root);
  assert.deepEqual(info.roots, [repo.path("src")]);
  assert.ok(info.config.pyproject);
  assert.equal(info.config.setupCfg, null);
}));
