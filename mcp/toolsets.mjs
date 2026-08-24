import { readFile } from "node:fs/promises";

const version = "membrane.toolsets.v1";
const fallback = ["membrane_context"];
const names = new Set([
  "membrane_context", "membrane_source_read", "membrane_blueprint", "membrane_knowledge_propose", "membrane_checkpoint_save",
  "membrane_checkpoint_load", "membrane_working_context", "membrane_temporal_fact", "membrane_scratchpad", "membrane_feedback",
  "membrane_diagnostic_workspace", "membrane_diagnostic_mutation", "membrane_diagnostic_snapshot", "membrane_diagnostic_fence",
  "membrane_diagnostic_capabilities", "membrane_diagnostic_baseline", "membrane_diagnostic_provider",
]);

const groupNames = ["default", "memory", "blueprint", "diagnostic"];

export function parseToolsetConfig(raw) {
  try {
    const config = JSON.parse(raw);
    if (!config || typeof config !== "object" || Array.isArray(config) || Object.keys(config).sort().join(",") !== "groups,version" || config.version !== version) return null;
    const groups = config.groups;
    if (!groups || typeof groups !== "object" || Array.isArray(groups) || Object.keys(groups).sort().join(",") !== groupNames.slice().sort().join(",")) return null;
    for (const group of groupNames) {
      if (!Array.isArray(groups[group]) || new Set(groups[group]).size !== groups[group].length || groups[group].some((name) => typeof name !== "string" || !names.has(name))) return null;
    }
    if (groups.default.length !== 1 || groups.default[0] !== "membrane_context") return null;
    return groups;
  } catch { return null; }
}

const groups = parseToolsetConfig(await readFile(new URL("../schemas/registry/toolsets.yaml", import.meta.url), "utf8"));

export function toolsetNames(params = {}, configuredGroups = groups) {
  const requested = params?._meta?.[version];
  if (!configuredGroups || requested === undefined || !Array.isArray(requested) || new Set(requested).size !== requested.length || requested.some((group) => typeof group !== "string" || !Object.hasOwn(configuredGroups, group))) return fallback;
  return [...new Set([...configuredGroups.default, ...requested.flatMap((group) => configuredGroups[group])])];
}
