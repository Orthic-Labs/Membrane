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
  /^\s*(ls|cat|grep|rg)\b|^\s*git\s+(status|diff)\b/i;

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
  // Inspection commands remain unfenced.
  if (INSPECTION_PREFIX_RE.test(command)) return false;
  const lowered = command.toLowerCase();
  const toolLower = String(toolName ?? "").toLowerCase();
  return VERIFICATION_WORD_RE.test(lowered) || VERIFICATION_WORD_RE.test(toolLower);
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
