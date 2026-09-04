// Bounded SQL selection must use the same categorical authority as the later
// graph ranking. Sorting nullable confidence first discards compiler facts
// before an application-level comparator ever sees them.
import { semanticAuthorityRankForFact } from "./evidence-authority.mjs";

const registered = new WeakSet();
const FUNCTION_NAME = "blueprint_symbol_authority_rank";

export function symbolAuthorityOrder(db, alias = "") {
  if (alias !== "" && !/^[A-Za-z_][A-Za-z0-9_]*$/.test(alias)) {
    throw new TypeError("symbol authority SQL alias must be an identifier");
  }
  if (!registered.has(db)) {
    // Connection-local and read-only: no schema changes, writes, or graph-wide
    // hydration. The central evaluator remains the sole provenance classifier.
    db.function(FUNCTION_NAME, { deterministic: true }, (extra) => {
      let fact = {};
      if (typeof extra === "string" && extra.length) {
        try { fact = JSON.parse(extra); } catch { /* malformed metadata has no authority */ }
      }
      return semanticAuthorityRankForFact(fact);
    });
    registered.add(db);
  }
  return `${FUNCTION_NAME}(${alias ? `${alias}.` : ""}extra)`;
}
