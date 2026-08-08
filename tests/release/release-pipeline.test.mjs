import test from "node:test";
import assert from "node:assert/strict";
import { buildPlan, validateRelease } from "../../scripts/release/orchestrate-release.mjs";

const hash = "a".repeat(64); const base = { tag: "v0.9.0", commit: "b".repeat(40), tree: hash, release_generation: hash, vector_dispatch: "CRYPT_VECTOR_DISPATCH_V2", target: "mac-arm64", artifact_sha256: hash, evidence_sha256: hash, publish: false, stages: ["provenance", "sbom", "sign", "platform_trust", "verify"] };
for (const stage of base.stages) base[stage] = { status: "planned", sha256: hash, provider: stage === "platform_trust" ? "apple-notary" : "source" };
test("valid plan binds immutable identity and all stages", () => { const plan = buildPlan(base); assert.equal(plan.publish, false); assert.equal(plan.identity.commit, base.commit); assert.deepEqual(plan.stages.map((s) => s.name), base.stages); });
test("unsupported target and mutable publication fail closed", () => { assert.throws(() => validateRelease({ ...base, target: "linux-x64" }), /unsupported target/); assert.throws(() => validateRelease({ ...base, publish: true }), /publish must be false/); });
test("vector dispatch and tagged revision are mandatory", () => { assert.throws(() => validateRelease({ ...base, vector_dispatch: "old" }), /vector_dispatch/); assert.throws(() => validateRelease({ ...base, tag: "main" }), /semver tag/); });
test("Windows requires Authenticode trust", () => { assert.throws(() => validateRelease({ ...base, target: "windows-x64" }), /windows-authenticode/); });
