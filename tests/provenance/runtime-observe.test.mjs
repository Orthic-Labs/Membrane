// MBR-502: runtime observe() — Node test runner.
//
// Two invariants exercised here:
//
// 1. The bridge's `captureWorkingTreeJs` returns a WorkingTreeSnapshotV1
//    with the expected fields when the bridge is driven with a stub
//    `gitCommand` that emits fake git output. The stub is injected via
//    the bridge's optional argument so the suite never depends on a
//    real `git` binary on the box.
//
// 2. `observeJs` appends exactly one JSONL line per call to the
//    journal under a tmp dir; five calls produce five lines.

import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  captureWorkingTreeJs,
  diffNumstat,
  headSnapshot,
  observeJs,
  recordProvenanceJs,
  statusPaths,
} from "./bridge.mjs";

const FAKE_HEAD = "abc1234567890abcdef1234567890abcdef12345";
const FAKE_PORCELAIN = " M src/lib.rs\n?? adapters/provenance/index.mjs\n";
const FAKE_NUMSTAT = "12\t3\tsrc/lib.rs\n0\t0\tCargo.lock\n";

function makeStubGit() {
  const calls = [];
  const gitCommand = (_workspace, args) => {
    calls.push(args);
    const sub = args[0];
    if (sub === "rev-parse") {
      return { status: 0, stdout: `${FAKE_HEAD}\n`, stderr: "" };
    }
    if (sub === "status") {
      return { status: 0, stdout: FAKE_PORCELAIN, stderr: "" };
    }
    if (sub === "diff") {
      return { status: 0, stdout: FAKE_NUMSTAT, stderr: "" };
    }
    return { status: 1, stdout: "", stderr: `unexpected: ${sub}` };
  };
  return { calls, gitCommand };
}

test("captureWorkingTreeJs returns a WorkingTreeSnapshotV1 with the expected fields", () => {
  const stub = makeStubGit();
  const workspace = "/tmp/workspace-not-real";
  const snapshot = captureWorkingTreeJs(workspace, 1_700_000_000_000, stub.gitCommand);
  assert.equal(snapshot.schemaVersion, 1);
  assert.equal(snapshot.workspaceRoot, workspace);
  assert.equal(snapshot.gitHead, FAKE_HEAD);
  assert.deepEqual(snapshot.dirtyPaths, [
    "src/lib.rs",
    "adapters/provenance/index.mjs",
  ]);
  assert.equal(snapshot.diffAdded, 12);
  assert.equal(snapshot.diffRemoved, 3);
  assert.equal(snapshot.capturedAtUnixMs, 1_700_000_000_000);
  // Three git subcommands, in the documented order.
  assert.deepEqual(
    stub.calls.map((args) => args[0]),
    ["rev-parse", "status", "diff"],
  );
});

test("headSnapshot returns null when git reports an unborn HEAD", () => {
  const unbornStub = (_workspace, args) => {
    if (args[0] !== "rev-parse") {
      return { status: 1, stdout: "", stderr: "not rev-parse" };
    }
    return {
      status: 128,
      stdout: "",
      stderr: "fatal: ambiguous argument 'HEAD': unknown revision",
    };
  };
  assert.equal(headSnapshot("/tmp/workspace", unbornStub), null);
});

test("statusPaths and diffNumstat return parsed data", () => {
  const stub = makeStubGit();
  assert.deepEqual(statusPaths("/tmp/workspace", stub.gitCommand), [
    "src/lib.rs",
    "adapters/provenance/index.mjs",
  ]);
  assert.deepEqual(diffNumstat("/tmp/workspace", stub.gitCommand), {
    added: 12,
    removed: 3,
  });
});

test("observeJs appends exactly one JSONL line per call", () => {
  const dir = mkdtempSync(join(tmpdir(), "membrane-mbr502-js-"));
  try {
    const journal = join(dir, "provenance.jsonl");
    const stub = makeStubGit();
    const row = observeJs({
      operation: "read",
      clientId: "claude",
      scopeGrant: "scope-xyz",
      workspace: "/tmp/workspace",
      installationId: "00000000-0000-4000-8000-0000000000aa",
      storePath: journal,
      nowUnixMs: 1_700_000_000_500,
      gitCommand: stub.gitCommand,
    });
    assert.equal(row.operation, "read");
    assert.equal(row.clientId, "claude");
    assert.equal(row.scopeGrant, "scope-xyz");
    assert.equal(row.schemaVersion, 1);
    assert.equal(row.recordedAtUnixMs, 1_700_000_000_500);
    assert.equal(row.workingTree.gitHead, FAKE_HEAD);
    const raw = readFileSync(journal, "utf8");
    const lines = raw.split("\n").filter((line) => line.length > 0);
    assert.equal(lines.length, 1);
    const parsed = JSON.parse(lines[0]);
    assert.equal(parsed.operation, "read");
    assert.equal(parsed.workingTree.diffAdded, 12);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("five successive observeJs calls produce five journal lines", () => {
  const dir = mkdtempSync(join(tmpdir(), "membrane-mbr502-js-5"));
  try {
    const journal = join(dir, "provenance.jsonl");
    const stub = makeStubGit();
    for (let i = 0; i < 5; i += 1) {
      observeJs({
        operation: "read",
        clientId: "claude",
        scopeGrant: null,
        workspace: "/tmp/workspace",
        installationId: "iid",
        storePath: journal,
        nowUnixMs: 1_700_000_000_600 + i,
        gitCommand: stub.gitCommand,
      });
    }
    const raw = readFileSync(journal, "utf8");
    const lines = raw.split("\n").filter((line) => line.length > 0);
    assert.equal(lines.length, 5);
    const parsed = lines.map((line) => JSON.parse(line));
    assert.deepEqual(
      parsed.map((row) => row.recordedAtUnixMs),
      [
        1_700_000_000_600,
        1_700_000_000_601,
        1_700_000_000_602,
        1_700_000_000_603,
        1_700_000_000_604,
      ],
    );
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("recordProvenanceJs creates parent directories on demand", () => {
  const dir = mkdtempSync(join(tmpdir(), "membrane-mbr502-js-mkdir-"));
  try {
    const deep = join(dir, "nested", "sub", "provenance.jsonl");
    const row = {
      schemaVersion: 1,
      installationId: "iid",
      operation: "install",
      clientId: null,
      scopeGrant: null,
      workingTree: {
        schemaVersion: 1,
        workspaceRoot: "/tmp/workspace",
        gitHead: null,
        dirtyPaths: [],
        diffAdded: 0,
        diffRemoved: 0,
        capturedAtUnixMs: 1,
      },
      recordedAtUnixMs: 1,
    };
    recordProvenanceJs(row, deep);
    const raw = readFileSync(deep, "utf8");
    assert.equal(raw.split("\n").filter((line) => line.length > 0).length, 1);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
