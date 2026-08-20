import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const generator = resolve(here, "generate-cli-subcommands.mjs");

test("checked-in Rust CLI projection is registry-current", () => {
  const result = spawnSync(process.execPath, [generator, "--check"], {
    encoding: "utf8",
  });
  assert.equal(result.status, 0, result.stderr || result.stdout);
});
