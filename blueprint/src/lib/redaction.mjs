const SECRET_KEY = /token|secret|password|passwd|api[_-]?key|authorization|cookie|private[_-]?key|client_email/i;
const SECRET_VALUE = /(?:gh[pousr]_[A-Za-z0-9_]{20,}|github_pat_[A-Za-z0-9_]{30,}|npm_[A-Za-z0-9]{30,}|AKIA[0-9A-Z]{16}|-----BEGIN [A-Z ]*PRIVATE KEY-----|Bearer\s+[A-Za-z0-9._~-]+|sk-[A-Za-z0-9_-]{20,}|sk-ant-[A-Za-z0-9_-]{20,}|xox[abpr]-[A-Za-z0-9-]{10,}|eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9._~\/+=-]{10,})/g;
const SECRET_URL_PASSWORD = /(?:postgres|postgresql|mysql|mongodb(?:\+srv)?|redis):\/\/[^:\s]+:([^@\s]+)@/gi;
const SECRET_RAW_BASE64_40 = /\b[A-Za-z0-9/+=]{40}\b/g;

export function redactForEgress(value) {
  if (Array.isArray(value)) return value.map(redactForEgress);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, SECRET_KEY.test(key) ? "[REDACTED]" : redactForEgress(item)]));
  }
  if (typeof value !== "string") return value;
  let out = value.replace(SECRET_VALUE, "[REDACTED]");
  out = out.replace(SECRET_URL_PASSWORD, (m, pw) => m.replace(pw, "[REDACTED]"));
  // Raw 40-char base64 (e.g. AWS secret access key w/out prefix) — broad but
  // safe to redact (false-positive is harmless, leak is not). Must run after
  // URL-password to avoid double-redacting.
  //
  // One false positive is NOT harmless: a git commit SHA is exactly 40 hex
  // characters, so this rule was rewriting `indexed_revision` inside every
  // freshness receipt that crossed the MCP egress boundary. That corrupts the
  // receipt — the revision a generation was indexed at is the evidence a caller
  // uses to reason about freshness, and a redacted one is indistinguishable
  // from a different generation. All-hex 40-char strings are hashes, not
  // credentials; anything under a secret-NAMED key is still redacted by
  // SECRET_KEY above, and prefixed credential shapes are matched by
  // SECRET_VALUE.
  out = out.replace(SECRET_RAW_BASE64_40, (match) => (/^[0-9a-f]{40}$/i.test(match) ? match : "[REDACTED]"));
  return out;
}
