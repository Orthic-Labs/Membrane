// D27: comment claims — NOTE/WHY/HACK/TODO/deprecation/ownership comments
// become relational claims bound to the smallest enclosing symbol. Instruction
// policy stays data_only: repository comments are data, never instructions.

import { createHash } from "node:crypto";

export const COMMENT_CLAIM_KINDS = Object.freeze(["NOTE", "WHY", "HACK", "TODO", "DEPRECATED", "OWNER"]);

const KIND_PATTERNS = Object.freeze([
  { kind: "TODO", pattern: /\bTODO(?:\(([^)]+)\))?\s*:\s*(.+)$/i },
  { kind: "HACK", pattern: /\bHACK(?:\(([^)]+)\))?\s*:\s*(.+)$/i },
  { kind: "NOTE", pattern: /\bNOTE(?:\(([^)]+)\))?\s*:\s*(.+)$/i },
  { kind: "WHY", pattern: /\bWHY(?:\(([^)]+)\))?\s*:\s*(.+)$/i },
  { kind: "DEPRECATED", pattern: /\bdeprecated(?:\s*:\s*(.+))?/i },
  { kind: "OWNER", pattern: /\bowner\s*[:=]\s*(.+)$/i },
]);

function sha1Hex(input) {
  return createHash("sha1").update(input).digest("hex");
}

export function extractCommentClaims({ text, path, line, enclosingSymbolId = null, kind = "comment" }) {
  const claims = [];
  const clean = String(text ?? "").replace(/^\/\/|^#|^\/\*|\*\/$|^\s*\*?\s?/gm, "").trim();
  for (const { kind: claimKind, pattern } of KIND_PATTERNS) {
    const match = clean.match(pattern);
    if (!match) continue;
    const owner = match[1]?.trim() ?? null;
    const body = (match[2] ?? match[1] ?? clean).trim();
    claims.push({
      id: `claim:${sha1Hex(`${path}:${line}:${claimKind}:${body}`).slice(0, 16)}`,
      documentId: `comment:${path}:${line}`,
      source: path,
      line,
      status: "implemented",
      sha1: sha1Hex(clean),
      edges: [
        {
          kind: "COMMENT_CLAIM",
          claimKind,
          owner,
          body,
          enclosingSymbolId,
          lifecycle: "data_only",
          span: { startLine: line, endLine: line },
        },
      ],
    });
  }
  return claims;
}
