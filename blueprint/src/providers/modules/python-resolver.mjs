import { existsSync, readFileSync } from "node:fs";
import { dirname, isAbsolute, join, normalize, relative, resolve } from "node:path";

export const RESOLVED = "RESOLVED";
export const UNRESOLVED = "UNRESOLVED";

const STDLIB = new Set(`__future__ abc argparse array ast asyncio base64 binascii bisect builtins bz2
calendar cmath collections concurrent configparser contextlib contextvars copy csv ctypes dataclasses
datetime decimal difflib dis email encodings enum errno functools gc getopt getpass gettext glob graphlib
gzip hashlib heapq hmac html http importlib inspect io ipaddress itertools json keyword linecache locale
logging lzma math mimetypes mmap multiprocessing operator os pathlib pdb pickle pkgutil platform pprint
queue random re secrets select selectors shelve shlex shutil signal site socket sqlite3 ssl stat statistics
string struct subprocess sys sysconfig tempfile textwrap threading time timeit tkinter token tokenize tomllib
trace traceback types typing unicodedata unittest urllib uuid venv warnings weakref webbrowser winreg xml zipfile
zlib zoneinfo`.split(/\s+/));

const miss = (reason) => ({ resolved: null, status: UNRESOLVED, reason });
const hit = (resolved, reason) => ({ resolved, status: RESOLVED, reason });
const read = (path) => {
  try { return readFileSync(path, "utf8"); } catch { return null; }
};

export function extractPythonModuleSpecifiers(text) {
  const found = [];
  const lines = String(text ?? "").split(/\r?\n/);
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    const from = line.match(/^\s*from\s+([.A-Za-z_]\w*(?:\.\w+)*)\s+import\s+([A-Za-z_]\w*)/);
    if (from) {
      found.push({ specifier: from[1], importedName: from[2], line: index + 1 });
      continue;
    }
    const imported = line.match(/^\s*import\s+([A-Za-z_]\w*(?:\.\w+)*)/);
    if (imported) found.push({ specifier: imported[1], importedName: null, line: index + 1 });
  }
  return found;
}
const inside = (root, target) => {
  const rel = relative(root, target);
  return rel === "" || (!rel.startsWith("..") && !isAbsolute(rel));
};

function configuredRoots(root) {
  const pyproject = read(join(root, "pyproject.toml"));
  if (pyproject) {
    const packageDir = pyproject.match(/package-dir\s*=\s*\{[^}]*["']?["']\s*=\s*["']([^"']+)["']/)?.[1];
    const findSection = pyproject.match(/\[tool\.setuptools\.packages\.find\]([\s\S]*?)(?=\n\[|$)/)?.[1];
    const where = findSection?.match(/\bwhere\s*=\s*\[?\s*["']([^"']+)["']/)?.[1];
    const selected = packageDir ?? where;
    if (selected) return { roots: [join(root, selected)].filter(existsSync), pyproject, setupCfg: null };
  }
  const setupCfg = read(join(root, "setup.cfg"));
  if (setupCfg) {
    const options = setupCfg.match(/\[options\]([\s\S]*?)(?=\n\[|$)/)?.[1] ?? "";
    const packageDir = options.match(/package_dir\s*=\s*(?:\r?\n\s*)?=\s*([^\s#;]+)/)?.[1];
    const findSection = setupCfg.match(/\[options\.packages\.find\]([\s\S]*?)(?=\n\[|$)/)?.[1];
    const where = findSection?.match(/\bwhere\s*=\s*([^\s#;]+)/)?.[1];
    const selected = packageDir ?? where;
    if (selected) return { roots: [join(root, selected)].filter(existsSync), pyproject: null, setupCfg };
  }
  return { roots: [root], pyproject, setupCfg };
}

export function pythonProjectInfo(repoRoot) {
  if (!repoRoot) return { roots: [], config: null };
  const root = resolve(repoRoot);
  const found = configuredRoots(root);
  return {
    roots: found.roots.map(normalize),
    config: {
      pyproject: found.pyproject ? { source: "pyproject.toml" } : null,
      setupCfg: found.setupCfg ? { source: "setup.cfg" } : null,
    },
  };
}

function directTarget(base, fromFile) {
  const file = `${base}.py`;
  if (existsSync(file)) return hit(file, "module");
  const init = join(base, "__init__.py");
  return existsSync(init) ? hit(init, "package") : null;
}

function namedTarget(packageInit, importedName) {
  const dir = dirname(packageInit);
  const direct = directTarget(join(dir, importedName), packageInit);
  if (direct) return hit(direct.resolved, "package_submodule");
  const text = read(packageInit) ?? "";
  for (const line of text.split(/\r?\n/)) {
    const match = line.match(/^\s*from\s+(\.+)([\w.]*)\s+import\s+(.+)$/);
    if (!match) continue;
    const names = match[3].split(",").map((name) => name.trim().split(/\s+as\s+/)[1] ?? name.trim().split(/\s+as\s+/)[0]);
    if (!names.includes(importedName)) continue;
    let base = dir;
    for (let i = 1; i < match[1].length; i += 1) base = dirname(base);
    const target = directTarget(join(base, ...match[2].split(".").filter(Boolean)), packageInit);
    if (target) return hit(target.resolved, "init_reexport");
  }
  return null;
}

function relativeImport({ clean, dots, fromFile, root, importedName }) {
  let base = dirname(fromFile);
  for (let i = 1; i < dots; i += 1) base = dirname(base);
  if (!inside(root, base)) return miss("relative_above_root");
  const parts = clean.slice(dots).split(".").filter(Boolean);
  const target = parts.length ? directTarget(join(base, ...parts), fromFile) : hit(join(base, "__init__.py"), "package");
  if (!target || !existsSync(target.resolved)) return miss(`missing_relative:${".".repeat(dots)}${parts.join(".")}`);
  if (importedName && target.resolved.endsWith("__init__.py")) {
    return namedTarget(target.resolved, importedName) ?? miss(`missing_relative:${".".repeat(dots)}${parts.join(".")}`);
  }
  return hit(target.resolved, target.resolved.endsWith("__init__.py") ? "relative_package" : "relative_module");
}

export function resolvePythonModule({ specifier, fromFile, repoRoot, importedName }) {
  if (![specifier, fromFile, repoRoot].every((value) => typeof value === "string" && value.trim())) return miss("missing_input");
  const clean = specifier.trim();
  const root = resolve(repoRoot);
  const source = resolve(fromFile);
  const dots = clean.match(/^\.+/)?.[0].length ?? 0;
  if (dots) return relativeImport({ clean, dots, fromFile: source, root, importedName });
  const top = clean.split(".")[0];
  if (!/^[A-Za-z_]\w*$/.test(top)) return miss(`invalid_module_name:${clean}`);
  if (STDLIB.has(top)) return miss(`stdlib:${top}`);
  for (const searchRoot of pythonProjectInfo(root).roots) {
    const target = directTarget(join(searchRoot, ...clean.split(".")), source);
    if (!target) continue;
    if (importedName && target.resolved.endsWith("__init__.py")) {
      return namedTarget(target.resolved, importedName) ?? miss(`not_repo_module:${top}`);
    }
    return hit(target.resolved, "package");
  }
  return miss(`not_repo_module:${top}`);
}

export function readRepoFile(repoRoot, relativePath) {
  const root = resolve(repoRoot);
  const target = resolve(root, relativePath);
  return inside(root, target) ? read(target) : null;
}
