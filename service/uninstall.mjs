// D15: uninstall the resident watcher service. Preserves repository graph
// data unless --purge-data is explicit.

import { execFileSync } from "node:child_process";
import { rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { serviceTarget } from "./install.mjs";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));

export function uninstallService({ purgeData = false, dataDir = null } = {}) {
  const target = serviceTarget();
  if (process.platform === "darwin") {
    try { execFileSync("launchctl", ["unload", target]); } catch {}
  } else if (process.platform === "linux") {
    try { execFileSync("systemctl", ["--user", "disable", "--now", "cortex.service"]); } catch {}
  } else {
    try { execFileSync("schtasks", ["/Delete", "/F", "/TN", "OrthicCortex"]); } catch {}
  }
  rmSync(target, { force: true });
  const purged = [];
  if (purgeData) {
    const dir = dataDir ?? join(SCRIPT_DIR, "..", ".agent");
    rmSync(dir, { recursive: true, force: true });
    purged.push(dir);
  }
  return { uninstalled: true, target, purged };
}
