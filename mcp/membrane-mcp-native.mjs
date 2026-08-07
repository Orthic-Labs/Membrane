#!/usr/bin/env node
import { spawn } from "node:child_process";
import { resolve } from "node:path";

const binary = process.env.MEMBRANE_BIN || resolve(new URL("../engine/target/debug/membrane", import.meta.url).pathname);
const child = spawn(binary, ["stdio-mcp"], { stdio: ["inherit", "inherit", "inherit"] });
child.on("error", (error) => { process.stderr.write(`membrane-mcp-native: ${error.message}\n`); process.exitCode = 1; });
child.on("exit", (code, signal) => { process.exitCode = code ?? (signal ? 1 : 0); });
