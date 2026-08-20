import assert from "node:assert/strict";
import { generateKeyPairSync } from "node:crypto";
import { mintScopeGrantV1, scopeGrantSigningBytes, scopeGrantTrust, validateScopeGrantV1 } from "./scope-grant-v1.mjs";

const binding = { root: "/workspace/repo", repository_id: "repo-123", scope_id: "scope-123" };
const args = { task: "inspect exact source", taskId: "task-123", session: "session-123", budget: 800, intent: "diagnose", anchors: "src/app.ts" };
const freshness = { graphState: "dirty_overlay", blueprintGeneration: "xxh128:graph-123", manifestDigest: "sha256:" + "a".repeat(64), providers: { blueprint: { usable: true } } };
const packet = { blocks: [{ provider: "blueprint", sourceRef: "src/app.ts:4-18" }, { provider: "cortex", sourceRef: "memory:local" }] };
const now = Date.parse("2026-08-02T00:00:00.000Z");
const signer = generateKeyPairSync("ed25519").privateKey;
const forgedSigner = generateKeyPairSync("ed25519").privateKey;
const trust = scopeGrantTrust(signer);
const signed = (overrides = {}) => mintScopeGrantV1({ binding, args, packet, freshness, signingKey: signer, now, ...overrides });
const valid = (grant) => validateScopeGrantV1(grant, { binding, args, packet, freshness, signingKey: signer, trustedPublicKey: trust.publicKey, trustedKeyId: trust.keyId, now });

const grant = signed();
assert.ok(grant);
assert.equal(grant.taskId, args.taskId);
assert.equal(grant.signatureAlgorithm, "Ed25519");
assert.equal(grant.keyId, trust.keyId);
assert.deepEqual(grant.readPaths, [{ path: "src/app.ts", startLine: 4, endLine: 18 }]);
assert.equal(valid(grant), true);
assert.match(scopeGrantSigningBytes(grant).toString("utf8"), /^rightstudio\.scope-grant\.v1\0\{/);

assert.equal(signed({ freshness: { ...freshness, graphState: "stale_snapshot" } }), null);
assert.equal(signed({ packet: { blocks: [{ provider: "blueprint", sourceRef: "../secret:1-2" }] } }), null);
assert.equal(valid({ ...grant, signature: grant.signature.slice(0, -2) + "aa" }), false, "tamper denied");
assert.equal(valid({ ...grant, keyId: "scope-grant-ed25519-v1:" + "0".repeat(32) }), false, "wrong key id denied");
const forged = mintScopeGrantV1({ binding, args, packet, freshness, signingKey: forgedSigner, now });
assert.equal(valid(forged), false, "forged signer denied");
assert.equal(valid({ ...grant, expiresAt: "2026-08-01T00:00:00.000Z" }), false, "expired denied");
for (const [field, value] of [
  ["taskId", "other"], ["sessionId", "other"], ["repositoryId", "other"], ["repositoryRoot", "/other"],
  ["blueprintGeneration", "other"], ["requestHash", "sha256:" + "c".repeat(64)], ["contextPacketHash", "sha256:" + "d".repeat(64)],
]) assert.equal(valid({ ...grant, [field]: value }), false, `${field} binding denied`);
assert.notEqual(grant.id, signed().id, "minted ScopeGrant IDs are one-use replay identities");
