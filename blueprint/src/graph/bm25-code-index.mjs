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
    // A code identifier is atomic. `oldValue` tokenizes to ["old", "value"]
    // so that a document declaring `oldValue` is reachable by either half,
    // but a document that matches only the generic `value` half is not a hit
    // for `oldValue` — it is a different symbol that happens to share a
    // subtoken. Admitting it makes this discovery lane report evidence the
    // query never asked for, and (because such a document is usually in a
    // *different*, unchanged file) it survives the freshness boundary that
    // correctly suppressed the symbol actually asked about. So a document
    // qualifies only when it covers every subtoken of at least one whole
    // query word. Single-subtoken words are unaffected, so prose queries keep
    // today's OR behavior.
    const wordGroups = String(query ?? "")
      .split(/\s+/)
      .map((word) => [...new Set(tokenize(word))])
      .filter((group) => group.length > 0);
    const n = this.documents.size;
    const rows = [];
    for (const document of this.documents.values()) {
      if (wordGroups.length && !wordGroups.some((group) => group.every((token) => document.tf.has(token)))) continue;
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
