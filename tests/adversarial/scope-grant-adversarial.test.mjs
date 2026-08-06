// tests/adversarial/scope-grant-adversarial.test.mjs — MBR-802.
//
// Attacks the scope-grant V1 model in mcp/scope-grant-v1.mjs (imported read-only
// via absolute file URL): forged scopeGrantId, forged signer, replay / tamper,
// authority-widening field edits, and prompt-injection carried inside grant
// fields. Every forgery must fail validation. Unauthorized admission rate = 0.

import assert from "node:assert/strict";
import test from "node:test";
import { pathToFileURL, fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

import { makeScopeGrantWorld, INJECTION_PAYLOADS } from "../fixtures/adversarial/fixtures.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const SG_URL = pathToFileURL(resolve(HERE, "../../mcp/scope-grant-v1.mjs")).href;
const { mintScopeGrantV1, validateScopeGrantV1, scopeGrantTrust } = await import(SG_URL);

const world = makeScopeGrantWorld();
const { trusted, forged, binding, args, freshness, packet, now } = world;
const trust = scopeGrantTrust(trusted);

const mint = (overrides = {}) =>
  mintScopeGrantV1({ binding, args, packet, freshness, signingKey: trusted, now, ...overrides });
const validate = (grant) =>
  validateScopeGrantV1(grant, {
    binding, args, packet, freshness,
    signingKey: trusted, trustedPublicKey: trust.publicKey, trustedKeyId: trust.keyId, now,
  });

const validGrant = mint();

test("a legitimately minted grant validates (control — validator is not fail-always)", () => {
  assert.ok(validGrant, "mint must succeed for a well-formed request");
  assert.equal(validate(validGrant), true, "control grant must validate");
});

test("forged signer key is rejected even when the grant is otherwise well-formed", () => {
  const forgedGrant = mintScopeGrantV1({ binding, args, packet, freshness, signingKey: forged, now });
  assert.ok(forgedGrant, "minting with an attacker key still produces a structurally valid grant");
  assert.equal(validate(forgedGrant), false, "grant signed by an untrusted key must be rejected");
});

test("forged scopeGrantId / tampered signature is rejected", () => {
  assert.equal(
    validate({ ...validGrant, id: "sgv1-forged-attacker" }),
    false, "a replaced scopeGrantId breaks the signature and must be rejected",
  );
  assert.equal(
    validate({ ...validGrant, signature: validGrant.signature.slice(0, -2) + "aa" }),
    false, "a tampered signature must be rejected",
  );
});

test("authority-widening immutable-field edits are all rejected", () => {
  const widened = [
    ["repositoryId", "repo-admin"],
    ["repositoryRoot", "/etc"],
    ["sourceReadBytesMax", 1024 * 1024 * 1024],
    ["uniqueFilesMax", 10000],
    ["resultsMax", 10000],
    ["taskId", "other-task"],
    ["sessionId", "other-session"],
  ];
  for (const [field, value] of widened) {
    assert.equal(
      validate({ ...validGrant, [field]: value }),
      false, `widening immutable field '${field}' must be rejected`,
    );
  }
  // Widening the repository or read-path sets must also be rejected.
  assert.equal(validate({ ...validGrant, repositoryIds: ["/etc"] }), false);
  assert.equal(
    validate({ ...validGrant, readPaths: [{ path: "../secret", startLine: 1, endLine: 9 }] }),
    false, "a path-escaping readPath must be rejected",
  );
});

test("expired and non-active grants are rejected (replay window is closed)", () => {
  assert.equal(
    validate({ ...validGrant, expiresAt: "2026-08-03T00:00:00.000Z" }),
    false, "expired grant must be rejected",
  );
  assert.equal(
    validate({ ...validGrant, status: "revoked" }),
    false, "non-active grant must be rejected",
  );
});

test("prompt-injection text carried inside grant fields never validates to admission", () => {
  for (const payload of Object.values(INJECTION_PAYLOADS)) {
    const text = typeof payload === "string" ? payload : JSON.stringify(payload);
    // Directive stuffed into intent / anchors / task — breaks requestHash binding.
    const poisoned = mint({ args: { ...args, intent: text } });
    assert.equal(validate(poisoned), false, `injection in 'intent' must not validate: ${text.slice(0, 32)}`);
    // Directive stuffed into a repository identity field.
    assert.equal(
      validate({ ...validGrant, repositoryRoot: text }),
      false, `injection in 'repositoryRoot' must not validate: ${text.slice(0, 32)}`,
    );
  }
});

test("minting is fail-closed for adversarial packet shapes", () => {
  // A blueprint sourceRef that escapes the repo root cannot produce a grant.
  assert.equal(
    mint({ packet: { blocks: [{ provider: "blueprint", sourceRef: "../secret:1-2" }] } }),
    null, "path-traversal sourceRef must not mint",
  );
  // A stale graph state cannot produce a grant.
  assert.equal(
    mint({ freshness: { ...freshness, graphState: "stale_snapshot" } }),
    null, "stale freshness must not mint",
  );
  // Missing blueprint provider usability cannot produce a grant.
  assert.equal(
    mint({ freshness: { ...freshness, providers: { blueprint: { usable: false } } } }),
    null, "unusable blueprint provider must not mint",
  );
});
