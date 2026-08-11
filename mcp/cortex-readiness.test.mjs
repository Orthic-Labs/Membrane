import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { READINESS_STATES, readCortexReadiness } from "./cortex-readiness.mjs";

const base = { schemaVersion: 1, repository: { id: "repo" }, manifest: { generationId: "gen-1", manifestDigest: "sha256:abc" } };

test("readiness preserves four Membrane states", () => {
  assert.deepEqual([...READINESS_STATES].sort(), ["current", "degraded", "stale", "unwatched"]);
});

test("uses typed Cortex status schema without graph-store access", async () => {
  const readiness = await readCortexReadiness("/repo", { observer: async () => ({ ...base, state: "fresh" }) });
  assert.equal(readiness.freshness, "current");
  assert.equal(readiness.generationId, "gen-1");
  assert.equal(readiness.source, "cortex-ipc");
});

test("maps Cortex degraded states without spawning a fallback process", async () => {
  for (const state of ["indeterminate", "incomplete", "corrupt"]) {
    const readiness = await readCortexReadiness("/repo", { observer: async () => ({ ...base, state }) });
    assert.equal(readiness.freshness, "degraded");
  }
});

test("unavailable or malformed Cortex status degrades honestly", async () => {
  const unavailable = await readCortexReadiness("/repo", { observer: async () => { throw new Error("offline"); } });
  const malformed = await readCortexReadiness("/repo", { observer: async () => ({ schemaVersion: 9 }) });
  assert.equal(unavailable.reason, "cortex_status_unavailable");
  assert.equal(malformed.reason, "cortex_status_schema_invalid");
  assert.equal(unavailable.fallback, "observer_unavailable");
});

test("observer seam has no graph-store or Cortex process fallback", async () => {
  const source = await readFile(new URL("./cortex-readiness.mjs", import.meta.url), "utf8");
  assert.doesNotMatch(source, /node:child_process|execFile|spawn\(/);
  assert.doesNotMatch(source, /node:sqlite|DatabaseSync/);
});
