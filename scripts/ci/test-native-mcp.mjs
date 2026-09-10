#!/usr/bin/env node
import { spawnSync } from "node:child_process";

const args = [
  "cargo",
  "test",
  "--manifest-path",
  "engine/Cargo.toml",
  ...(process.platform === "win32" ? ["--target", "x86_64-pc-windows-msvc"] : []),
  "-p",
  "membrane-mcp",
  "--locked",
  "--no-fail-fast",
];
const command = process.env.RIGHTKIT || (process.platform === "win32" ? "rightkit.cmd" : "rightkit");
const result = spawnSync(command, args, { stdio: "inherit", windowsHide: true, shell: process.platform === "win32" });
process.exit(result.status ?? 1);
