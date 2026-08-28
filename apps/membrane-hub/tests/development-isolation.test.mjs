import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const dev = readFileSync(new URL("../scripts/dev.mjs", import.meta.url), "utf8");

test("development Hub is isolated from installed runtime", () => {
  for (const term of [
    'MEMBRANE_RUNTIME_ORIGIN: "development"',
    "MEMBRANE_DEV_ROOT",
    "MEMBRANE_DEV_PORT",
    "MEMBRANE_CONFIG_ROOT",
    "MEMBRANE_DATA_ROOT",
    "MEMBRANE_CACHE_ROOT",
    "MEMBRANE_LOG_ROOT",
    '"Orthic Labs", "Membrane Dev"',
  ]) assert.ok(dev.includes(term), term);
});
