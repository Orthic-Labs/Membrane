// D41: rule exceptions — require owner, rationale, expiry, rule ID, and
// bounded scope. Expired or malformed exceptions never suppress findings.

export function isExceptionValid(exception, { now = Date.now() } = {}) {
  if (!exception) return false;
  const required = ["owner", "rationale", "expires", "ruleId", "scope"];
  for (const key of required) {
    if (exception[key] === undefined || exception[key] === null || exception[key] === "") return false;
  }
  const expiry = new Date(exception.expires).getTime();
  if (!Number.isFinite(expiry)) return false;
  if (expiry <= now) return false;
  return true;
}

export function suppressedByException(finding, exceptions = [], { now = Date.now() } = {}) {
  for (const exception of exceptions) {
    if (!isExceptionValid(exception, { now })) continue;
    if (exception.ruleId !== finding.ruleId) continue;
    if (exception.scope === "global") return true;
    if (exception.scope === "path" && exception.path && String(finding.source).startsWith(exception.path)) return true;
  }
  return false;
}
