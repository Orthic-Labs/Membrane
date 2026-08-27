import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const rightkit = process.env.RIGHTKIT || (process.platform === "win32" ? "rightkit.cmd" : "rightkit");

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: root,
    stdio: "inherit",
    windowsHide: true,
    shell: command.endsWith(".cmd"),
  });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

run(rightkit, [
  "cargo",
  "build",
  "--manifest-path",
  "engine/Cargo.toml",
  "-p",
  "cortex",
  "--bin",
  "cortex",
  "--locked",
]);
run("node", [
  "--test",
  "mcp/scope-grant-v1.test.mjs",
  "mcp/deadline.test.mjs",
  "mcp/delivery-serialization.test.mjs",
]);
run(rightkit, [
  "cargo",
  "test",
  "--manifest-path",
  "engine/Cargo.toml",
  "-p",
  "cortex",
  "--test",
  "doc_spine",
  "--test",
  "doc_spine_equivalence",
  "--locked",
]);
run(rightkit, [
  "cargo",
  "test",
  "--manifest-path",
  "engine/Cargo.toml",
  "-p",
  "membrane-runtime",
  "pull::",
  "--locked",
]);
