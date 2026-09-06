function tokenize(value) {
  return String(value ?? "")
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .replace(/[_./:#-]+/g, " ")
    .toLowerCase()
    .split(/[^a-z0-9]+/)
    .filter((token) => token.length >= 2);
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
  }

  replace(documents = []) {
    this.documents.clear();
    for (const document of documents) this.documents.set(String(document.id), this.#normalize(document));
    this.#recompute();
    return this;
  }

  replaceDocument(document) {
    this.documents.set(String(document.id), this.#normalize(document));
    this.#recompute();
    return this;
  }

  removeDocument(id) {
    this.documents.delete(String(id));
    this.#recompute();
    return this;
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

  #recompute() {
    this.df.clear();
    let total = 0;
    for (const document of this.documents.values()) {
      total += document.length;
      for (const token of new Set(document.tokens)) this.df.set(token, (this.df.get(token) ?? 0) + 1);
    }
    this.avgdl = this.documents.size ? total / this.documents.size : 0;
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
    const flat = (value) => String(value ?? "").toLowerCase().replace(/[^a-z0-9]+/g, "");
    const queryWords = String(query ?? "")
      .split(/\s+/)
      .map(flat)
      .filter((word) => word.length >= 2);
    const names = (document) => [document.name, document.qualifiedName, document.path, document.signature];
    const covers = (document) => queryWords.some((word) => names(document).some((value) => {
      const candidate = flat(value);
      return candidate.length >= 2 && (candidate.includes(word) || word.includes(candidate));
    }));
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
