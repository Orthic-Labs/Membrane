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
    // Admission rule: a document must match the RAREST corpus-known subtoken
    // of at least one whole query word.
    //
    // A code identifier is atomic, but only loosely. `oldValue` tokenizes to
    // ["old", "value"] so either half can find it. Admitting a document that
    // matches only the generic half reports a different symbol that happens to
    // share a subtoken; requiring ALL subtokens instead destroys the dominant
    // code-search shape, where the caller over-specifies (`UserRepository` for
    // `UserRepo`). A fixed coverage fraction is not the answer either — it is a
    // tuning, and it breaks as soon as the two identifiers share one more
    // subtoken (`getOldValue` vs `getStableValue`) or the query carries a path
    // or extension segment.
    //
    // Rarity is the property that actually distinguishes these cases. Within a
    // query word, the subtoken with the lowest document frequency is the one
    // carrying the discriminating information: `old` in `oldValue`, `account`
    // in `UserAccountRepository`. A document that misses it is not an answer to
    // this query however many generic halves it shares.
    //
    // Subtokens the corpus has never seen are dropped before choosing: they
    // discriminate nothing here, which is exactly why an over-specified query
    // (`repository`, `services`, the plural `values`) still finds the shorter
    // stored identifier. A word left with no known subtokens cannot qualify
    // anything.
    const wordGroups = String(query ?? "")
      .split(/\s+/)
      .map((word) => {
        const known = [...new Set(tokenize(word))].filter((token) => (this.df.get(token) ?? 0) > 0);
        if (!known.length) return null;
        const rarest = Math.min(...known.map((token) => this.df.get(token)));
        return known.filter((token) => this.df.get(token) === rarest);
      })
      .filter(Boolean);
    const covers = (document) => wordGroups.some((group) => group.some((token) => document.tf.has(token)));
    const n = this.documents.size;
    const rows = [];
    for (const document of this.documents.values()) {
      if (wordGroups.length && !covers(document)) continue;
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
