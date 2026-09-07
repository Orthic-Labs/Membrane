// Identifier segmentation, aligned with Ledger's `add_component_aliases`
// (engine/crates/membrane-runtime/src/ledger/index.rs) so the two lexical
// engines agree on what a word is.
//
// Three boundaries, matching Ledger exactly:
//   lower/digit -> upper   `httpServer`     -> http server
//   upper-run   -> Upper+lower `HTTPServer` -> http server
//                              `XMLHttpRequest` -> xml http request
//   any non-alphanumeric   `user_account`, `src/a.b#c`
//
// Unicode: Ledger's FTS5 uses `unicode61 remove_diacritics 2` and normalizes
// queries NFKC + casefold. An ASCII-only `[^a-z0-9]` split silently erased
// every non-ASCII identifier here, so Blueprint and Ledger tokenized the same
// query differently. We normalize NFKC and split on `\p{L}`/`\p{N}` instead.
//
// No minimum token length. The canon says not to import a donor's
// minimum-token-length rule blindly, and a 1-character identifier (`x`, `i`,
// `n`, a CJK ideograph) is real code that the >= 2 floor dropped from both the
// index and the query. The floor is retained only where it does real work —
// the admission gate below, where it is a containment guard, not a token rule.
function tokenize(value) {
  return String(value ?? "")
    .normalize("NFKC")
    .replace(/(\p{Ll}|\p{N})(\p{Lu})/gu, "$1 $2")
    .replace(/(\p{Lu}+)(\p{Lu}\p{Ll})/gu, "$1 $2")
    .toLowerCase()
    .split(/[^\p{L}\p{N}]+/u)
    .filter(Boolean);
}

function termFrequency(tokens) {
  const counts = new Map();
  for (const token of tokens) counts.set(token, (counts.get(token) ?? 0) + 1);
  return counts;
}

export class Bm25CodeIndex {
  constructor({ k1 = 1.2, b = 0.75 } = {}) {
    this.k1 = k1;
    this.b = b;
    this.documents = new Map();
    this.df = new Map();
    this.avgdl = 0;
    // Sum of document lengths. Integer, so avgdl is bit-identical to the sum a
    // full rebuild would compute in the same insertion-independent way.
    this.totalLength = 0;
  }

  replace(documents = []) {
    this.documents.clear();
    this.df.clear();
    this.totalLength = 0;
    for (const document of documents) this.#insert(this.#normalize(document));
    this.#refreshAvgdl();
    return this;
  }

  // Incremental df/avgdl maintenance. A single changed document used to trigger
  // an unconditional full-corpus rebuild, which the canon prohibits. df is a
  // count of documents containing a term, so it is exactly maintainable: retract
  // the outgoing document's unique terms, apply the incoming one's.
  replaceDocument(document) {
    const id = String(document.id);
    const previous = this.documents.get(id);
    if (previous) this.#retract(previous);
    this.#insert(this.#normalize(document));
    this.#refreshAvgdl();
    return this;
  }

  removeDocument(id) {
    const previous = this.documents.get(String(id));
    if (previous) {
      this.#retract(previous);
      this.#refreshAvgdl();
    }
    return this;
  }

  #insert(document) {
    this.documents.set(document.id, document);
    this.totalLength += document.length;
    for (const token of new Set(document.tokens)) this.df.set(token, (this.df.get(token) ?? 0) + 1);
  }

  #retract(document) {
    this.documents.delete(document.id);
    this.totalLength -= document.length;
    for (const token of new Set(document.tokens)) {
      const next = (this.df.get(token) ?? 0) - 1;
      // Deleting at zero is what keeps the incremental df map deep-equal to a
      // rebuilt one: a rebuild never materializes a zero entry.
      if (next > 0) this.df.set(token, next);
      else this.df.delete(token);
    }
  }

  #refreshAvgdl() {
    this.avgdl = this.documents.size ? this.totalLength / this.documents.size : 0;
  }

  #normalize(document) {
    const weighted = [
      document.name, document.name,
      document.qualifiedName, document.qualifiedName,
      document.path,
      ...(document.identifiers ?? []),
      document.signature,
    ].filter(Boolean).join(" ");
    const tokens = tokenize(weighted);
    return { ...document, id: String(document.id), tokens, tf: termFrequency(tokens), length: Math.max(1, tokens.length) };
  }

  search(query, { limit = 20 } = {}) {
    const terms = [...new Set(tokenize(query))];
    if (!terms.length || !this.documents.size) return [];
    // Admission rule: a query word and a candidate identifier must NAME each
    // other — one's normalized text must contain the other's.
    //
    // BM25 is a ranking function, not a candidate-selection rule. Scoring every
    // document that shares any subtoken makes `oldValue` report `stableValue`,
    // which shares only the generic `value` half. Two earlier attempts gated on
    // subtoken statistics — a coverage fraction, then document-frequency rarity
    // — and both were tunings: the first survived only at exactly two
    // subtokens, and the second made admission depend on the rest of the
    // corpus, so indexing two unrelated `old*` symbols re-admitted
    // `stableValue`. A rule whose behaviour changes when unrelated documents
    // are added is not a rule.
    //
    // Containment is structural and corpus-independent: it reads only the
    // query and the one document being judged, so the same pair always decides
    // the same way however large the index grows. It is symmetric because both
    // directions are real code-search shapes — `value` should find
    // `stableValue` (query inside identifier), and an over-specified
    // `UserAccountRepository` should still find `UserAccountRepo` (identifier
    // inside query). `oldValue` and `stableValue` contain neither, so the
    // generic-half match is refused. Ranking among admitted documents remains
    // BM25's job, unchanged.
    //
    // The >= 2 length floor below is NOT the tokenizer's discarded minimum-token
    // rule. Here it guards containment specifically: a single character is a
    // substring of almost every identifier, so `candidate.includes(word)` on a
    // 1-character word would admit the whole corpus. A 1-character query word is
    // still a real identifier, so instead of being dropped it is admitted by
    // exact token equality against the document's own tokens — which reads only
    // this document, preserving corpus-independence.
    const flat = (value) => String(value ?? "").normalize("NFKC").toLowerCase().replace(/[^\p{L}\p{N}]+/gu, "");
    const queryWords = String(query ?? "")
      .split(/\s+/)
      .map(flat)
      .filter(Boolean);
    const names = (document) => [document.name, document.qualifiedName, document.path, document.signature];
    const covers = (document) => queryWords.some((word) => {
      if ([...word].length < 2) return document.tf.has(word);
      return names(document).some((value) => {
        const candidate = flat(value);
        return [...candidate].length >= 2 && (candidate.includes(word) || word.includes(candidate));
      });
    });
    const n = this.documents.size;
    const rows = [];
    for (const document of this.documents.values()) {
      if (queryWords.length && !covers(document)) continue;
      let score = 0;
      const contributions = [];
      for (const term of terms) {
        const tf = document.tf.get(term) ?? 0;
        if (!tf) continue;
        const df = this.df.get(term) ?? 0;
        const idf = Math.log(1 + ((n - df + 0.5) / (df + 0.5)));
        const denominator = tf + this.k1 * (1 - this.b + this.b * (document.length / (this.avgdl || 1)));
        const termScore = idf * ((tf * (this.k1 + 1)) / denominator);
        score += termScore;
        contributions.push({ term, score: termScore });
      }
      if (score <= 0) continue;
      const exactName = String(document.name ?? "").toLowerCase() === String(query).trim().toLowerCase();
      rows.push({ id: document.id, score: score + (exactName ? 1000 : 0), exactName, contributions, document });
    }
    rows.sort((a, b) => Number(b.exactName) - Number(a.exactName) || b.score - a.score || a.id.localeCompare(b.id));
    return rows.slice(0, Math.max(1, Math.min(200, Number(limit) || 20)));
  }
}

export function buildBm25CodeIndex(generation) {
  const documents = (generation?.nodes ?? [])
    .filter((node) => node?.kind === "symbol" || node?.kind === "class" || node?.labels?.some((label) => ["Function", "Method", "Class", "Interface", "Trait", "Test", "Screen"].includes(label)))
    .map((node) => ({
      id: node.id,
      name: node.name ?? "",
      qualifiedName: node.qualifiedName ?? "",
      path: node.path ?? "",
      signature: node.signature ?? node.rawDeclaredType ?? "",
      identifiers: node.labels ?? [],
      node,
    }));
  return new Bm25CodeIndex().replace(documents);
}

export { tokenize as tokenizeCodeIdentifiers };
