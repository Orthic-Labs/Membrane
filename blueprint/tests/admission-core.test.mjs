import assert from "node:assert/strict";
import { mkdtempSync, rmSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

import { createAdmission, DECISION_ACTIONS } from "../src/lib/admission.mjs";
import { CortexError } from "../src/lib/application/errors.mjs";
import { createReceiptStore } from "../src/lib/receipt-store.mjs";

const CLAIM_BOUNDARY_KEYS = ["cleanClaimAllowed", "gaps", "prohibitedClaims", "safeClaims", "status"];

function makeIsolatedAdmission(overrides = {}) {
  const storeDir = mkdtempSync(join(tmpdir(), "bp-admission-store-"));
  const evidenceDir = mkdtempSync(join(tmpdir(), "bp-admission-evidence-"));
  const repoRoot = resolve(mkdtempSync(join(tmpdir(), "bp-admission-repo-")));
  const generation = {
    manifest: {
      generationId: "xxh128:gen-fixed",
      manifestDigest: "sha256:manifest-fixed",
      generatedAt: "gen:fixed",
    },
    provider: { id: "cortex-static" },
    sourceObservation: { dirty: false },
    nodes: [],
    edges: [],
  };
  const candidateSet = {
    schemaVersion: 1,
    provider: "cortex-static",
    freshness: { revision: "xxh128:gen-fixed", manifestDigest: "sha256:manifest-fixed" },
    candidates: [
      {
        id: "symbol:src/a.ts::alpha",
        sourceRef: "src/a.ts:1-3",
        sourceHash: "xxh128:aaa",
        protected: false,
        exact: true,
      },
      {
        id: "symbol:src/b.ts::beta",
        sourceRef: "src/b.ts:1-2",
        sourceHash: "xxh128:bbb",
        protected: true,
        exact: false,
      },
    ],
    omissions: [{ id: "symbol:src/c.ts::gamma", reason: "over_candidate_ceiling" }],
  };
  const repoIdentity = `${repoRoot.split(/[\\/]/).pop()}@${repoRoot}`;
  const api = createAdmission({
    storeDir,
    evidenceDir,
    repoIdentity,
    readGeneration: () => generation,
    createContextCandidateSet: (_gen, options = {}) => {
      if (options.query === "expand-me" || options.path === "src/extra.ts") {
        return {
          ...candidateSet,
          candidates: [
            ...candidateSet.candidates,
            {
              id: "symbol:src/extra.ts::extra",
              sourceRef: "src/extra.ts:1-1",
              sourceHash: "xxh128:eee",
              protected: false,
              exact: false,
            },
          ],
        };
      }
      return candidateSet;
    },
    graphStatus: () => ({ state: "fresh", capabilities: { dirtyOverlayFileCount: 0 } }),
    ...overrides,
  });
  return { api, storeDir, evidenceDir, repoRoot, generation, candidateSet };
}

test("orient blocks when graph is missing — decision only, no hooks", async () => {
  const { api, storeDir, evidenceDir } = makeIsolatedAdmission({
    graphStatus: () => ({ state: "missing" }),
    readGeneration: () => null,
  });
  try {
    const result = await api.orient({
      task: "inspect auth",
      sessionId: "sess",
      repoRoot: "/tmp/demo-repo",
    });
    assert.equal(result.action, "block");
    assert.equal(result.reasonCode, "missing_graph");
    assert.match(result.nextAction, /cortex build/);
    assert.ok(DECISION_ACTIONS.includes(result.action));
    assert.equal(result.schemaVersion, 1);
  } finally {
    rmSync(storeDir, { recursive: true, force: true });
    rmSync(evidenceDir, { recursive: true, force: true });
  }
});

test("orient issues a host receipt and Forge evidence file", async () => {
  const { api, storeDir, evidenceDir } = makeIsolatedAdmission();
  try {
    const result = await api.orient({
      task: "find alpha",
      sessionId: "sess-1",
      taskId: "find alpha",
      repoRoot: "/tmp/demo-repo",
      anchors: ["src/b.ts"],
    });
    assert.equal(result.action, "allow");
    assert.equal(result.reasonCode, "oriented");
    assert.ok(result.receiptId);
    assert.ok(result.allowedScopes.includes("src/a.ts"));
    assert.ok(result.allowedScopes.includes("src/b.ts"));
    assert.equal(result.evidence.kind, "cortex_orientation");
    assert.equal(result.evidence.locator, `cortex://receipt/${result.receiptId}`);
    assert.ok(result.evidencePath);
    const onDisk = JSON.parse(readFileSync(result.evidencePath, "utf8"));
    assert.equal(onDisk.kind, "cortex_orientation");
    assert.equal(result.receipt.generationId, "xxh128:gen-fixed");
    assert.equal(result.receipt.sessionId, "sess-1");

    const reused = await api.orient({
      task: "find alpha",
      sessionId: "sess-1",
      taskId: "find alpha",
      repoRoot: "/tmp/demo-repo",
    });
    assert.equal(reused.action, "continue");
    assert.equal(reused.reasonCode, "receipt_reuse");
    assert.equal(reused.receiptId, result.receiptId);
  } finally {
    rmSync(storeDir, { recursive: true, force: true });
    rmSync(evidenceDir, { recursive: true, force: true });
  }
});

test("expand unions graph-supported paths and rejects absolute self-approval", async () => {
  const { api, storeDir, evidenceDir } = makeIsolatedAdmission();
  try {
    const oriented = await api.orient({
      task: "base",
      sessionId: "s",
      taskId: "base",
      repoRoot: "/tmp/demo-repo",
    });
    const blocked = await api.expand({
      receiptId: oriented.receiptId,
      paths: ["/etc/passwd"],
      repoRoot: "/tmp/demo-repo",
    });
    assert.equal(blocked.action, "block");
    assert.equal(blocked.reasonCode, "absolute_path_rejected");
    const blockedWindows = await api.expand({
      receiptId: oriented.receiptId,
      paths: ["C:\\Windows\\System32"],
      repoRoot: "/tmp/demo-repo",
    });
    assert.equal(blockedWindows.action, "block");
    assert.equal(blockedWindows.reasonCode, "absolute_path_rejected");

    const expanded = await api.expand({
      receiptId: oriented.receiptId,
      query: "expand-me",
      path: "src/extra.ts",
      repoRoot: "/tmp/demo-repo",
    });
    assert.equal(expanded.action, "continue");
    assert.equal(expanded.reasonCode, "expanded");
    assert.ok(expanded.allowedScopes.includes("src/extra.ts"));
    assert.equal(expanded.receipt.overlayRevision, oriented.receipt.overlayRevision + 1);
    assert.equal(
      expanded.evidence.invalidation_key,
      `${expanded.receipt.manifestDigest}:${expanded.receipt.overlayRevision}`,
    );
  } finally {
    rmSync(storeDir, { recursive: true, force: true });
    rmSync(evidenceDir, { recursive: true, force: true });
  }
});

test("status and revoke round-trip", async () => {
  const { api, storeDir, evidenceDir } = makeIsolatedAdmission();
  try {
    const oriented = await api.orient({
      task: "status task",
      sessionId: "s2",
      taskId: "status task",
      repoRoot: "/tmp/demo-repo",
    });
    const st = await api.status({ receiptId: oriented.receiptId });
    assert.equal(st.action, "continue");
    assert.equal(st.reasonCode, "receipt_active");

    const revoked = await api.revoke({ receiptId: oriented.receiptId });
    assert.equal(revoked.action, "allow");
    assert.equal(revoked.reasonCode, "revoked");
    assert.equal(revoked.receipt.status, "revoked");

    const after = await api.status({ receiptId: oriented.receiptId });
    assert.equal(after.action, "block");
    assert.equal(after.reasonCode, "receipt_revoked");

    const expandBlocked = await api.expand({
      receiptId: oriented.receiptId,
      query: "anything",
      repoRoot: "/tmp/demo-repo",
    });
    assert.equal(expandBlocked.reasonCode, "receipt_revoked");
  } finally {
    rmSync(storeDir, { recursive: true, force: true });
    rmSync(evidenceDir, { recursive: true, force: true });
  }
});

test("generation mismatch blocks orient", async () => {
  const { api, storeDir, evidenceDir } = makeIsolatedAdmission();
  try {
    const result = await api.orient({
      task: "pin",
      sessionId: "s",
      repoRoot: "/tmp/demo-repo",
      expectedGeneration: "xxh128:other",
    });
    assert.equal(result.action, "block");
    assert.equal(result.reasonCode, "generation_mismatch");
  } finally {
    rmSync(storeDir, { recursive: true, force: true });
    rmSync(evidenceDir, { recursive: true, force: true });
  }
});

test("createAdmission store is injectable for Crypt-style hosts", () => {
  const storeDir = mkdtempSync(join(tmpdir(), "bp-host-store-"));
  try {
    const store = createReceiptStore({ storeDir });
    const api = createAdmission({
      store,
      readGeneration: () => null,
      createContextCandidateSet: () => ({ candidates: [], omissions: [] }),
      graphStatus: () => ({ state: "missing" }),
    });
    assert.equal(api.store.storeDir, store.storeDir);
  } finally {
    rmSync(storeDir, { recursive: true, force: true });
  }
});

// CX-B3: every admission decision must carry the exact five-key claimBoundary
// block, and orient/status branches must derive it from the live graph state.

function assertClaimBoundaryShape(claimBoundary) {
  assert.deepEqual(Object.keys(claimBoundary).sort(), CLAIM_BOUNDARY_KEYS);
  assert.ok(["clear", "restricted"].includes(claimBoundary.status));
  assert.equal(typeof claimBoundary.cleanClaimAllowed, "boolean");
  assert.ok(Array.isArray(claimBoundary.safeClaims));
  assert.ok(Array.isArray(claimBoundary.prohibitedClaims));
  assert.ok(Array.isArray(claimBoundary.gaps));
}

test("fresh orient allows clean claims and derives gaps from omission reasons", async () => {
  const { api, storeDir, evidenceDir, repoRoot } = makeIsolatedAdmission();
  try {
    const result = await api.orient({
      task: "find alpha",
      sessionId: "sess-cb-fresh",
      taskId: "find alpha",
    });
    assert.equal(result.action, "allow");
    assert.equal(result.reasonCode, "oriented");
    assertClaimBoundaryShape(result.claimBoundary);
    assert.equal(result.claimBoundary.status, "clear");
    assert.equal(result.claimBoundary.cleanClaimAllowed, true);
    assert.deepEqual(result.claimBoundary.safeClaims, ["Results reflect the current sealed generation."]);
    assert.deepEqual(result.claimBoundary.prohibitedClaims, []);
    assert.deepEqual(result.claimBoundary.gaps, ["over_candidate_ceiling"]);
  } finally {
    rmSync(storeDir, { recursive: true, force: true });
    rmSync(evidenceDir, { recursive: true, force: true });
    rmSync(repoRoot, { recursive: true, force: true });
  }
});

test("stale orient restricts clean claims with a non-empty prohibited list", async () => {
  const { api, storeDir, evidenceDir, repoRoot } = makeIsolatedAdmission({
    graphStatus: () => ({ state: "stale", capabilities: { dirtyOverlayFileCount: 1 } }),
  });
  try {
    const result = await api.orient({
      task: "find alpha",
      sessionId: "sess-cb-stale",
      taskId: "find alpha",
    });
    assert.equal(result.action, "continue");
    assert.equal(result.reasonCode, "oriented_stale");
    assertClaimBoundaryShape(result.claimBoundary);
    assert.equal(result.claimBoundary.status, "restricted");
    assert.equal(result.claimBoundary.cleanClaimAllowed, false);
    assert.ok(result.claimBoundary.prohibitedClaims.length > 0);
    assert.ok(result.claimBoundary.prohibitedClaims.includes("Graph-derived facts are current."));
    assert.deepEqual(result.claimBoundary.safeClaims, [
      "Results reflect generation xxh128:gen-fixed, which predates current worktree changes.",
    ]);
    assert.deepEqual(result.claimBoundary.gaps, ["over_candidate_ceiling"]);
  } finally {
    rmSync(storeDir, { recursive: true, force: true });
    rmSync(evidenceDir, { recursive: true, force: true });
    rmSync(repoRoot, { recursive: true, force: true });
  }
});

test("indeterminate orient restricts clean claims", async () => {
  const { api, storeDir, evidenceDir, repoRoot } = makeIsolatedAdmission({
    graphStatus: () => ({ state: "indeterminate", capabilities: { dirtyOverlayFileCount: 0 } }),
  });
  try {
    const result = await api.orient({
      task: "find alpha",
      sessionId: "sess-cb-indeterminate",
      taskId: "find alpha",
    });
    assert.equal(result.action, "continue");
    assert.equal(result.reasonCode, "oriented_indeterminate");
    assertClaimBoundaryShape(result.claimBoundary);
    assert.equal(result.claimBoundary.status, "restricted");
    assert.equal(result.claimBoundary.cleanClaimAllowed, false);
    assert.ok(result.claimBoundary.prohibitedClaims.length > 0);
  } finally {
    rmSync(storeDir, { recursive: true, force: true });
    rmSync(evidenceDir, { recursive: true, force: true });
    rmSync(repoRoot, { recursive: true, force: true });
  }
});

test("orient early branches carry a restricted claimBoundary", async () => {
  const missing = makeIsolatedAdmission({
    graphStatus: () => ({ state: "missing" }),
    readGeneration: () => null,
  });
  const mismatched = makeIsolatedAdmission();
  try {
    const missingResult = await missing.api.orient({
      task: "inspect auth",
      sessionId: "sess",
    });
    assert.equal(missingResult.action, "block");
    assert.equal(missingResult.reasonCode, "missing_graph");
    assertClaimBoundaryShape(missingResult.claimBoundary);
    assert.equal(missingResult.claimBoundary.status, "restricted");
    assert.equal(missingResult.claimBoundary.cleanClaimAllowed, false);
    assert.deepEqual(missingResult.claimBoundary.gaps, ["missing_graph"]);

    const mismatchResult = await mismatched.api.orient({
      task: "pin",
      sessionId: "s",
      expectedGeneration: "xxh128:other",
    });
    assert.equal(mismatchResult.action, "block");
    assert.equal(mismatchResult.reasonCode, "generation_mismatch");
    assertClaimBoundaryShape(mismatchResult.claimBoundary);
    assert.equal(mismatchResult.claimBoundary.status, "restricted");
    assert.equal(mismatchResult.claimBoundary.cleanClaimAllowed, false);
  } finally {
    rmSync(missing.storeDir, { recursive: true, force: true });
    rmSync(missing.evidenceDir, { recursive: true, force: true });
    rmSync(mismatched.storeDir, { recursive: true, force: true });
    rmSync(mismatched.evidenceDir, { recursive: true, force: true });
    rmSync(missing.repoRoot, { recursive: true, force: true });
    rmSync(mismatched.repoRoot, { recursive: true, force: true });
  }
});

test("status on a fresh active receipt allows clean claims via worktreeIdentity root", async () => {
  const { api, storeDir, evidenceDir, repoRoot } = makeIsolatedAdmission();
  try {
    const oriented = await api.orient({
      task: "status task",
      sessionId: "s2",
      taskId: "status task",
    });
    assert.ok(oriented.receipt.worktreeIdentity.startsWith("file:"));
    const st = await api.status({ receiptId: oriented.receiptId });
    assert.equal(st.action, "continue");
    assert.equal(st.reasonCode, "receipt_active");
    assertClaimBoundaryShape(st.claimBoundary);
    assert.equal(st.claimBoundary.status, "clear");
    assert.equal(st.claimBoundary.cleanClaimAllowed, true);
    assert.deepEqual(st.claimBoundary.prohibitedClaims, []);
    assert.deepEqual(st.claimBoundary.safeClaims, ["Results reflect the current sealed generation."]);
    assert.deepEqual(st.claimBoundary.gaps, ["over_candidate_ceiling"]);
  } finally {
    rmSync(storeDir, { recursive: true, force: true });
    rmSync(evidenceDir, { recursive: true, force: true });
    rmSync(repoRoot, { recursive: true, force: true });
  }
});

test("status honors an explicit repoRoot before the receipt worktree identity", async () => {
  const observedRoots = [];
  const { api, storeDir, evidenceDir, repoRoot } = makeIsolatedAdmission({
    graphStatus: (root) => {
      observedRoots.push(resolve(root));
      return { state: "fresh", capabilities: { dirtyOverlayFileCount: 0 } };
    },
  });
  const statusRepoRoot = resolve(mkdtempSync(join(tmpdir(), "bp-admission-status-repo-")));
  try {
    const oriented = await api.orient({
      task: "status root precedence",
      sessionId: "s-root-precedence",
      taskId: "status root precedence",
      repoRoot,
    });
    const st = await api.status({ receiptId: oriented.receiptId, repoRoot: statusRepoRoot });
    assert.equal(st.claimBoundary.cleanClaimAllowed, true);
    assert.equal(observedRoots.at(-1), statusRepoRoot);
  } finally {
    rmSync(storeDir, { recursive: true, force: true });
    rmSync(evidenceDir, { recursive: true, force: true });
    rmSync(repoRoot, { recursive: true, force: true });
    rmSync(statusRepoRoot, { recursive: true, force: true });
  }
});

test("status on a stale active receipt restricts clean claims", async () => {
  const { api, storeDir, evidenceDir, repoRoot } = makeIsolatedAdmission({
    graphStatus: () => ({ state: "stale", capabilities: { dirtyOverlayFileCount: 1 } }),
  });
  try {
    const oriented = await api.orient({
      task: "status task",
      sessionId: "s2",
      taskId: "status task",
    });
    const st = await api.status({ receiptId: oriented.receiptId });
    assert.equal(st.action, "continue");
    assert.equal(st.reasonCode, "receipt_active");
    assertClaimBoundaryShape(st.claimBoundary);
    assert.equal(st.claimBoundary.status, "restricted");
    assert.equal(st.claimBoundary.cleanClaimAllowed, false);
    assert.ok(st.claimBoundary.prohibitedClaims.length > 0);
    assert.ok(st.claimBoundary.prohibitedClaims.includes("Graph-derived facts are current."));
  } finally {
    rmSync(storeDir, { recursive: true, force: true });
    rmSync(evidenceDir, { recursive: true, force: true });
    rmSync(repoRoot, { recursive: true, force: true });
  }
});

test("status early branches carry a restricted claimBoundary", async () => {
  const { api, storeDir, evidenceDir, repoRoot } = makeIsolatedAdmission();
  try {
    const noReceipt = await api.status({ receiptId: "does-not-exist" });
    assert.equal(noReceipt.action, "noop");
    assert.equal(noReceipt.reasonCode, "no_receipt");
    assertClaimBoundaryShape(noReceipt.claimBoundary);
    assert.equal(noReceipt.claimBoundary.status, "restricted");
    assert.equal(noReceipt.claimBoundary.cleanClaimAllowed, false);
    assert.deepEqual(noReceipt.claimBoundary.gaps, ["no_receipt"]);

    const oriented = await api.orient({
      task: "status task",
      sessionId: "s3",
      taskId: "status task",
    });
    await api.revoke({ receiptId: oriented.receiptId });
    const revoked = await api.status({ receiptId: oriented.receiptId });
    assert.equal(revoked.action, "block");
    assert.equal(revoked.reasonCode, "receipt_revoked");
    assertClaimBoundaryShape(revoked.claimBoundary);
    assert.equal(revoked.claimBoundary.status, "restricted");
    assert.equal(revoked.claimBoundary.cleanClaimAllowed, false);
  } finally {
    rmSync(storeDir, { recursive: true, force: true });
    rmSync(evidenceDir, { recursive: true, force: true });
    rmSync(repoRoot, { recursive: true, force: true });
  }
});

// CX-B3 errors lane: required code mappings plus preserved legacy fields.

test("CortexError carries required metadata for mapped codes", () => {
  const required = [
    { code: "stale_blocked", retryable: true, nextOperation: "cortex_orient" },
    { code: "generation_mismatch", retryable: true, nextOperation: "cortex_orient" },
    { code: "missing_generation", retryable: true, nextOperation: "cortex build" },
    { code: "missing_graph", retryable: true, nextOperation: "cortex build" },
    { code: "root_not_enrolled", retryable: false, nextOperation: "cortex init" },
  ];
  for (const { code, retryable, nextOperation } of required) {
    const error = new CortexError(code, `message for ${code}`, { marker: code });
    assert.ok(error instanceof CortexError, `${code} instanceof CortexError`);
    assert.equal(error.code, code);
    assert.equal(typeof error.retryable, "boolean", `${code} retryable boolean`);
    assert.equal(error.retryable, retryable, `${code} retryable value`);
    assert.ok(error.remediation, `${code} must carry remediation`);
    assert.equal(typeof error.remediation.summary, "string", `${code} remediation summary`);
    assert.ok(error.remediation.summary.length > 0, `${code} remediation summary non-empty`);
    assert.equal(error.remediation.nextOperation, nextOperation, `${code} nextOperation`);
    assert.ok(error.remediation.arguments !== null && typeof error.remediation.arguments === "object", `${code} arguments object`);
    assert.ok(!Array.isArray(error.remediation.arguments), `${code} arguments not an array`);
  }
});

test("CortexError preserves legacy fields and defaults for unmapped codes", () => {
  const error = new CortexError("stale_blocked", "Cortex freshness barrier did not catch up.", { receipt: { id: 1 } });
  assert.equal(error.name, "CortexError");
  assert.equal(error.code, "stale_blocked");
  assert.equal(error.message, "Cortex freshness barrier did not catch up.");
  assert.deepEqual(error.details, { receipt: { id: 1 } });

  const unmapped = new CortexError("no_such_code", "m");
  assert.equal(unmapped.code, "no_such_code");
  assert.equal(unmapped.retryable, false);
  assert.equal(unmapped.remediation, null);
});
