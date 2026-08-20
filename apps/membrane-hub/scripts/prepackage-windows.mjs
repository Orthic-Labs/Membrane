// Windows phase one: compile without an installer. RightKit patches Tauri's
// NSIS marker then signs raw EXE before phase two embeds it.
import { spawnSync } from "node:child_process";

function run(command, args) {
  const result = spawnSync(command, args, { stdio: "inherit", windowsHide: true });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} failed with exit ${result.status}`);
}

run("pnpm", ["run", "build"]);
run("node", ["scripts/runtime-inventory.mjs", "write"]);
run("pnpm", ["exec", "tauri", "build", "--no-bundle"]);
