// D41: architecture rule DSL parser (S-22). Rules select from/to by path,
// symbol, label/type, layer, framework, edge kind, precision tier, and
// confidence threshold.

export const RULES_VERSION = 1;

export function parseRules(yaml) {
  // Minimal deterministic YAML subset for the pinned rule shape.
  const rules = [];
  const lines = String(yaml ?? "").split(/\r?\n/);
  let current = null;
  let section = null;
  const push = () => {
    if (current && current.id) rules.push(current);
    current = null;
    section = null;
  };
  for (const raw of lines) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    const version = line.match(/^version:\s*(\d+)$/);
    if (version) continue;
    const ruleStart = line.match(/^-\s*id:\s*(.+)$/);
    if (ruleStart) {
      push();
      current = { id: ruleStart[1].trim(), from: {}, disallow: {}, severity: "error" };
      continue;
    }
    if (!current) continue;
    if (line.startsWith("exceptions:")) { section = "exceptions"; continue; }
    if (section === "exceptions") continue;
    const fromMatch = line.match(/^from:\s*$/);
    if (fromMatch) { section = "from"; continue; }
    const disallowMatch = line.match(/^disallow:\s*$/);
    if (disallowMatch) { section = "disallow"; continue; }
    const toMatch = line.match(/^to:\s*$/);
    if (toMatch) { section = "to"; continue; }
    const kv = line.match(/^([a-zA-Z0-9_-]+):\s*(.+)$/);
    if (!kv) continue;
    let [key, value] = [kv[1], kv[2].trim()];
    if (value.startsWith('"') && value.endsWith('"')) value = value.slice(1, -1);
    if (value.startsWith("'") && value.endsWith("'")) value = value.slice(1, -1);
    if (key === "severity") { current.severity = value; continue; }
    if (key === "rationale") { current.rationale = value; continue; }
    if (key === "path") {
      if (section === "from") current.from.path = value;
      else if (section === "to") current.disallow.to = current.disallow.to ?? {};
      current.disallow.to = current.disallow.to ?? {};
      current.disallow.to.path = value;
      continue;
    }
    if (key === "minConfidence") { current.disallow.minConfidence = value; continue; }
    if (key === "edgeKinds" || key === "edge_kinds") {
      current.disallow.edgeKinds = value.replace(/^\[|\]$/g, "").split(",").map((s) => s.trim()).filter(Boolean);
      continue;
    }
  }
  push();
  return { version: RULES_VERSION, rules };
}
