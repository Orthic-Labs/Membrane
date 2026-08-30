import assert from "node:assert/strict";
import test from "node:test";
import { sidecarBuildCommand } from "./sidecar-build-command.mjs";

test("default sidecar build preserves RightKit broker command", () => {
  assert.deepEqual(sidecarBuildCommand({ environment: {}, platform: "darwin" }), { command: "rightkit", prefix: ["cargo"] });
  assert.deepEqual(sidecarBuildCommand({ environment: { RIGHTKIT: "/opt/rightkit" }, platform: "win32" }), { command: "/opt/rightkit", prefix: ["cargo"] });
});

test("public CI direct Cargo mode removes only broker prefix", () => {
  assert.deepEqual(sidecarBuildCommand({ environment: { MEMBRANE_PUBLIC_CI_DIRECT_CARGO: "1", RIGHTKIT: "/opt/rightkit" }, platform: "darwin" }), { command: "cargo", prefix: [] });
});
