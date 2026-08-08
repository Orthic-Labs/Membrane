import assert from "node:assert/strict";
import test from "node:test";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { verifyOciRelease } from "../../scripts/release/verify-oci-release.mjs";

const manifest = JSON.parse(readFileSync(new URL("../../packaging/oci/release.v1.json", import.meta.url)));
test("OCI contract stays unavailable & non-publishing", () => assert.deepEqual(verifyOciRelease(manifest), { verified: true, state: "unavailable", publish: false }));
test("OCI contract rejects release-ready mutation", () => assert.throws(() => verifyOciRelease({ ...manifest, state: "ready" }), /FAIL CLOSED/));
test("ready OCI contract requires every hash-bound release proof", () => {
  const root = mkdtempSync(join(tmpdir(), "membrane-oci-")); const proof = name => { writeFileSync(join(root, name), name); return { path: name, sha256: createHash("sha256").update(name).digest("hex") }; };
  const ready = { ...manifest, state: "ready", image: `ghcr.io/orthic/membrane@sha256:${"a".repeat(64)}`, base: `cgr.dev/chainguard/static@sha256:${"b".repeat(64)}`, identity: { tag: "v1.2.3", commit: "c".repeat(40), release_generation: "d".repeat(64), artifact_sha256: "e".repeat(64) }, evidence: Object.fromEntries(["sbom", "ed25519", "cosign", "rootlessHealth", "secretScan"].map(name => [name, proof(name)])) };
  assert.equal(verifyOciRelease(ready, root).state, "ready"); ready.evidence.sbom.sha256 = "f".repeat(64); assert.throws(() => verifyOciRelease(ready, root), /hash mismatch/);
});
test("Containerfile fixes non-root, digest base & a real headless healthcheck", () => {
  const file = readFileSync(new URL("../../packaging/oci/Containerfile", import.meta.url), "utf8");
  assert.match(file, /^FROM .+@sha256:[a-f0-9]{64}$/m);
  assert.match(file, /^USER 65532:65532$/m);
  // MBR-910: the healthcheck must invoke a real, read-only, non-interactive
  // CLI subcommand -- not a fabricated flag no binary implements.
  assert.match(file, /HEALTHCHECK .*CMD \["\/usr\/local\/bin\/membrane", "cli", "doctor", "paths"\]/);
  assert.doesNotMatch(file, /(?:SECRET|TOKEN|PASSWORD|API_KEY)=/i);
});

test("Containerfile exposes no network port by default (loopback-bound guarantee)", () => {
  const file = readFileSync(new URL("../../packaging/oci/Containerfile", import.meta.url), "utf8");
  assert.doesNotMatch(file, /^EXPOSE\b/m);
});
