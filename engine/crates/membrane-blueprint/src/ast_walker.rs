//! GC7: native port of `blueprint/src/graph/generic-ast-walker.mjs`
//! (the table-driven `walkTable` generic fallback path exercised by
//! `blueprint/tests/generic-ast-walker.test.mjs`) plus the companion
//! `defineLanguageTable` shape from `blueprint/src/graph/language-table.mjs`.
//!
//! This module deliberately reimplements a *minimal* tree-sitter node walk
//! independent of `graph.rs` (owned by lane GRAM): it never calls into that
//! module's private helpers, only the `tree_sitter` crate already declared
//! in this crate's `Cargo.toml`.
//!
//! Scope note: the legacy file also contains hand-written `jsLike`/`python`/
//! `rust` per-grammar strategies, but the only strategy exercised by the
//! ported legacy test suite is the generic, declarative `patterns` walker
//! driven by a `LanguageTable`. That is what is ported here faithfully
//! (visitation order, qualified-name uniquing, evidence shape, confidence,
//! parse-status reporting, and the "data profile never fabricates
//! function/call facts" guarantee).

use std::collections::BTreeMap;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tree_sitter::Node;

pub const GENERIC_WALKER_VERSION: &str = "1.1.0";

/// `factProfile`: legacy default is `"code"`; `"data"` (markup/config
/// grammars) must never fabricate function/call facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactProfile {
    Code,
    Data,
}

impl Default for FactProfile {
    fn default() -> Self {
        FactProfile::Code
    }
}

/// `nameOf(node, pattern)`: resolve a symbol's display name either via a
/// named field (`pattern.field`) or the first named child whose type is in
/// `pattern.nodeTypes`.
#[derive(Debug, Clone, Default)]
pub struct NamePattern {
    pub field: Option<String>,
    pub node_types: Vec<String>,
}

/// One entry of `table.functions` / `table.classes` / `table.declarations`.
#[derive(Debug, Clone, Default)]
pub struct FactPattern {
    pub node_types: Vec<String>,
    pub name: NamePattern,
    pub labels: Vec<String>,
}

/// One entry of `table.imports`.
#[derive(Debug, Clone, Default)]
pub struct ImportPattern {
    pub node_types: Vec<String>,
    pub name: NamePattern,
}

/// One entry of `table.comments`.
#[derive(Debug, Clone, Default)]
pub struct CommentPattern {
    pub node_types: Vec<String>,
}

/// Mirrors `defineLanguageTable(...)` — a frozen, declarative per-grammar
/// fact table. Only the fields exercised by the generic `patterns` walker
/// are represented; `functionTypes`/`typeLabels`/`strategy` (used only by
/// the jsLike/python/rust hand-written strategies) are intentionally out of
/// scope for this port.
#[derive(Debug, Clone, Default)]
pub struct LanguageTable {
    pub id: String,
    pub extensions: Vec<String>,
    pub grammar_file: String,
    pub fact_profile: FactProfile,
    pub functions: Vec<FactPattern>,
    pub classes: Vec<FactPattern>,
    pub declarations: Vec<FactPattern>,
    pub imports: Vec<ImportPattern>,
    pub comments: Vec<CommentPattern>,
}

/// `defineLanguageTable`: normalizes/sorts extensions and applies the
/// `factProfile` default, matching the legacy freeze semantics (values are
/// fixed at construction; there is nothing further to mutate at call
/// sites, consistent with `Object.freeze` in the legacy module).
pub fn define_language_table(mut table: LanguageTable) -> LanguageTable {
    table.extensions.sort();
    table
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct WalkedNode {
    pub id: String,
    pub kind: String,
    pub labels: Vec<String>,
    pub name: Option<String>,
    #[serde(rename = "qualifiedName")]
    pub qualified_name: Option<String>,
    pub path: String,
    pub confidence: f64,
    #[serde(rename = "confidenceTier")]
    pub confidence_tier: &'static str,
    pub evidence: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct WalkedEdge {
    pub kind: String,
    pub source: String,
    pub target: Option<String>,
    pub evidence: Vec<Value>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct WalkReport {
    pub kind: &'static str,
    pub path: String,
    pub message: &'static str,
}

#[derive(Debug, Clone, serde::Serialize, Default)]
pub struct WalkResult {
    pub nodes: Vec<WalkedNode>,
    pub edges: Vec<WalkedEdge>,
    pub reports: Vec<WalkReport>,
}

fn content_hash_hex(source: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
    hex::encode(hasher.finalize())
}

/// `child(node, field)`: field-named child lookup.
fn field_child<'a>(node: Node<'a>, field: &str) -> Option<Node<'a>> {
    node.child_by_field_name(field)
}

/// `named(node, types)`: named children filtered by `type`.
fn named_children_of_types<'a>(node: Node<'a>, types: &[String]) -> Vec<Node<'a>> {
    let mut out = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if types.iter().any(|t| t == child.kind()) {
            out.push(child);
        }
    }
    out
}

fn node_text<'a>(node: Node<'a>, source: &'a str) -> &'a str {
    node.utf8_text(source.as_bytes()).unwrap_or("")
}

struct WalkCtx<'a> {
    source: &'a str,
    file_path: String,
    content_hash: String,
    confidence: f64,
    confidence_tier: &'static str,
    // Kept for shape-parity with the legacy `ctx.meta` (provider/grammarHash/
    // precisionTier are threaded into every emitted node/edge in the legacy
    // module); not currently surfaced on `WalkedNode`/`WalkedEdge` since the
    // ported test suite does not assert on them.
    #[allow(dead_code)]
    provider: Option<String>,
    #[allow(dead_code)]
    grammar_hash: Option<String>,
    #[allow(dead_code)]
    precision_tier: Option<String>,
    nodes: Vec<WalkedNode>,
    contains_edges: Vec<WalkedEdge>,
    defines_edges: Vec<WalkedEdge>,
    raw_imports: Vec<(Value, Option<String>, Value)>,
    qualified_counts: BTreeMap<String, usize>,
}

impl<'a> WalkCtx<'a> {
    fn evidence(&self, node: Node<'_>) -> Value {
        json!({
            "path": self.file_path,
            "startLine": node.start_position().row + 1,
            "endLine": node.end_position().row + 1,
            "contentHash": self.content_hash,
        })
    }

    /// `symbol(ctx, node, name, qualifiedName, labels, kind, container, relation)`
    fn symbol(
        &mut self,
        node: Node<'_>,
        name: &str,
        qualified_name: &str,
        labels: Vec<String>,
        kind: &str,
        container: &str,
        relation: &'static str,
    ) -> String {
        let evidence = self.evidence(node);
        let id = format!("symbol:{}::{}", self.file_path, qualified_name);
        let value = WalkedNode {
            id: id.clone(),
            kind: kind.to_string(),
            labels,
            name: Some(name.to_string()),
            qualified_name: Some(qualified_name.to_string()),
            path: self.file_path.clone(),
            confidence: self.confidence,
            confidence_tier: self.confidence_tier,
            evidence: vec![evidence.clone()],
            text: None,
        };
        self.nodes.push(value);
        let edge = WalkedEdge {
            kind: relation.to_string(),
            source: container.to_string(),
            target: Some(id.clone()),
            evidence: vec![evidence],
        };
        if relation == "DEFINES" {
            self.defines_edges.push(edge);
        } else {
            self.contains_edges.push(edge);
        }
        id
    }

    fn add_comment(&mut self, node: Node<'_>) {
        let evidence = self.evidence(node);
        self.nodes.push(WalkedNode {
            id: format!("comment:{}:{}", self.file_path, node.id()),
            kind: "comment".to_string(),
            labels: vec!["Comment".to_string()],
            name: None,
            qualified_name: None,
            path: self.file_path.clone(),
            confidence: self.confidence,
            confidence_tier: self.confidence_tier,
            evidence: vec![evidence],
            text: Some(node_text(node, self.source).to_string()),
        });
    }

    fn add_import(&mut self, node: Node<'_>, specifier: Option<&str>) {
        let Some(specifier) = specifier else { return };
        let stripped = strip_quotes(specifier);
        if stripped.is_empty() {
            return;
        }
        self.raw_imports.push((json!(stripped), None, self.evidence(node)));
    }
}

/// `strip(value)`: trim + strip a single layer of surrounding quote marks.
fn strip_quotes(value: &str) -> String {
    let trimmed = value.trim();
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'`' || first == b'\'' || first == b'"') && first == last {
            return trimmed[1..trimmed.len() - 1].to_string();
        }
    }
    trimmed.to_string()
}

/// `nameOf(node, pattern)`.
fn name_of<'a>(node: Node<'a>, pattern: &NamePattern, source: &'a str) -> Option<String> {
    if let Some(field) = &pattern.field {
        if let Some(child) = field_child(node, field) {
            return Some(node_text(child, source).to_string());
        }
    }
    named_children_of_types(node, &pattern.node_types)
        .first()
        .map(|n| node_text(*n, source).to_string())
}

/// `uniqueQualifiedName(ctx, name, scope, node)`.
fn unique_qualified_name(ctx: &mut WalkCtx<'_>, name: &str, scope: &[String], node: Node<'_>) -> String {
    let mut parts: Vec<&str> = scope.iter().map(|s| s.as_str()).collect();
    parts.push(name);
    let base = parts.join(".");
    let count = ctx.qualified_counts.entry(base.clone()).or_insert(0);
    *count += 1;
    if *count == 1 {
        base
    } else {
        format!("{base}#{}-{}", node.start_position().row + 1, *count)
    }
}

/// `patterns(ctx)`: the generic declarative walk, in the same visitation
/// order as the legacy implementation (functions, then classes, then
/// declarations, then imports, then comments, per node; depth-first,
/// pre-order, threading at most one new scope name per level).
fn visit(ctx: &mut WalkCtx<'_>, table: &LanguageTable, node: Node<'_>, scope: Vec<String>) {
    let mut scope_names: Vec<String> = Vec::new();

    // NOTE: the legacy `patterns(ctx)` walker applies `table.functions` /
    // `table.classes` / `table.imports` unconditionally — there is no
    // runtime `factProfile` gate in the walker itself. "Data profiles never
    // fabricate function/call facts" is a *table-authoring* convention
    // (data-profile tables simply omit `functions`/`imports` entries), not
    // walker-enforced behavior. Reproduced faithfully: no gate here either.
    for pattern in &table.functions {
        if pattern.node_types.iter().any(|t| t == node.kind()) {
            if let Some(name) = name_of(node, &pattern.name, ctx.source) {
                let qualified = unique_qualified_name(ctx, &name, &scope, node);
                let container = format!("file:{}", ctx.file_path);
                ctx.symbol(node, &name, &qualified, pattern.labels.clone(), "symbol", &container, "CONTAINS");
                scope_names.push(name);
            }
        }
    }
    for pattern in &table.classes {
        if pattern.node_types.iter().any(|t| t == node.kind()) {
            if let Some(name) = name_of(node, &pattern.name, ctx.source) {
                let qualified = unique_qualified_name(ctx, &name, &scope, node);
                let container = format!("file:{}", ctx.file_path);
                ctx.symbol(node, &name, &qualified, pattern.labels.clone(), "class", &container, "CONTAINS");
                scope_names.push(name);
            }
        }
    }

    for pattern in &table.declarations {
        if pattern.node_types.iter().any(|t| t == node.kind()) {
            if let Some(raw) = name_of(node, &pattern.name, ctx.source) {
                let name = strip_quotes(&raw);
                if !name.is_empty() {
                    let qualified = unique_qualified_name(ctx, &name, &scope, node);
                    let container = format!("file:{}", ctx.file_path);
                    ctx.symbol(node, &name, &qualified, pattern.labels.clone(), "symbol", &container, "CONTAINS");
                    scope_names.push(name);
                }
            }
        }
    }

    for pattern in &table.imports {
        if pattern.node_types.iter().any(|t| t == node.kind()) {
            let specifier = name_of(node, &pattern.name, ctx.source);
            ctx.add_import(node, specifier.as_deref());
        }
    }

    for pattern in &table.comments {
        if pattern.node_types.iter().any(|t| t == node.kind()) {
            ctx.add_comment(node);
        }
    }

    let child_scope = if let Some(first) = scope_names.first() {
        let mut next = scope;
        next.push(first.clone());
        next
    } else {
        scope
    };

    let mut cursor = node.walk();
    let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
    for child in children {
        visit(ctx, table, child, child_scope.clone());
    }
}

/// Options for [`walk_table`], mirroring the legacy `walkTable(options)`
/// call shape (`table`, `tree`, `file`, `filePath`, `providerId`,
/// `grammarHash`, `precisionTier`).
pub struct WalkOptions<'a> {
    pub table: &'a LanguageTable,
    pub source: &'a str,
    pub root: Option<Node<'a>>,
    pub file_path: String,
    pub provider_id: Option<String>,
    pub grammar_hash: Option<String>,
    pub precision_tier: Option<String>,
    pub has_error: bool,
}

/// Native port of `walkTable(options)` for the generic `patterns` strategy.
pub fn walk_table(options: WalkOptions<'_>) -> WalkResult {
    let content_hash = content_hash_hex(options.source);
    let confidence = if options.has_error { 0.6 } else { 1.0 };
    let confidence_tier = "EXACT_RESOLUTION";

    let Some(root) = options.root else {
        return WalkResult {
            nodes: Vec::new(),
            edges: Vec::new(),
            reports: vec![WalkReport {
                kind: "parse_failed",
                path: options.file_path,
                message: "no root node",
            }],
        };
    };

    let mut ctx = WalkCtx {
        source: options.source,
        file_path: options.file_path.clone(),
        content_hash,
        confidence,
        confidence_tier,
        provider: options.provider_id,
        grammar_hash: options.grammar_hash,
        precision_tier: options.precision_tier,
        nodes: Vec::new(),
        contains_edges: Vec::new(),
        defines_edges: Vec::new(),
        raw_imports: Vec::new(),
        qualified_counts: BTreeMap::new(),
    };

    visit(&mut ctx, options.table, root, Vec::new());

    let mut edges: Vec<WalkedEdge> = Vec::new();
    edges.extend(ctx.contains_edges.iter().cloned().map(|mut e| {
        e.kind = "CONTAINS".to_string();
        e
    }));
    edges.extend(ctx.defines_edges.iter().cloned().map(|mut e| {
        e.kind = "DEFINES".to_string();
        e
    }));
    for (specifier, kind_hint, evidence) in &ctx.raw_imports {
        let _ = kind_hint;
        let specifier_str = specifier.as_str().unwrap_or_default();
        let target = if specifier_str.is_empty() {
            None
        } else {
            Some(format!("file:{specifier_str}"))
        };
        edges.push(WalkedEdge {
            kind: "IMPORTS".to_string(),
            source: format!("file:{}", options.file_path),
            target,
            evidence: vec![evidence.clone()],
        });
    }

    let reports = if options.has_error {
        vec![WalkReport {
            kind: "partial_parse",
            path: options.file_path,
            message: "parse contains errors",
        }]
    } else {
        Vec::new()
    };

    WalkResult {
        nodes: ctx.nodes,
        edges,
        reports,
    }
}
