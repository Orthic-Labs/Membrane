import assert from "node:assert/strict";
import test from "node:test";
import { intersectAuthority, permitsLevel, authorizeTarget, canReachTarget, levelRank } from "./authorization.mjs";

test("intersectAuthority returns the monotone minimum of all authorities", () => {
  assert.equal(intersectAuthority("read-only", "write-trusted"), "read-only");
  assert.equal(intersectAuthority("write-proposed", "write-trusted"), "write-proposed");
  assert.equal(intersectAuthority("write-trusted", "admin"), "write-trusted");
  assert.equal(intersectAuthority("admin"), "admin");
  assert.equal(intersectAuthority(), "read-only");
  assert.equal(intersectAuthority("read-only", "read-only", "read-only"), "read-only");
});

test("intersectAuthority rejects an unknown level instead of guessing", () => {
  assert.throws(() => intersectAuthority("superuser", "admin"), /unknown_authority_level:superuser/);
});

test("permitsLevel admits reads at read-only and mutating ops only at write-proposed+", () => {
  assert.equal(permitsLevel("read-only", "context"), true);
  assert.equal(permitsLevel("read-only", "source_read"), true);
  assert.equal(permitsLevel("read-only", "checkpoint_load"), true);
  assert.equal(permitsLevel("read-only", "system_status"), true);
  assert.equal(permitsLevel("read-only", "feedback"), false);
  assert.equal(permitsLevel("read-only", "checkpoint"), false);
  assert.equal(permitsLevel("write-proposed", "feedback"), true);
  assert.equal(permitsLevel("write-trusted", "feedback"), true);
  assert.equal(permitsLevel("write-proposed", "checkpoint"), true);
});

test("levelRank exposes the total order", () => {
  assert.equal(levelRank("read-only"), 0);
  assert.equal(levelRank("write-proposed"), 1);
  assert.equal(levelRank("write-trusted"), 2);
  assert.equal(levelRank("admin"), 3);
});

test("same-target mutation requires the caller's own level, not the admin slot alone", async () => {
  const callerBinding = { repository_id: "repo-a", grant_policy: { level: "write-proposed" } };
  const targetBinding = { repository_id: "repo-a", grant_policy: { level: "write-proposed" } };
  // Explicit task authority => effective = min(installation, caller, target, admin, task).
  const ok = await authorizeTarget({ callerBinding, targetBinding, hasExplicitChildGrant: false, taskGrantLevel: "write-proposed", action: "feedback" });
  assert.equal(ok.effectiveLevel, "write-proposed");
  // The SN template is fail-closed: an absent task/session grant caps at read-only.
  const noTaskGrant = await authorizeTarget({ callerBinding, targetBinding, hasExplicitChildGrant: false, action: "context" });
  assert.equal(noTaskGrant.effectiveLevel, "read-only");
  // A read-only caller stays read-only even with an explicit high task grant.
  const denied = authorizeTarget({
    callerBinding: { repository_id: "repo-a", grant_policy: { level: "read-only" } },
    targetBinding,
    hasExplicitChildGrant: false,
    taskGrantLevel: "write-trusted",
    action: "feedback",
  });
  await assert.rejects(denied, /caller_not_authorized/);
});

test("MBR-002 acceptance: a read-only workspace caller cannot invoke any mutating child operation even when the child binding is write-trusted", async () => {
  const callerBinding = { repository_id: "ws-repo", grant_policy: { level: "read-only", child_repository_ids: ["child-repo"] } };
  const writeTrustedChild = { repository_id: "child-repo", grant_policy: { level: "write-trusted" } };
  await assert.rejects(
    authorizeTarget({
      callerBinding,
      targetBinding: writeTrustedChild,
      hasExplicitChildGrant: true,
      childGrantLevel: writeTrustedChild.grant_policy.level,
      taskGrantLevel: "admin",
      installationLevel: "write-trusted",
      action: "feedback",
    }),
    /caller_not_authorized/,
  );
  await assert.rejects(
    authorizeTarget({
      callerBinding,
      targetBinding: writeTrustedChild,
      hasExplicitChildGrant: true,
      childGrantLevel: writeTrustedChild.grant_policy.level,
      taskGrantLevel: "admin",
      installationLevel: "write-trusted",
      action: "checkpoint",
    }),
    /caller_not_authorized/,
  );
  // A read-only workspace caller MAY still read that write-trusted child.
  const ok = await authorizeTarget({
    callerBinding,
    targetBinding: writeTrustedChild,
    hasExplicitChildGrant: true,
    childGrantLevel: writeTrustedChild.grant_policy.level,
    taskGrantLevel: "admin",
    installationLevel: "write-trusted",
    action: "source_read",
  });
  assert.equal(ok.effectiveLevel, "read-only");
});

test("an ungranted cross-root call is denied before any level check", async () => {
  await assert.rejects(
    authorizeTarget({
      callerBinding: { repository_id: "ws-repo", grant_policy: { level: "admin" } },
      targetBinding: { repository_id: "child-repo", grant_policy: { level: "admin" } },
      hasExplicitChildGrant: false,
      action: "context",
    }),
    /cross_root_binding_denied/,
  );
});

test("canReachTarget authorizes a granted sibling but omits an ungranted one", async () => {
  const callerBinding = { repository_id: "ws-repo", grant_policy: { level: "write-trusted", child_repository_ids: ["granted-child"] } };
  const granted = { repository_id: "granted-child", grant_policy: { level: "write-trusted" } };
  const ungrantedSibling = { repository_id: "other-child", grant_policy: { level: "write-trusted" } };
  // Granted sibling is reachable (non-null effective level).
  assert.ok(await canReachTarget({ callerBinding, targetBinding: granted, action: "context", hasExplicitChildGrant: true }));
  assert.ok(await canReachTarget({ callerBinding, targetBinding: granted, action: "context", taskGrantLevel: "write-trusted", hasExplicitChildGrant: true }));
  // Ungranted sibling is never reachable, even at an admin task grant.
  assert.equal(await canReachTarget({ callerBinding, targetBinding: ungrantedSibling, action: "context", taskGrantLevel: "admin", hasExplicitChildGrant: false }), null);
  assert.equal(await canReachTarget({ callerBinding, targetBinding: ungrantedSibling, action: "feedback", taskGrantLevel: "admin", hasExplicitChildGrant: false }), null);
});
