#!/usr/bin/env node
// right-release requires add-on `source` paths to stay repo-relative: it refuses
// absolute sources outright (packages/release/addon-contract.mjs). A managed
// build writes to an external target root instead, so cargo cannot put the
// executables where the add-on config declares them.
//
// Build through the managed toolchain, then stage the produced executables into
// the declared repo-relative path. The paths come from cargo's own artifact
// stream rather than a guess at the target directory, so this stays correct
// whether the build was managed, cached, or plain local.
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const repo = fileURLToPath(new URL("../../", import.meta.url));
const argv = process.argv.slice(2);
const separator = argv.indexOf("--");
if (separator < 0) throw new Error("usage: stage-addon-binaries.mjs <cargo args> -- <bin names>");
const cargoArgs = argv.slice(0, separator);
const wanted = new Set(argv.slice(separator + 1));
if (!wanted.size) throw new Error("at least one binary name is required");

const targetIndex = cargoArgs.indexOf("--target");
const target = targetIndex >= 0 ? cargoArgs[targetIndex + 1] : null;
if (!target) throw new Error("--target is required so the staged path is unambiguous");

// json-render-diagnostics keeps warnings human-readable on stderr while stdout
// stays a machine-readable artifact stream.
const result = spawnSync("cargo", [...cargoArgs, "--message-format=json-render-diagnostics"], {
  cwd: repo, encoding: "utf8", maxBuffer: 512 * 1024 * 1024, stdio: ["inherit", "pipe", "inherit"],
});
if (result.error) throw result.error;
if (result.status !== 0) throw new Error(`add-on cargo build failed with exit ${result.status}`);

const produced = new Map();
for (const line of (result.stdout ?? "").split("\n")) {
  if (!line.startsWith("{")) continue;
  let message;
  try { message = JSON.parse(line); } catch { continue; }
  if (message.reason !== "compiler-artifact" || !message.executable) continue;
  produced.set(message.target?.name, message.executable);
}

const destination = join(repo, "engine", "target", target, "release");
mkdirSync(destination, { recursive: true });
for (const name of wanted) {
  // The add-on declares the staged filename, which carries .exe on Windows,
  // while cargo names the artifact target without any extension.
  const source = produced.get(name.replace(/\.exe$/, ""));
  if (!source) throw new Error(`add-on build produced no executable named ${name}`);
  copyFileSync(source, join(destination, name));
  console.log(`[membrane] staged ${name} from ${source}`);
}
