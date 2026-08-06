// tests/adversarial/authorization-adversarial.test.mjs — MBR-802.
//
// Exercises the REAL authorization decision logic in mcp/authorization.mjs
// (imported read-only via absolute file URL) against the fixture battery.
// Every attack must be REJECTED. The unauthorized admission rate must be zero.

import assert from "node:assert/strict";
import test from "node:test";
import { pathToFileURL, fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

import {
  buildCases, READ_ACTIONS, WRITE_ACTIONS, makeBinding, INJECTION_PAYLOADS,
} from "../fixtures/adversarial/fixtures.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const AUTH_URL = pathToFileURL(resolve(HERE, "../../mcp/authorization.mjs")).href;
const { authorizeTarget, canReachTarget, intersectAuthority, permitsLevel, levelRank } =
  await import(AUTH_URL);

// Execute one fixture case and return { admitted, error, effectiveLevel }.
async function attempt(c) {
  try {
    const result = await authorizeTarget({
      callerBinding: c.caller,
      targetBinding: c.target,
      childGrantLevel: c.childGrantLevel,
      taskGrantLevel: c.taskGrantLevel,
      installationLevel: c.installationLevel ?? "write-proposed",
      action: c.action,
      hasExplicitChildGrant: c.hasExplicitChildGrant,
    });
    return { admitted: true, effectiveLevel: result.effectiveLevel, error: null };
  } catch (err) {
    return { admitted: false, effectiveLevel: null, error: String(err?.message ?? err) };
  }
}

test("adversarial authorization battery: zero unauthorized admissions", async () => {
  const cases = buildCases();
  const unauthorizedAdmissions = [];
  for (const c of cases) {
    const r = await attempt(c);
    if (c.expect === "admit") {
      assert.equal(r.admitted, true, `control ${c.id} must admit (harness not fail-always)`);
      assert.equal(r.effectiveLevel, c.expectEffectiveLevel, `control ${c.id} effective level`);
      continue;
    }
    if (r.admitted) {
      unauthorizedAdmissions.push(c.id);
      continue;
    }
    if (c.expectError) {
      assert.match(r.error, new RegExp(c.expectError), `${c.id} denied with expected error (${c.note})`);
    }
  }
  assert.deepEqual(
    unauthorizedAdmissions, [],
    `UNAUTHORIZED ADMISSIONS (rate must be 0): ${unauthorizedAdmissions.join(", ")}`,
  );
});

test("every write action is denied to a read-only caller across all reachable targets", async () => {
  const caller = makeBinding("repo-a", "read-only");
  const target = makeBinding("repo-a", "write-trusted");
  for (const action of WRITE_ACTIONS) {
    const r = await attempt({
      caller, target, action,
      hasExplicitChildGrant: false, taskGrantLevel: "admin", installationLevel: "admin",
    });
    assert.equal(r.admitted, false, `read-only caller must not perform write action '${action}'`);
  }
});

test("every read action is admitted to a read-only caller on its own target (sanity floor)", async () => {
  const caller = makeBinding("repo-a", "read-only");
  const target = makeBinding("repo-a", "read-only");
  for (const action of READ_ACTIONS) {
    const r = await attempt({
      caller, target, action,
      hasExplicitChildGrant: false, taskGrantLevel: "read-only",
    });
    assert.equal(r.admitted, true, `read action '${action}' should be admitted at read-only`);
  }
});

test("canReachTarget omits every ungranted sibling (aggregate fan-out cannot widen)", async () => {
  const callerBinding = makeBinding("ws-repo", "admin", {
    grant_policy: { level: "admin", child_repository_ids: ["granted"] },
  });
  const ungranted = makeBinding("ungranted-sibling", "write-trusted");
  for (const action of [...READ_ACTIONS, ...WRITE_ACTIONS]) {
    const level = await canReachTarget({
      callerBinding, targetBinding: ungranted, action,
      taskGrantLevel: "admin", hasExplicitChildGrant: false,
    });
    assert.equal(level, null, `ungranted sibling must be unreachable for '${action}'`);
  }
});

test("monotone intersection never recovers authority from the strongest single slot", async () => {
  // Rotate the single strong slot; all others read-only. Writes must be denied.
  const strongSlots = ["installationLevel", "taskGrantLevel", "childGrantLevel"];
  for (const strong of strongSlots) {
    const params = {
      callerBinding: makeBinding("ws", "read-only"),
      targetBinding: makeBinding("ws", "read-only"),
      hasExplicitChildGrant: false,
      action: "feedback",
      installationLevel: "read-only",
      taskGrantLevel: "read-only",
      childGrantLevel: "read-only",
    };
    params[strong] = "admin";
    await assert.rejects(authorizeTarget(params), /caller_not_authorized/,
      `single strong slot '${strong}' must not recover write authority`);
  }
});

test("prompt-injection directive text never widens authority when carried as data", async () => {
  for (const payload of Object.values(INJECTION_PAYLOADS)) {
    const text = typeof payload === "string" ? payload : JSON.stringify(payload);
    // Directive carried in the target's repository_id — cross-root match fails closed.
    const r = await attempt({
      caller: makeBinding("repo-a", "read-only"),
      target: makeBinding(text, "admin"),
      action: "feedback",
      hasExplicitChildGrant: false, taskGrantLevel: "admin",
    });
    assert.equal(r.admitted, false, `injection payload must not admit: ${text.slice(0, 40)}`);
  }
});

test("intersectAuthority is fail-closed and monotone for the full level lattice", () => {
  assert.equal(intersectAuthority("admin", "read-only", "admin"), "read-only");
  assert.equal(intersectAuthority("write-trusted", "write-proposed"), "write-proposed");
  assert.equal(intersectAuthority(), "read-only");
  assert.throws(() => intersectAuthority("owner"), /unknown_authority_level/);
  assert.throws(() => levelRank("superuser"), /unknown_authority_level/);
});

test("permitsLevel gates mutating ops behind write-proposed regardless of read membership", () => {
  assert.equal(permitsLevel("read-only", "feedback"), false);
  assert.equal(permitsLevel("read-only", "checkpoint"), false);
  assert.equal(permitsLevel("write-proposed", "feedback"), true);
  assert.equal(permitsLevel("read-only", "context"), true);
});
