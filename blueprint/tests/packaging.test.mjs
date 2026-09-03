import assert from "node:assert/strict";
import { readFileSync, existsSync, statSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "..");
const pkg = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8"));

test("package.json exposes standalone version, bin, engines, and files", () => {
  assert.equal(pkg.name, "@membrane/blueprint");
  assert.equal(pkg.bin?.blueprint, "./scripts/blueprint.mjs");
  assert.match(String(pkg.version), /^\d+\.\d+\.\d+/);
  assert.equal(pkg.type, "module");
  assert.ok(pkg.engines?.node);
  assert.match(pkg.engines.node, />=22\.22\.3/);
  assert.ok(existsSync(join(ROOT, "scripts/blueprint.mjs")));
  assert.ok(Array.isArray(pkg.files));
  for (const required of ["scripts/", "src/graph/", "src/lib/", "src/sources/", "schemas/"]) {
    assert.ok(pkg.files.includes(required), `files must include ${required}`);
  }
  assert.equal(pkg.exports?.["./admission"], "./src/lib/admission.mjs");
  assert.ok(pkg.scripts?.test);
  assert.ok(pkg.scripts?.["test:workspace"]);
  assert.doesNotMatch(pkg.scripts.test, /workspace/);
  assert.match(pkg.scripts["test:workspace"], /workspace/);
});

test("admission library and schemas ship in the package surface", () => {
  assert.ok(existsSync(join(ROOT, "src/lib/admission.mjs")));
  assert.ok(existsSync(join(ROOT, "src/lib/receipt-store.mjs")));
  assert.ok(existsSync(join(ROOT, "src/lib/orientation-evidence.mjs")));
  assert.ok(existsSync(join(ROOT, "schemas/blueprint-admission-v1.schema.json")));
});

test("every src/ module reachable from a bin entry point is covered by files[]", () => {
  // Regression for a packaging defect where src/providers/ was omitted from
  // package.json `files[]`: the published tarball was missing a directory
  // imported by src/graph/static-provider.mjs, so `blueprint graph build`
  // died with ERR_MODULE_NOT_FOUND on every installed build. This walks the
  // real static import graph from every `bin` entry point and asserts each
  // reached src/<dir>/ has a matching files[] entry.
  const binEntries = Object.values(pkg.bin).map((rel) => join(ROOT, rel));
  assert.ok(binEntries.length > 0, "package.json must declare bin entries");

  const importRe =
    /(?:import|export)\s+(?:[^'"]*?from\s+)?['"](\.[^'"]+)['"]|import\(\s*['"](\.[^'"]+)['"]\s*\)|require\(\s*['"](\.[^'"]+)['"]\s*\)/g;

  function isFile(candidate) {
    try {
      return statSync(candidate).isFile();
    } catch {
      return false;
    }
  }

  function resolveSpecifier(fromFile, spec) {
    const base = join(dirname(fromFile), spec);
    const candidates = [base, `${base}.mjs`, `${base}.js`, join(base, "index.mjs"), join(base, "index.js")];
    return candidates.find((candidate) => isFile(candidate));
  }

  const visited = new Set();
  const srcDirs = new Set();

  function walk(file) {
    const norm = file;
    if (visited.has(norm) || !existsSync(norm)) return;
    visited.add(norm);
    const content = readFileSync(norm, "utf8");
    let match;
    importRe.lastIndex = 0;
    while ((match = importRe.exec(content))) {
      const spec = match[1] || match[2] || match[3];
      if (!spec) continue;
      const resolved = resolveSpecifier(norm, spec);
      if (resolved) walk(resolved);
    }
    const rel = norm.slice(ROOT.length + 1).replace(/\\/g, "/");
    if (rel.startsWith("src/")) {
      const parts = rel.split("/");
      srcDirs.add(`${parts[0]}/${parts[1]}/`);
    }
  }

  for (const entry of binEntries) walk(entry);

  assert.ok(srcDirs.size > 0, "expected the import walk to reach at least one src/ directory");
  for (const dir of srcDirs) {
    assert.ok(
      pkg.files.includes(dir),
      `files[] omits ${dir}, which is imported (directly or transitively) from a bin entry point`,
    );
  }
});
