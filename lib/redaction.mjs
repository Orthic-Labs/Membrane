const SECRET_KEY = /token|secret|password|passwd|api[_-]?key|authorization|cookie|private[_-]?key|client_email/i;
const SECRET_VALUE = /(?:gh[pousr]_[A-Za-z0-9_]{20,}|github_pat_[A-Za-z0-9_]{30,}|npm_[A-Za-z0-9]{30,}|AKIA[0-9A-Z]{16}|-----BEGIN [A-Z ]*PRIVATE KEY-----|Bearer\s+[A-Za-z0-9._~-]+|sk-[A-Za-z0-9]{20,})/g;

export function redactForEgress(value) {
  if (Array.isArray(value)) return value.map(redactForEgress);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, SECRET_KEY.test(key) ? "[REDACTED]" : redactForEgress(item)]));
  }
  return typeof value === "string" ? value.replace(SECRET_VALUE, "[REDACTED]") : value;
}
