#!/usr/bin/env node
// D13: SPDX SBOM builder — emits an SPDX-2.3 JSON SBOM for a release
// candidate directory.

import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export function buildSbom({ root, version, commit, packages = [] }) {
  return {
    spdxVersion: "SPDX-2.3",
    dataLicense: "CC0-1.0",
    SPDXID: "SPDXRef-DOCUMENT",
    name: `blueprint-${version}`,
    documentNamespace: `https://orthic-labs.github.io/spdx/blueprint-${version}-${String(commit).slice(0, 12)}`,
    creationInfo: { created: new Date().toISOString(), creators: ["Tool: blueprint sbom builder"] },
    packages: [
      { name: "blueprint", SPDXID: "SPDXRef-Package-Blueprint", versionInfo: version, downloadLocation: "NOASSERTION", filesAnalyzed: false },
      ...packages,
    ],
  };
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  const dir = process.argv[2];
  if (!dir) { console.error("usage: sbom.mjs <candidate-dir>"); process.exit(1); }
  const compatibility = JSON.parse(readFileSync(join(dir, "compatibility.json"), "utf8"));
  const sbom = buildSbom({ root: dir, version: compatibility.version, commit: compatibility.commit });
  process.stdout.write(`${JSON.stringify(sbom, null, 2)}\n`);
}
