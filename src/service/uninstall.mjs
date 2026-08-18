// D15: uninstall — Hub-owned lifecycle (D-S03), no OS registration.
// Preserves repository graph data unless --purge-data is explicit; does not touch OS service managers.

import { rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));

export function uninstallService({ purgeData = false, dataDir = null } = {}) {
  const target = null; // D-S03: no OS target
  const purged = [];
  if (purgeData) {
    const dir = dataDir ?? join(SCRIPT_DIR, "..", "..", ".agent");
    rmSync(dir, { recursive: true, force: true });
    purged.push(dir);
  }
  return { uninstalled: true, target, purged, note: "OS service registration never existed per D-S03" };
}
