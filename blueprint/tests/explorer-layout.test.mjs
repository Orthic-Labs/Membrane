import assert from "node:assert/strict";
import test from "node:test";
import { projectOrb, sphericalLayout } from "../src/lib/explorer/layout.mjs";

test("spherical layout is deterministic and bounded", () => {
  const input = [{ id: "c" }, { id: "a" }, { id: "b" }];
  const first = sphericalLayout(input, { radius: 10 });
  assert.deepEqual(first, sphericalLayout(input, { radius: 10 }));
  assert.deepEqual(first.map((node) => node.id), ["a", "b", "c"]);
  for (const node of first) assert.ok(Math.hypot(node.x, node.y, node.z) <= 10.000001);
  assert.equal(projectOrb(first).length, 3);
});
