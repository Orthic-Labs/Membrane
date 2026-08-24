// Single verification-command classifier for the Semantic Edit Fence.
//
// Recognizes verification/escalation commands that must be gated by a
// cleared fence in opted-in workspaces, while leaving ordinary inspection
// commands unfenced. One table-driven helper is the single authority; both
// hook runtime and workspace operations must import this.
//
// Verification surface (design §10): test, check, build, compile, release,
// publish plus their common tool-wrapped forms (cargo test/check/build,
// pnpm test/build, npm test / npm run build, yarn test/build, make,
// gradle, mvn, go test/build). Ordinary inspection commands remain
// unfenced: ls, cat, grep, rg, git status, git diff.

const INSPECTION_PREFIX_RE =
  /^\s*(?:ls|cat|grep|rg)(?:\s|$)|^\s*git\s+(?:status|diff)(?:\s|$)/i;

// Standalone build-system entrypoints that are themselves verification even
// without an explicit verb token alongside them.
const VERIFICATION_WORD_RE =
  /\b(test|tests|check|build|compile|release|publish|make|gradle|mvn)\b/i;

/**
 * Whether `rawCommand` (and optionally `toolName`) crosses the verification
 * boundary. Inspection commands are never verification even when their
 * argument text happens to contain a keyword (e.g. `grep -r test`).
 *
 * @param {string} rawCommand
 * @param {string} [toolName]
 * @returns {boolean}
 */
export function isVerificationCommand(rawCommand, toolName = "") {
  const command = String(rawCommand ?? "").trim();
  if (!command) return false;
  const lowered = command.toLowerCase();
  const toolLower = String(toolName ?? "").toLowerCase();
  const segments = splitTopLevelSegments(command);
  // A malformed shell fragment is unsafe to classify as inspection when it
  // contains a verification token: fail closed.
  if (segments.unbalanced && (VERIFICATION_WORD_RE.test(lowered) || VERIFICATION_WORD_RE.test(toolLower))) return true;
  return segments.parts.some((part) => {
    if (INSPECTION_PREFIX_RE.test(part)) return false;
    return VERIFICATION_WORD_RE.test(part.toLowerCase());
  }) || VERIFICATION_WORD_RE.test(toolLower);
}

/** Split only top-level shell separators; this deliberately is not a shell
 * parser. Quotes & escaped separators remain inside one bounded segment. */
function splitTopLevelSegments(command) {
  const parts = [];
  let current = "";
  let quote = null;
  let escaped = false;
  for (let index = 0; index < command.length; index += 1) {
    const character = command[index];
    if (escaped) {
      current += character;
      escaped = false;
      continue;
    }
    if (character === "\\") {
      current += character;
      escaped = true;
      continue;
    }
    if (quote) {
      current += character;
      if (character === quote) quote = null;
      continue;
    }
    if (character === "'" || character === '"' || character === "`") {
      quote = character;
      current += character;
      continue;
    }
    if (character === ";" || character === "\n" || character === "|") {
      if (current.trim()) parts.push(current.trim());
      current = "";
      if ((character === "|" || character === "&") && command[index + 1] === character) index += 1;
      else if (character === "&" && command[index + 1] === "&") index += 1;
      continue;
    }
    if (character === "&" && command[index + 1] === "&") {
      if (current.trim()) parts.push(current.trim());
      current = "";
      index += 1;
      continue;
    }
    current += character;
  }
  if (current.trim()) parts.push(current.trim());
  return { parts, unbalanced: Boolean(quote || escaped) };
}

export const VERIFICATION_KEYWORDS = Object.freeze([
  "test",
  "check",
  "build",
  "compile",
  "release",
  "publish",
]);

export const INSPECTION_COMMANDS = Object.freeze([
  "ls",
  "cat",
  "grep",
  "rg",
  "git status",
  "git diff",
]);
