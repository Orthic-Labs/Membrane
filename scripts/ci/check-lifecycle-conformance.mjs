import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";

const root = resolve(import.meta.dirname, "../..");
for (const retired of [join(root, "install", "workspace"), join(root, "dist", "install", "workspace-manifest.json")]) {
  try { readFileSync(retired); throw new Error(`retired interpreter workspace package returned: ${retired}`); }
  catch (error) { if (error.code !== "ENOENT") throw error; }
}
const hubHandoff = readFileSync(join(root, "docs", "hub-handoff.md"), "utf8");
if (!hubHandoff.includes("no `supervisor-child`") || !hubHandoff.includes("Hub process")) {
  throw new Error("lifecycle contract must retain explicit in-process Hub ownership and retired-child denial");
}
