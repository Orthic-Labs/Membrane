import { startExplorerServer } from "../http-server.mjs";
import { serveExplorerAsset } from "./static.mjs";

export async function startLocalExplorer({ root = process.cwd(), outDir = ".agent", service } = {}) {
  const applicationService = service ?? (await import("../application/service.mjs")).createBlueprintApplicationService({ outDir, allowEmbeddedRoot: true });
  return startExplorerServer({ service: applicationService, repoRoot: root, serveAsset: serveExplorerAsset });
}
