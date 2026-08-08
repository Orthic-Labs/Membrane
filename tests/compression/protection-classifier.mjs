const KINDS = new Set([
  "criteria", "error", "identifier", "hash", "quoted_evidence", "security_finding", "complete_read",
]);

const RULES = [
  ["criteria", /\b(?:acceptance criteria|criterion|must|shall|required|expected)\b/i],
  ["error", /\b(?:error|failed|failure|fatal|panic|exception|not ok)\b/i],
  ["identifier", /(?:\b[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\b|\b(?:(?:candidate|receipt|trace|task|session|scope)[_-]?id|id)\s*[:=]\s*[^\s,;]+)/i], // UUID boundaries use ASCII [A-Za-z0-9_], matching Rust is_word.
  ["hash", /(?:sha256:[0-9a-f]{64}|\b[0-9a-f]{40}\b)/],
  ["quoted_evidence", /^[\t ]*(?:>|["'`])/],
  ["security_finding", /\b(?:CVE-\d{4}-\d+|CWE-\d+|security finding|vulnerab(?:ility|le)|severity\s*[:=]\s*(?:critical|high|medium|low))\b/i],
];
const COMPLETE_READ = /\[complete-read\]|complete_read=true|completeread=true|begin complete read/i;

function byteLines(text) {
  const bytes = Buffer.from(text, "utf8"), lines = [];
  for (let start = 0, end; start < bytes.length; start = end) {
    end = bytes.indexOf(0x0a, start); end = end < 0 ? bytes.length : end + 1;
    lines.push({ start, end, text: bytes.subarray(start, end).toString("utf8") });
  }
  return { bytes, lines };
}

export function classifyProtectedEvidence({ text, kind, completeRead = false, protected: explicit = false }) {
  if (typeof text !== "string") throw new TypeError("protected evidence text must be a string");
  const { bytes, lines } = byteLines(text);
  if (!bytes.length) return [];
  if (completeRead || explicit || KINDS.has(kind) || lines.some((line) => COMPLETE_READ.test(line.text))) return [{ start: 0, end: bytes.length, reason: completeRead || lines.some((line) => COMPLETE_READ.test(line.text)) ? "complete_read" : kind || "explicit" }];
  return lines.flatMap((line) => {
    const match = RULES.find(([, pattern]) => pattern.test(line.text));
    return match ? [{ start: line.start, end: line.end, reason: match[0] }] : [];
  });
}

export function protectedBytes(text, spans) {
  const bytes = Buffer.from(text, "utf8");
  return Buffer.concat(spans.map(({ start, end }) => bytes.subarray(start, end)));
}

export const PROTECTED_EVIDENCE_KINDS = Object.freeze([...KINDS]);
