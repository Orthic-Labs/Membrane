import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";

const root = resolve(import.meta.dirname, "../..");
const workspaceInstall = join(root, "install", "workspace");
const registrationFiles = [
  "cortex_service_" + "launchd.py",
  "cortex_service_" + "registrars.py",
];
const prohibited = ["launch" + "d", "system" + "d", "scht" + "asks"];

for (const file of registrationFiles.map((name) => join(workspaceInstall, name))) {
  if (existsSync(file)) throw new Error(`OS lifecycle registration file must not exist: ${file}`);
}
const source = readFileSync(join(workspaceInstall, "cortex_service.py"), "utf8").toLowerCase();
for (const token of prohibited) {
  if (source.includes(token)) throw new Error(`OS lifecycle registration must not return: ${token}`);
}
const hubHandoff = readFileSync(join(root, "docs", "hub-handoff.md"), "utf8");
if (!hubHandoff.includes("membrane service run") || !hubHandoff.includes("Hub")) {
  throw new Error("lifecycle contract must retain explicit standalone service run and Hub ownership");
}
