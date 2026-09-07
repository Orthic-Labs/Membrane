function normalizePath(value) { return String(value ?? "").replaceAll("\\", "/").replace(/^\.\//, ""); }

function namingStyle(name) {
  const stem = String(name).replace(/\.[^.]+$/, "");
  if (/^[a-z][a-z0-9]*(?:-[a-z0-9]+)+$/.test(stem)) return "kebab-case";
  if (/^[a-z][a-z0-9]*(?:_[a-z0-9]+)+$/.test(stem)) return "snake_case";
  if (/^[a-z][A-Za-z0-9]*$/.test(stem) && /[A-Z]/.test(stem)) return "camelCase";
  if (/^[A-Z][A-Za-z0-9]*$/.test(stem)) return "PascalCase";
  return "other";
}

function weakEvidence(kind, claim, support, total, examples, counterexamples) {
  return {
    kind,
    evidenceClass: "WeakEvidence",
    claim,
    support,
    total,
    coverage: total ? support / total : 0,
    examples: examples.slice(0, 8),
    counterexamples: counterexamples.slice(0, 8),
    policyAuthority: false,
  };
}

function isTestPath(path) {
  return /(^|\/)(?:tests?|__tests__)(\/|$)|(?:^|[._-])(?:test|spec)\.[^.]+$/i.test(path);
}

/** Descriptive convention mining only. Counterexamples are first-class. */
export function detectProjectConventions(files = [], { minimumExamples = 3, minimumCoverage = 0.75 } = {}) {
  const paths = files.map((file) => normalizePath(file.path)).filter(Boolean);
  const evidence = [];

  // Production filename conventions and test-placement conventions are distinct
  // populations. Mixing *.test/spec paths into ordinary source naming makes the
  // inferred production style depend on test framework suffixes rather than the
  // repository's source convention.
  const sourcePaths = paths.filter((path) => /\.[A-Za-z0-9]+$/.test(path) && !isTestPath(path));
  const styles = new Map();
  for (const path of sourcePaths) {
    const name = path.split("/").at(-1);
    const style = namingStyle(name);
    if (!styles.has(style)) styles.set(style, []);
    styles.get(style).push(path);
  }
  const rankedStyles = [...styles.entries()].sort((a, b) => b[1].length - a[1].length || a[0].localeCompare(b[0]));
  if (rankedStyles.length && rankedStyles[0][1].length >= minimumExamples) {
    const [style, examples] = rankedStyles[0];
    const counterexamples = rankedStyles.slice(1).flatMap(([, rows]) => rows);
    const row = weakEvidence("file_naming", `Most source files use ${style}.`, examples.length, sourcePaths.length, examples, counterexamples);
    if (row.coverage >= minimumCoverage) evidence.push(row);
  }

  const testPaths = paths.filter(isTestPath);
  if (testPaths.length >= minimumExamples) {
    const directoryTests = testPaths.filter((path) => /(^|\/)(?:tests?|__tests__)(\/|$)/i.test(path));
    const colocated = testPaths.filter((path) => !directoryTests.includes(path));
    const preferred = directoryTests.length >= colocated.length ? directoryTests : colocated;
    const counter = preferred === directoryTests ? colocated : directoryTests;
    const mode = preferred === directoryTests ? "dedicated test directories" : "co-located test/spec files";
    const row = weakEvidence("test_placement", `Tests are usually placed in ${mode}.`, preferred.length, testPaths.length, preferred, counter);
    if (row.coverage >= minimumCoverage) evidence.push(row);
  }

  const topDirectories = new Map();
  for (const path of paths) {
    const first = path.includes("/") ? path.split("/")[0] : ".";
    topDirectories.set(first, (topDirectories.get(first) ?? 0) + 1);
  }
  const commonDirs = [...topDirectories.entries()].filter(([, count]) => count >= minimumExamples).sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  if (commonDirs.length) evidence.push(weakEvidence(
    "module_layout",
    `Common top-level code areas: ${commonDirs.slice(0, 5).map(([dir]) => dir).join(", ")}.`,
    commonDirs.reduce((sum, [, count]) => sum + count, 0),
    paths.length,
    commonDirs.slice(0, 8).map(([dir, count]) => `${dir}:${count}`),
    [],
  ));

  return Object.freeze({
    schemaVersion: 1,
    kind: "project-conventions",
    evidenceClass: "WeakEvidence",
    policyAuthority: false,
    evidence: Object.freeze(evidence),
  });
}
