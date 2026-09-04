// Bounded SQL selection must use the same categorical authority as the later
// graph ranking. Sorting nullable confidence first discards compiler facts
// before an application-level comparator ever sees them.
import { semanticAuthorityRankForFact } from "./evidence-authority.mjs";

const registered = new WeakSet();
const FUNCTION_NAME = "blueprint_symbol_authority_rank";
const TIE_FUNCTION = "blueprint_symbol_confidence_tiebreak";

function metadata(extra) {
  if (extra === null || extra === undefined) return {};
  if (typeof extra !== "string") return null;
  try { const value = JSON.parse(extra); return value && typeof value === "object" && !Array.isArray(value) ? value : null; }
  catch { return null; }
}

export function symbolAuthorityOrder(db, alias = "") {
  if (alias !== "" && !/^[A-Za-z_][A-Za-z0-9_]*$/.test(alias)) {
    throw new TypeError("symbol authority SQL alias must be an identifier");
  }
  if (!registered.has(db)) {
    // Connection-local and read-only: no schema changes, writes, or graph-wide
    // hydration. The central evaluator remains the sole provenance classifier.
    db.function(FUNCTION_NAME, { deterministic: true }, (extra) => {
      return semanticAuthorityRankForFact(metadata(extra));
    });
    db.function(TIE_FUNCTION, { deterministic: true }, (extra, confidence) => {
      const fact = metadata(extra);
      if (!fact || typeof confidence !== "number" || !Number.isFinite(confidence) || confidence < 0 || confidence > 1) return -1;
      // Preserve the frozen V1 retrieval order only within genuinely untagged
      // legacy rows. This never supplies authority or outranks a categorical
      // fact. Tagged data may consult confidence only for heuristic evidence.
      const legacy = fact.provenance == null && fact.precisionTier == null
        && fact.provider == null && fact.sourceProvider == null
        && fact.confidenceTier == null && fact.resolved == null;
      return legacy || semanticAuthorityRankForFact(fact) === semanticAuthorityRankForFact({ provenance: "HEURISTIC_BRIDGE" }) ? confidence : -1;
    });
    registered.add(db);
  }
  return `${FUNCTION_NAME}(${alias ? `${alias}.` : ""}extra)`;
}

// The secondary ordering is deliberately a separate SQL expression: the
// categorical key must ALWAYS precede it at every bounded selection site.
export function symbolConfidenceTieOrder(db, alias = "") {
  symbolAuthorityOrder(db, alias);
  const prefix = alias ? `${alias}.` : "";
  return `${TIE_FUNCTION}(${prefix}extra, ${prefix}confidence) DESC`;
}
