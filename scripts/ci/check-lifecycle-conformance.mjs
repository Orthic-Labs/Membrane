import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";

const root = resolve(import.meta.dirname, "../..");
const workspaceInstall = join(root, "install", "workspace");
const prohibited = ["launch" + "d"];
const source = readFileSync(join(workspaceInstall, "membrane_shims.py"), "utf8").toLowerCase();
for (const token of prohibited) {
  if (source.includes(token)) throw new Error(`OS lifecycle registration must not return: ${token}`);
}
const hubHandoff = readFileSync(join(root, "docs", "hub-handoff.md"), "utf8");
if (!hubHandoff.includes("no `supervisor-child`") || !hubHandoff.includes("Hub process")) {
  throw new Error("lifecycle contract must retain explicit in-process Hub ownership and retired-child denial");
}
