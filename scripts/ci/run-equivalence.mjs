import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: root,
    stdio: "inherit",
    windowsHide: true,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

const python = process.platform === "win32" ? "py" : "python3";
const pythonPrefix = process.platform === "win32" ? ["-3.11"] : [];
run("rightkit", [
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
run(python, [
  ...pythonPrefix,
  "-m",
  "pytest",
  "-q",
  "engine/federation/test_equivalence_net.py",
  "engine/federation/test_warm_receipts.py",
  "engine/federation/test_gateway_merge_order.py",
  "engine/federation/test_gateway_failure_taxonomy.py",
  "engine/federation/test_gateway_freshness.py",
  "engine/federation/test_gateway_stdio.py",
]);
run("node", [
  "--test",
  "mcp/scope-grant-v1.test.mjs",
  "mcp/deadline.test.mjs",
  "mcp/delivery-serialization.test.mjs",
]);
run("rightkit", [
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
run("rightkit", [
  "cargo",
  "test",
  "--manifest-path",
  "engine/Cargo.toml",
  "-p",
  "membrane-runtime",
  "runc::tests::",
  "--locked",
]);
