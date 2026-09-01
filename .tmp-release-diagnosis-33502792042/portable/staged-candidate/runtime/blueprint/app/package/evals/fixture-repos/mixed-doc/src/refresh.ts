import config from "../config/blueprint.json" with { type: "json" };

export function refreshRepository(root: string) {
  return { root, maxFiles: config.maxFiles, refreshed: true };
}
