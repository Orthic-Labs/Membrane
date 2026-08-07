import assert from "node:assert/strict";
import test from "node:test";

function cliName(id) {
  return id.toLowerCase().replaceAll(".", "-");
}

function checkParity(operations, parser) {
  const expectedNames = new Set(operations.map(({ id }) => cliName(id)));
  const inventory = parser("") ?? [];
  const report = {
    total_ops: operations.length,
    reachable: 0,
    missing: [],
    extra: inventory.map(([name]) => name).filter((name) => !expectedNames.has(name)),
    flag_drift: [],
  };

  for (const operation of operations) {
    const name = cliName(operation.id);
    const actualPairs = parser(name);
    if (actualPairs === undefined) {
      report.missing.push(name);
      continue;
    }
    report.reachable += 1;
    const expected = new Map(
      operation.parameters.map((parameter) => [
        `--${parameter.name}`,
        parameter.default ?? "",
      ]),
    );
    const actual = new Map(actualPairs);
    for (const [flag, expectedDefault] of expected) {
      if (!actual.has(flag)) {
        report.flag_drift.push([name, flag, "<missing>"]);
      } else if (actual.get(flag) !== expectedDefault) {
        report.flag_drift.push([name, `${flag}=${expectedDefault}`, `${flag}=${actual.get(flag)}`]);
      }
    }
    for (const [flag, actualDefault] of actual) {
      if (!expected.has(flag)) report.flag_drift.push([name, "<none>", `${flag}=${actualDefault}`]);
    }
  }
  return report;
}

const operations = [{
  id: "Context.Read",
  parameters: [{ name: "limit", default: "10", help: "Maximum result count." }],
}];

function parser(entries) {
  const commands = new Map(entries);
  return (name) => name === ""
    ? [...commands.keys()].map((command) => [command, ""])
    : commands.get(name);
}

test("empty input round-trips as an empty JSON report", () => {
  const roundTrip = JSON.parse(JSON.stringify(checkParity([], parser([]))));
  assert.equal(roundTrip.total_ops, 0);
  assert.equal(roundTrip.reachable, 0);
  assert.deepEqual(roundTrip.missing, []);
  assert.deepEqual(roundTrip.extra, []);
  assert.deepEqual(roundTrip.flag_drift, []);
});

test("all registry operations are reachable", () => {
  const report = checkParity(operations, parser([
    ["context-read", [["--limit", "10"]]],
  ]));
  assert.equal(report.reachable, report.total_ops);
  assert.equal(report.missing.length, 0);
});

test("a parser-only subcommand is reported as extra", () => {
  const report = checkParity(operations, parser([
    ["context-read", [["--limit", "10"]]],
    ["orphan", []],
  ]));
  assert.ok(report.extra.length > 0);
  assert.deepEqual(report.extra, ["orphan"]);
});

test("a mismatched flag default is reported as flag drift", () => {
  const report = checkParity(operations, parser([
    ["context-read", [["--limit", "25"]]],
  ]));
  assert.ok(report.flag_drift.length > 0);
  assert.deepEqual(report.flag_drift[0], ["context-read", "--limit=10", "--limit=25"]);
});
