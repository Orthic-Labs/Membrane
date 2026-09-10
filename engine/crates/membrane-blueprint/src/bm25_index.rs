//! Native port of `blueprint/src/graph/bm25-code-index.mjs`.
//!
//! Tokenization is aligned with Ledger's `add_component_aliases`
//! (`engine/crates/membrane-runtime/src/ledger/index.rs`) so the two lexical
//! engines agree on what a word is. Three boundaries, matching Ledger exactly:
//!
//!   lower/digit -> upper      `httpServer`     -> http server
//!   upper-run   -> Upper+lower `HTTPServer`    -> http server
//!                               `XMLHttpRequest` -> xml http request
//!   any non-alphanumeric      `user_account`, `src/a.b#c`
//!
//! Unicode: Ledger's FTS5 uses `unicode61 remove_diacritics 2` and normalizes
//! queries NFKC + casefold. The tokenizer here normalizes NFKC and splits on
//! `\p{L}`/`\p{N}` (via the `regex` crate's Unicode character classes),
//! matching the legacy JS behaviour exactly rather than an ASCII-only split.
//!
//! No minimum token length: a 1-character identifier (`x`, `i`, `n`, a CJK
//! ideograph) is real code and must survive both indexing and querying. The
//! floor is retained only inside search admission, where it guards
//! containment (a 1-character word is a substring of almost everything), not
//! the tokenizer.

use std::collections::HashMap;

use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;
use unicode_normalization::UnicodeNormalization;

fn boundary_lower_or_digit_to_upper() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(\p{Ll}|\p{N})(\p{Lu})").expect("valid regex"))
}

fn boundary_acronym_run_to_word() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(\p{Lu}+)(\p{Lu}\p{Ll})").expect("valid regex"))
}

fn split_non_alphanumeric() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[^\p{L}\p{N}]+").expect("valid regex"))
}

fn flat_non_alphanumeric() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[^\p{L}\p{N}]+").expect("valid regex"))
}

/// Splits identifiers into lowercase word tokens on camelCase, acronym,
/// snake_case, path-separator, and other non-alphanumeric boundaries.
pub fn tokenize(value: &str) -> Vec<String> {
    let normalized: String = value.nfkc().collect();
    let step1 = boundary_lower_or_digit_to_upper()
        .replace_all(&normalized, "$1 $2")
        .into_owned();
    let step2 = boundary_acronym_run_to_word()
        .replace_all(&step1, "$1 $2")
        .into_owned();
    let lowered = step2.to_lowercase();
    split_non_alphanumeric()
        .split(&lowered)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

fn term_frequency(tokens: &[String]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for token in tokens {
        *counts.entry(token.clone()).or_insert(0) += 1;
    }
    counts
}

/// Input document shape, mirroring the legacy JS object passed to
/// `Bm25CodeIndex#replace` / `#replaceDocument`.
#[derive(Debug, Clone, Default)]
pub struct CodeDocument {
    pub id: String,
    pub name: String,
    pub qualified_name: String,
    pub path: String,
    pub signature: String,
    pub identifiers: Vec<String>,
    /// Opaque payload carried through to search hits, analogous to the JS
    /// `node` field (kept generic since callers attach graph-specific data).
    pub node: Value,
}

#[derive(Debug, Clone)]
struct NormalizedDocument {
    document: CodeDocument,
    tokens: Vec<String>,
    tf: HashMap<String, usize>,
    length: usize,
}

#[derive(Debug, Clone)]
pub struct TermContribution {
    pub term: String,
    pub score: f64,
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub id: String,
    pub score: f64,
    pub exact_name: bool,
    pub contributions: Vec<TermContribution>,
    pub document: CodeDocument,
}

#[derive(Debug, Clone, Copy)]
pub struct SearchOptions {
    pub limit: i64,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self { limit: 20 }
    }
}

/// Native port of the legacy `Bm25CodeIndex` class: BM25 ranking over a
/// symbol corpus with incremental df/avgdl maintenance and a name-containment
/// admission gate ahead of scoring.
pub struct Bm25CodeIndex {
    k1: f64,
    b: f64,
    documents: HashMap<String, NormalizedDocument>,
    df: HashMap<String, usize>,
    avgdl: f64,
    total_length: usize,
}

impl Default for Bm25CodeIndex {
    fn default() -> Self {
        Self::new(1.2, 0.75)
    }
}

impl Bm25CodeIndex {
    pub fn new(k1: f64, b: f64) -> Self {
        Self {
            k1,
            b,
            documents: HashMap::new(),
            df: HashMap::new(),
            avgdl: 0.0,
            total_length: 0,
        }
    }

    pub fn avgdl(&self) -> f64 {
        self.avgdl
    }

    pub fn df_len(&self) -> usize {
        self.df.len()
    }

    pub fn df_entries(&self) -> Vec<(String, usize)> {
        let mut entries: Vec<(String, usize)> = self.df.iter().map(|(k, v)| (k.clone(), *v)).collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries
    }

    pub fn document_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.documents.keys().cloned().collect();
        ids.sort();
        ids
    }

    pub fn len(&self) -> usize {
        self.documents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    /// Full-corpus rebuild, equivalent to `replace()`.
    pub fn replace(&mut self, documents: Vec<CodeDocument>) -> &mut Self {
        self.documents.clear();
        self.df.clear();
        self.total_length = 0;
        for document in documents {
            let normalized = Self::normalize(document);
            self.insert(normalized);
        }
        self.refresh_avgdl();
        self
    }

    /// Incremental df/avgdl-preserving upsert, equivalent to
    /// `replaceDocument()`.
    pub fn replace_document(&mut self, document: CodeDocument) -> &mut Self {
        let id = document.id.clone();
        if let Some(previous) = self.documents.remove(&id) {
            self.retract(&previous);
        }
        let normalized = Self::normalize(document);
        self.insert(normalized);
        self.refresh_avgdl();
        self
    }

    pub fn remove_document(&mut self, id: &str) -> &mut Self {
        if let Some(previous) = self.documents.remove(id) {
            self.retract(&previous);
            self.refresh_avgdl();
        }
        self
    }

    fn insert(&mut self, document: NormalizedDocument) {
        self.total_length += document.length;
        let unique_tokens: std::collections::HashSet<&String> = document.tokens.iter().collect();
        for token in unique_tokens {
            *self.df.entry(token.clone()).or_insert(0) += 1;
        }
        self.documents.insert(document.document.id.clone(), document);
    }

    fn retract(&mut self, document: &NormalizedDocument) {
        self.total_length -= document.length;
        let unique_tokens: std::collections::HashSet<&String> = document.tokens.iter().collect();
        for token in unique_tokens {
            if let Some(next) = self.df.get_mut(token) {
                if *next > 1 {
                    *next -= 1;
                } else {
                    self.df.remove(token);
                }
            }
        }
    }

    fn refresh_avgdl(&mut self) {
        self.avgdl = if self.documents.is_empty() {
            0.0
        } else {
            self.total_length as f64 / self.documents.len() as f64
        };
    }

    fn normalize(document: CodeDocument) -> NormalizedDocument {
        let mut weighted_parts: Vec<&str> = Vec::new();
        if !document.name.is_empty() {
            weighted_parts.push(&document.name);
            weighted_parts.push(&document.name);
        }
        if !document.qualified_name.is_empty() {
            weighted_parts.push(&document.qualified_name);
            weighted_parts.push(&document.qualified_name);
        }
        if !document.path.is_empty() {
            weighted_parts.push(&document.path);
        }
        for identifier in &document.identifiers {
            if !identifier.is_empty() {
                weighted_parts.push(identifier);
            }
        }
        if !document.signature.is_empty() {
            weighted_parts.push(&document.signature);
        }
        let weighted = weighted_parts.join(" ");
        let tokens = tokenize(&weighted);
        let tf = term_frequency(&tokens);
        let length = tokens.len().max(1);
        NormalizedDocument { document, tokens, tf, length }
    }

    fn flat(value: &str) -> String {
        let normalized: String = value.nfkc().collect();
        let lowered = normalized.to_lowercase();
        flat_non_alphanumeric().replace_all(&lowered, "").into_owned()
    }

    /// BM25 ranking with name-containment admission, equivalent to
    /// `search()`. See the legacy module for the full rationale behind the
    /// admission rule -- it must read only the query and the one candidate
    /// document being judged, never corpus-wide statistics.
    pub fn search(&self, query: &str, options: SearchOptions) -> Vec<SearchHit> {
        let mut seen = std::collections::HashSet::new();
        let terms: Vec<String> = tokenize(query)
            .into_iter()
            .filter(|t| seen.insert(t.clone()))
            .collect();
        if terms.is_empty() || self.documents.is_empty() {
            return Vec::new();
        }

        let query_words: Vec<String> = query
            .split_whitespace()
            .map(Self::flat)
            .filter(|s| !s.is_empty())
            .collect();

        let covers = |doc: &NormalizedDocument| -> bool {
            let names = [
                doc.document.name.as_str(),
                doc.document.qualified_name.as_str(),
                doc.document.path.as_str(),
                doc.document.signature.as_str(),
            ];
            query_words.iter().any(|word| {
                if word.chars().count() < 2 {
                    return doc.tf.contains_key(word);
                }
                names.iter().any(|value| {
                    let candidate = Self::flat(value);
                    candidate.chars().count() >= 2
                        && (candidate.contains(word.as_str()) || word.contains(candidate.as_str()))
                })
            })
        };

        let n = self.documents.len() as f64;
        let query_trimmed_lower = query.trim().to_lowercase();
        let mut rows: Vec<SearchHit> = Vec::new();

        for document in self.documents.values() {
            if !query_words.is_empty() && !covers(document) {
                continue;
            }
            let mut score = 0.0f64;
            let mut contributions = Vec::new();
            for term in &terms {
                let tf = *document.tf.get(term).unwrap_or(&0);
                if tf == 0 {
                    continue;
                }
                let df = *self.df.get(term).unwrap_or(&0) as f64;
                let idf = (1.0 + ((n - df + 0.5) / (df + 0.5))).ln();
                let denominator = tf as f64
                    + self.k1 * (1.0 - self.b + self.b * (document.length as f64 / (if self.avgdl != 0.0 { self.avgdl } else { 1.0 })));
                let term_score = idf * ((tf as f64 * (self.k1 + 1.0)) / denominator);
                score += term_score;
                contributions.push(TermContribution { term: term.clone(), score: term_score });
            }
            if score <= 0.0 {
                continue;
            }
            let exact_name = document.document.name.to_lowercase() == query_trimmed_lower;
            rows.push(SearchHit {
                id: document.document.id.clone(),
                score: score + if exact_name { 1000.0 } else { 0.0 },
                exact_name,
                contributions,
                document: document.document.clone(),
            });
        }

        rows.sort_by(|a, b| {
            b.exact_name
                .cmp(&a.exact_name)
                .then_with(|| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal))
                .then_with(|| a.id.cmp(&b.id))
        });

        let limit = options.limit.max(1).min(200) as usize;
        rows.truncate(limit);
        rows
    }
}

/// Minimal generation-node view accepted by [`build_bm25_code_index`],
/// analogous to the fields the legacy JS reads off a Blueprint graph node.
#[derive(Debug, Clone, Default)]
pub struct GenerationNodeView {
    pub id: String,
    pub kind: String,
    pub labels: Vec<String>,
    pub name: Option<String>,
    pub qualified_name: Option<String>,
    pub path: Option<String>,
    pub signature: Option<String>,
    pub raw_declared_type: Option<String>,
    pub node: Value,
}

/// Builds an index from generation nodes, admitting only symbol-like nodes --
/// equivalent to the legacy `buildBm25CodeIndex`.
pub fn build_bm25_code_index(nodes: &[GenerationNodeView]) -> Bm25CodeIndex {
    const SYMBOL_LABELS: [&str; 7] = ["Function", "Method", "Class", "Interface", "Trait", "Test", "Screen"];
    let documents: Vec<CodeDocument> = nodes
        .iter()
        .filter(|node| {
            node.kind == "symbol"
                || node.kind == "class"
                || node.labels.iter().any(|label| SYMBOL_LABELS.contains(&label.as_str()))
        })
        .map(|node| CodeDocument {
            id: node.id.clone(),
            name: node.name.clone().unwrap_or_default(),
            qualified_name: node.qualified_name.clone().unwrap_or_default(),
            path: node.path.clone().unwrap_or_default(),
            signature: node
                .signature
                .clone()
                .or_else(|| node.raw_declared_type.clone())
                .unwrap_or_default(),
            identifiers: node.labels.clone(),
            node: node.node.clone(),
        })
        .collect();
    let mut index = Bm25CodeIndex::default();
    index.replace(documents);
    index
}
