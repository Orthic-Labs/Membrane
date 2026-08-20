// D52: canonical small/medium fixture for the soak fleet's "real" repo shape.
// The driver also accepts an arbitrary synthetic generation; this file is the
// representative shape the runbook's small-class budget maps onto.

import { writeFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";

export const SOAK_FIXTURE_PATHS = Object.freeze([
  "src/index.ts",
  "src/util.ts",
  "src/repo.ts",
  "docs/architecture.md",
  "package.json",
]);

export function writeSoakFixture(root) {
  mkdirSync(join(root, "src"), { recursive: true });
  mkdirSync(join(root, "docs"), { recursive: true });
  writeFileSync(join(root, "src/index.ts"), [
    "export const entry = () => 1;",
    "export function greet(name: string) { return `hi ${name}`; }",
    "",
  ].join("\n"));
  writeFileSync(join(root, "src/util.ts"), "export const MAGIC = 42;\n");
  writeFileSync(join(root, "src/repo.ts"), "import { MAGIC } from \"./util\";\nexport const value = MAGIC + 1;\n");
  writeFileSync(join(root, "docs/architecture.md"), "# Architecture\n\nA representative small fixture used by the soak fleet.\n");
  writeFileSync(join(root, "package.json"), JSON.stringify({ name: "soak-fixture", version: "0.0.0", type: "module" }, null, 2));
  return SOAK_FIXTURE_PATHS.slice();
}
