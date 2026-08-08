import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const contractPath = "packaging/homebrew/release.v1.json";
const caskPath = "packaging/homebrew/Casks/membrane.rb";
const formulaPath = "packaging/homebrew/Formula/membrane.rb";

const read = (path) => readFile(path, "utf8");

test("Homebrew metadata remains unavailable until immutable signed release evidence exists", async () => {
  const contract = JSON.parse(await read(contractPath));
  assert.equal(contract.schema, "orthic.membrane.homebrew-release.v1");
  assert.equal(contract.state, "unavailable");
  assert.equal(contract.publish, false);
  assert.deepEqual(Object.keys(contract.platforms).sort(), ["mac-arm64", "mac-x64"]);
  assert.equal(contract.sourceIdentity.version, null);
  assert.equal(contract.sourceIdentity.releaseGeneration, null);
  assert.equal(contract.cask.artifact, "dmg");
  for (const target of ["mac-arm64", "mac-x64"]) {
    assert.equal(contract.cask.artifacts[target].url, null);
    assert.equal(contract.cask.artifacts[target].sha256, null);
    assert.equal(contract.formula.artifacts[target].url, null);
    assert.equal(contract.formula.artifacts[target].sha256, null);
  }
  assert.deepEqual(contract.cask.requiredReceipts, ["codesign", "notary", "staple"]);
  assert.equal(contract.formula.artifact, "prebuilt-cli-daemon-tarball");
});

test("Cask & Formula fail before Homebrew can fetch or install placeholders", async () => {
  const [cask, formula] = await Promise.all([read(caskPath), read(formulaPath)]);
  for (const source of [cask, formula]) {
    assert.match(source, /intentionally unavailable/);
    assert.doesNotMatch(source, /\burl\s+['"]/);
    assert.doesNotMatch(source, /sha256\s+:no_check/);
    assert.doesNotMatch(source, /version\s+:latest/);
  }
  assert.match(cask, /codesign, notarization, & staple/);
  assert.match(formula, /CLI\/daemon URL/);
});
