// D33: HTTP framework providers — import-gated route→handler mapping for
// JS/TS (Next.js + Express/Fastify), Python (FastAPI + Django), and Rust
// (Tauri + Axum). Framework edges default to CROSS_FILE_HEURISTIC; they are
// upgraded only with compiler/runtime evidence.

export const FRAMEWORK_GATES = Object.freeze({
  "next-express": ["next", "express", "fastify"],
  "fastapi-django": ["fastapi", "django"],
  "tauri-axum": ["tauri", "axum"],
});

export function frameworkGateActive(imports = [], stack) {
  const gates = FRAMEWORK_GATES[stack] ?? [];
  return gates.some((gate) => imports.some((imp) => imp.includes(gate)));
}

// Deterministic route extraction from literal/decorator/config evidence.
export function extractRoutes({ stack, text, path }) {
  const routes = [];
  const lines = String(text ?? "").split(/\r?\n/);
  for (let i = 0; i < lines.length; i += 1) {
    const raw = lines[i];
    let line = raw.trim();
    if (line.startsWith("//") || line.startsWith("#") || line.startsWith("/*") || line.startsWith("*")) continue;
    // Strip inline comments so `code; // app.get('/x')` never fabricates a route.
    line = line.replace(/\/\/.*$/, "").replace(/#.*$/, "").trim();
    let match;
    if (stack === "next-express") {
      match = line.match(/(?:app|router)\.(get|post|put|delete|patch)\(\s*["'`]([^"'`]+)["'`]/i);
      if (match) routes.push({ kind: "route", method: match[1].toUpperCase(), path: match[2], line: i + 1, confidence: "CROSS_FILE_HEURISTIC", evidence: `${path}:${i + 1}` });
      match = line.match(/export\s+default\s+async\s+function\s+([A-Za-z0-9_]+)/);
      if (match) routes.push({ kind: "handler", name: match[1], line: i + 1, confidence: "CROSS_FILE_HEURISTIC", evidence: `${path}:${i + 1}` });
    } else if (stack === "fastapi-django") {
      match = line.match(/@app\.(get|post|put|delete|patch)\(\s*["'`]([^"'`]+)["'`]/i);
      if (match) routes.push({ kind: "route", method: match[1].toUpperCase(), path: match[2], line: i + 1, confidence: "CROSS_FILE_HEURISTIC", evidence: `${path}:${i + 1}` });
      match = line.match(/^def\s+([A-Za-z0-9_]+)\s*\(/);
      if (match) routes.push({ kind: "handler", name: match[1], line: i + 1, confidence: "CROSS_FILE_HEURISTIC", evidence: `${path}:${i + 1}` });
    } else if (stack === "tauri-axum") {
      match = line.match(/\.route\(\s*["'`]([^"'`]+)["'`]/i);
      if (match) routes.push({ kind: "route", method: "ANY", path: match[1], line: i + 1, confidence: "CROSS_FILE_HEURISTIC", evidence: `${path}:${i + 1}` });
      match = line.match(/async\s+fn\s+([A-Za-z0-9_]+)/);
      if (match) routes.push({ kind: "handler", name: match[1], line: i + 1, confidence: "CROSS_FILE_HEURISTIC", evidence: `${path}:${i + 1}` });
    }
  }
  return routes;
}
