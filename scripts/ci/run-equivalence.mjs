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
// Legacy mcp Node twins are excluded. Native Rust parity is exercised below.
run(rightkit, [
  "cargo",
  "test",
  "--manifest-path",
  "engine/Cargo.toml",
  "-p",
  "membrane-mcp",
  "--locked",
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
