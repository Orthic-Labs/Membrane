// Phase 7.4 — Two-budget tokens for cortex.
//
// Until now every cortex candidate carried a SINGLE `estimatedTokens` field
// computed from metadata at slice time (line-count proxy). That number is a
// cheap upper bound suitable for assembly-time budgeting, but it is NOT the
// token cost of actually resolving the candidate to source. The plan
// requires two distinct budgets:
//
//   1. Slice-time metadata estimate
//      `sliceTokens(evidence)` — a pure metadata computation (line count
//      proxy). Cheap, no file reads, used by assemblers to admit or omit
//      candidates against a token ceiling.
//
//   2. Resolution-time byte/actual-span estimate
//      `resolveTokens(text)` — bytes-of-actual-content divided by a known
//      chars-per-token ratio. Computed ONLY after a real read of the
//      candidate's source bytes. Strictly more honest than the proxy,
//      always reported alongside the slice estimate so downstream consumers
//      can compare the two.
//
// Resolution-time estimates MUST also implement trim-to-symbol and MUST emit
// a truncation receipt when bytes are dropped. A silent trim is the defect:
// the planner cannot distinguish "the file is short" from "we threw half the
// file away". Both budget functions are pure; this module never reads files
// itself. Callers feed in the bytes they already have.

const CHARS_PER_TOKEN = 3.5;
const MAX_RETAINED_BYTES = 256 * 1024;

// A token is ~3.5 chars for prose / source code. Used both to convert byte
// counts to token counts and to cap the output of `sliceTokens`. The same
// constant appears in `_shared.mjs#estimateTokens` — kept as a local copy to
// avoid pulling adapter internals into the lib directory.
export function charsToTokens(chars) {
  const n = Number(chars);
  if (!Number.isFinite(n) || n <= 0) return 1;
  return Math.max(1, Math.round(n / CHARS_PER_TOKEN));
}

function readNumber(value, fallback) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : fallback;
}

// 1. SLICE-TIME METADATA ESTIMATE.
//
// Cheap, deterministic, no file reads. Adapter feeds this when a candidate
// is being proposed for admission; it never sees real bytes. Returns an
// integer >= 1.
export function sliceTokens(evidence) {
  if (!evidence) return 1;
  const startLine = readNumber(evidence.startLine, 1);
  const endLine = readNumber(evidence.endLine, startLine);
  return Math.max(1, endLine - startLine + 1);
}

// 2. RESOLUTION-TIME BYTE/ACTUAL-SPAN ESTIMATE.
//
// The adapter has READ THE FILE, so it knows actual byte counts. This budget
// reports that, plus the line span of the actual content (which may differ
// from `evidence.endLine` if trim-to-symbol expanded or shrank the slice).
// Returns an integer >= 1.
//
// Callers usually pass `actualText` (the bytes as a string) and the symbol
// span they resolved to (after trim-to-symbol, if any). The function derives
// bytes via `Buffer.byteLength(actualText, "utf8")` and an honest
// chars-per-token conversion.
export function resolveTokens(actualText, options = {}) {
  const text = String(actualText ?? "");
  if (!text.length) {
    return { tokens: 1, bytes: 0, lines: 0 };
  }
  const bytes = Buffer.byteLength(text, "utf8");
  const lines = text.length ? text.split(/\r?\n/).length : 0;
  return {
    tokens: charsToTokens(bytes || text.length),
    bytes,
    lines,
    span: {
      startLine: options.startLine ?? 1,
      endLine: options.endLine ?? lines,
    },
  };
}

// TRIM-TO-SYMBOL.
//
// When a candidate's evidence says `[startLine..endLine]` but the symbol
// actually declared in that span is smaller (e.g. startLine is mid-function
// because of a regex cut), expanding arbitrarily is a silent defect. This
// helper snaps the cut to the nearest preceding opening `{` and the next
// closing `}` / `;` on the given lines, so the retained slice is a complete
// syntactic unit — never mid-token.
//
// Inputs:
//   lines: array of source lines (1-indexed by convention; index math below
//          uses 0-indexed array access).
//   startLine, endLine: 1-indexed inclusive span that the caller wants to
//                       trim, normally the symbol evidence start/end.
//
// Output:
//   { startLine, endLine, trimmed: boolean, reason } where:
//     - trimmed is true ONLY if the input span had to shrink.
//     - reason is a short machine-readable tag, e.g. "snapped_to_braces".
//
// Semantics:
//   * Walks backward from the input startLine to find the nearest
//     `{` line. The line containing that brace is the new startLine.
//   * Walks forward from the input endLine to find the nearest `}` or `;`
//     line. That line is the new endLine.
//   * If the input span already lands on braces, trimmed = false.
//   * If no `{` is found before startLine (e.g. an arrow-function header
//     without braces), trimmed = false — the symbol really is one line.
export function trimToSymbol(lines, startLine, endLine) {
  if (!Array.isArray(lines) || lines.length === 0) {
    return { startLine: 1, endLine: 1, trimmed: false, reason: "no_lines" };
  }
  const total = lines.length;
  const inStart = Math.max(1, Math.min(total, Math.floor(startLine)));
  const inEnd = Math.max(inStart, Math.min(total, Math.floor(endLine)));
  // Walk backwards: pick the latest line AT OR BEFORE inStart that opens a
  // brace, then step forward to the matching `}`. We don't try to balance
  // braces (that's a parsing job for tree-sitter) — we just want the
  // enclosing block boundaries, which are always before/after the symbol.
  let braceStart = -1;
  for (let i = inStart - 1; i >= 0; i -= 1) {
    if (lines[i].includes("{")) {
      braceStart = i;
      break;
    }
  }
  let braceEnd = -1;
  for (let i = inEnd - 1; i < total; i += 1) {
    const line = lines[i];
    if (line.includes("}") || line.trimEnd().endsWith(";")) {
      braceEnd = i;
      break;
    }
  }
  const outStart = braceStart >= 0 ? braceStart + 1 : inStart;
  const outEnd = braceEnd >= 0 ? braceEnd + 1 : inEnd;
  const trimmed = outStart !== inStart || outEnd !== inEnd;
  const reason = braceStart >= 0 && braceEnd >= 0
    ? "snapped_to_braces"
    : braceStart >= 0
      ? "snapped_to_open_brace"
      : braceEnd >= 0
        ? "snapped_to_close_or_semicolon"
        : "no_trim_needed";
  return { startLine: outStart, endLine: outEnd, trimmed, reason };
}

// TRUNCATION RECEIPT.
//
// Built by callers whenever actual content is trimmed — either for budget
// pressure, a byte cap, or because the symbol's tail was off-screen. The
// receipt is the proof that the budget number on the wire corresponds to a
// shorter body than the original source. Without it, downstream cannot
// distinguish "candidate is small" from "candidate used to be big".
//
// `original` MUST come from `resolveTokens(actualText, ...)` so the byte and
// line counts are consistent with the budget number. `retained` is the same
// shape against the trimmed bytes.
//
// The receipt always includes a `reason` so the planner can audit whether
// the trim was intentional (per-symbol byte cap, per-call byte cap, byte
// ratio threshold) or accidental.
export function truncationReceipt(retained, original, options = {}) {
  if (!retained || !original) {
    throw new Error("truncationReceipt requires both retained and original resolveTokens results");
  }
  const originalBytes = Number(original.bytes) || 0;
  const retainedBytes = Number(retained.bytes) || 0;
  if (retainedBytes >= originalBytes) {
    throw new Error("truncationReceipt called when no truncation occurred — caller bug");
  }
  return {
    schema: "cortex.truncation.v1",
    retainedBytes,
    originalBytes,
    retainedLines: retained.lines ?? 0,
    originalLines: original.lines ?? 0,
    retainedTokens: retained.tokens,
    originalTokens: original.tokens,
    reason: String(options.reason ?? "byte_cap"),
    symbolBoundary: Boolean(options.symbolBoundary ?? false),
    trimmedAt: new Date().toISOString(),
  };
}

// CONVENIENCE: trim text by retained bytes, snap to symbol boundary, return
// both the trimmed text and the truncation receipt. Bundles the common case
// for callers that have a single symbol-shaped slice and want all three
// invariants (trim, snap, receipt) at once.
//
// Inputs:
//   lines, startLine, endLine: as in trimToSymbol.
//   maxBytes: caller-side cap (defaults to MAX_RETAINED_BYTES).
//
// Output:
//   { text, startLine, endLine, resolve: <resolveTokens result>, receipt: <truncationReceipt|null>, symbolBoundary }
export function applyTrimToSymbol(lines, startLine, endLine, options = {}) {
  const maxBytes = readNumber(options.maxBytes, MAX_RETAINED_BYTES);
  const snapped = trimToSymbol(lines, startLine, endLine);
  // Slice the lines (1-indexed inclusive)
  const text = lines
    .slice(Math.max(0, snapped.startLine - 1), snapped.endLine)
    .join("\n");
  const original = resolveTokens(text, { startLine: snapped.startLine, endLine: snapped.endLine });
  if (original.bytes <= maxBytes) {
    return {
      text,
      startLine: snapped.startLine,
      endLine: snapped.endLine,
      resolve: original,
      receipt: null,
      symbolBoundary: snapped.trimmed,
    };
  }
  // Trim to a soft byte cap. Pick a per-line byte share so we drop trailing
  // lines, never mid-line. Once a `}` / `;` lands us inside the cap, stop.
  let retainedBytes = 0;
  let retainedEnd = snapped.endLine;
  for (let i = snapped.startLine - 1; i < snapped.endLine; i += 1) {
    const lineBytes = Buffer.byteLength(lines[i] + "\n", "utf8");
    if (retainedBytes + lineBytes > maxBytes) break;
    retainedBytes += lineBytes;
    retainedEnd = i + 1;
  }
  const trimmedText = lines.slice(snapped.startLine - 1, retainedEnd).join("\n");
  const retained = resolveTokens(trimmedText, { startLine: snapped.startLine, endLine: retainedEnd });
  const receipt = truncationReceipt(retained, original, {
    reason: "byte_cap",
    symbolBoundary: snapped.trimmed,
  });
  return {
    text: trimmedText,
    originalText: text, // pre-byte-cap, post-snap-to-symbol
    startLine: snapped.startLine,
    endLine: retainedEnd,
    resolve: retained,
    original, // exposes the original resolveTokens for callers that don't want to re-derive it
    receipt,
    symbolBoundary: snapped.trimmed,
  };
}

export const _internals = {
  CHARS_PER_TOKEN,
  MAX_RETAINED_BYTES,
};

// D27: symbol/parent-aware chunk resolution. Trims only at AST/token
// boundaries (the enclosing symbol's exact span) and emits a mandatory
// truncation receipt when bytes are dropped. `symbol` may carry
// `{ startLine, endLine, parent: { startLine, endLine } }`.
export function chunkBySyntax({ text, symbol, maxBytes = MAX_RETAINED_BYTES } = {}) {
  const lines = String(text ?? "").split(/\r?\n/);
  const symbolSpan = symbol?.startLine != null ? { startLine: Number(symbol.startLine), endLine: Number(symbol.endLine ?? symbol.startLine) } : null;
  const parentSpan = symbol?.parent?.startLine != null ? { startLine: Number(symbol.parent.startLine), endLine: Number(symbol.parent.endLine ?? symbol.parent.startLine) } : null;
  const span = symbolSpan ?? parentSpan ?? { startLine: 1, endLine: lines.length };
  const bounded = {
    startLine: Math.max(1, Math.min(lines.length, span.startLine)),
    endLine: Math.max(span.startLine, Math.min(lines.length, span.endLine)),
  };
  const chunk = lines.slice(bounded.startLine - 1, bounded.endLine).join("\n");
  const original = resolveTokens(chunk, bounded);
  if (original.bytes <= maxBytes) {
    return {
      text: chunk,
      startLine: bounded.startLine,
      endLine: bounded.endLine,
      resolve: original,
      receipt: null,
      symbolBoundary: Boolean(symbolSpan),
    };
  }
  // Soft byte cap: drop trailing lines, never mid-line, never mid-token.
  let retainedBytes = 0;
  let retainedEnd = bounded.endLine;
  for (let i = bounded.startLine - 1; i < bounded.endLine; i += 1) {
    const lineBytes = Buffer.byteLength(lines[i] + "\n", "utf8");
    if (retainedBytes + lineBytes > maxBytes) break;
    retainedBytes += lineBytes;
    retainedEnd = i + 1;
  }
  const trimmedText = lines.slice(bounded.startLine - 1, retainedEnd).join("\n");
  const retained = resolveTokens(trimmedText, { startLine: bounded.startLine, endLine: retainedEnd });
  const receipt = truncationReceipt(retained, original, { reason: "syntax_chunk_byte_cap", symbolBoundary: Boolean(symbolSpan) });
  return { text: trimmedText, originalText: chunk, startLine: bounded.startLine, endLine: retainedEnd, resolve: retained, original, receipt, symbolBoundary: Boolean(symbolSpan) };
}
