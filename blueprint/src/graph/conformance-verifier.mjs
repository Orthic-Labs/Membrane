function getPath(value, path) {
  return String(path).split(".").reduce((current, segment) => current?.[segment], value);
}

function matchesExpected(actual, expected) {
  if (expected && typeof expected === "object" && !Array.isArray(expected)) {
    if (Object.hasOwn(expected, "$exists")) return expected.$exists ? actual !== undefined && actual !== null : actual === undefined || actual === null;
    if (Object.hasOwn(expected, "$includes")) return Array.isArray(actual) && actual.includes(expected.$includes);
    if (Object.hasOwn(expected, "$containsAll")) return Array.isArray(actual) && expected.$containsAll.every((item) => actual.includes(item));
    if (Object.hasOwn(expected, "$matches")) return new RegExp(expected.$matches).test(String(actual ?? ""));
  }
  return Object.is(actual, expected);
}

function matchesWhere(record, where = {}) {
  return Object.entries(where).every(([path, expected]) => matchesExpected(getPath(record, path), expected));
}

function providerVersions(generation) {
  return (generation?.manifest?.providerComposition?.layers ?? []).map((layer) => ({
    id: layer.id,
    version: layer.version ?? null,
    precisionTier: layer.precisionTier ?? null,
    state: layer.state ?? null,
  }));
}

function assertionSource(generation, type) {
  if (type.includes("node")) return generation.nodes ?? [];
  if (type.includes("edge")) return generation.edges ?? [];
  throw new TypeError(`unknown semantic conformance assertion type: ${type}`);
}

function countPass(matches, assertion) {
  if (assertion.type.endsWith("_absent")) return matches.length === 0;
  const exactly = assertion.count?.exactly;
  if (exactly !== undefined) return matches.length === Number(exactly);
  return matches.length >= Number(assertion.count?.atLeast ?? 1);
}

function explain(assertion, matches, passed) {
  if (passed) return null;
  const expectation = assertion.type.endsWith("_absent")
    ? "no matching facts"
    : assertion.count?.exactly !== undefined
      ? `exactly ${assertion.count.exactly} matching fact(s)`
      : `at least ${assertion.count?.atLeast ?? 1} matching fact(s)`;
  return `${assertion.id}: expected ${expectation}, observed ${matches.length}; where=${JSON.stringify(assertion.where ?? {})}`;
}

/**
 * Deterministic, reviewable semantic conformance verifier.
 *
 * Assertions deliberately describe public fact meaning (node/edge + semantic
 * fields) rather than SQLite rows or provider-private serialization. Positive,
 * negative and ambiguity cases therefore survive storage/index refactors.
 */
export function verifySemanticConformance(generation, fixture = {}) {
  if (!generation || !Array.isArray(generation.nodes) || !Array.isArray(generation.edges)) {
    throw new TypeError("semantic conformance requires a generation with nodes and edges");
  }
  const assertions = Array.isArray(fixture.assertions) ? fixture.assertions : [];
  const results = assertions.map((assertion, index) => {
    if (!assertion?.id) throw new TypeError(`semantic conformance assertion ${index} is missing id`);
    const source = assertionSource(generation, String(assertion.type ?? ""));
    const matches = source.filter((record) => matchesWhere(record, assertion.where));
    const passed = countPass(matches, assertion);
    return Object.freeze({
      id: assertion.id,
      type: assertion.type,
      description: assertion.description ?? null,
      passed,
      matched: matches.length,
      sampleIds: Object.freeze(matches.slice(0, 5).map((record) => record.id)),
      failure: explain(assertion, matches, passed),
    });
  });
  const failures = results.filter((result) => !result.passed);
  return Object.freeze({
    schemaVersion: 1,
    kind: "BlueprintSemanticConformanceReport",
    fixture: fixture.name ?? "unnamed",
    status: failures.length ? "failed" : "passed",
    generationId: generation.manifest?.generationId ?? null,
    generationSchemaVersion: generation.schemaVersion ?? generation.manifest?.schemaVersion ?? null,
    providers: Object.freeze(providerVersions(generation)),
    assertions: Object.freeze(results),
    failures: Object.freeze(failures.map((failure) => failure.failure)),
  });
}

export function assertSemanticConformance(generation, fixture = {}) {
  const report = verifySemanticConformance(generation, fixture);
  if (report.status === "failed") {
    const error = new Error(`semantic conformance failed for ${report.fixture}:\n${report.failures.join("\n")}`);
    error.code = "semantic_conformance_failed";
    error.report = report;
    throw error;
  }
  return report;
}
