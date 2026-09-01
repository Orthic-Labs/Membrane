const COUNTERS = ["stale_results", "deltas_applied", "full_rebuilds", "alias_invocations"];
const SAMPLE_LIMIT = 256;

function ensureMeta(db) {
  db.exec("CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)");
}

function readValue(db, key, fallback = null) {
  ensureMeta(db);
  return db.prepare("SELECT value FROM meta WHERE key=?").get(key)?.value ?? fallback;
}

function writeValue(db, key, value) {
  ensureMeta(db);
  db.prepare("INSERT INTO meta(key,value) VALUES (?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").run(key, String(value));
}

export function incrementTelemetry(db, counter, amount = 1) {
  if (!COUNTERS.includes(counter)) throw new Error(`telemetry counter is not supported: ${counter}`);
  const current = Number(readValue(db, counter, 0));
  const next = current + Number(amount);
  writeValue(db, counter, next);
  return next;
}

export function recordBarrierDuration(db, elapsedMs) {
  const samples = (() => {
    try { return JSON.parse(readValue(db, "barrier_samples", "[]")); } catch { return []; }
  })();
  samples.push(Math.max(0, Number(elapsedMs)));
  const recent = samples.slice(-SAMPLE_LIMIT);
  const sorted = [...recent].sort((a, b) => a - b);
  const index = Math.min(sorted.length - 1, Math.ceil(sorted.length * 0.95) - 1);
  writeValue(db, "barrier_samples", JSON.stringify(recent));
  writeValue(db, "barrier_p95_ms", sorted.length ? sorted[Math.max(0, index)] : 0);
  return sorted.length ? sorted[Math.max(0, index)] : 0;
}

export function recordTokenSavings(db, result) {
  const existing = (() => {
    try { return JSON.parse(readValue(db, "token_savings", "[]")); } catch { return []; }
  })();
  existing.push(result);
  writeValue(db, "token_savings", JSON.stringify(existing.slice(-SAMPLE_LIMIT)));
  return result;
}

export function readTelemetry(db) {
  const counters = Object.fromEntries(COUNTERS.map((key) => [key, Number(readValue(db, key, 0))]));
  let tokenSavings = [];
  try { tokenSavings = JSON.parse(readValue(db, "token_savings", "[]")); } catch {}
  return {
    ...counters,
    barrier_p95_ms: Number(readValue(db, "barrier_p95_ms", 0)),
    token_savings: Array.isArray(tokenSavings) ? tokenSavings : [],
  };
}
