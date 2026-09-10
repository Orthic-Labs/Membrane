//! GC7 parity tests: native port of the generic `patterns` strategy in
//! `blueprint/src/graph/generic-ast-walker.mjs`, ported from
//! `blueprint/tests/generic-ast-walker.test.mjs`. The legacy suite drives
//! `walkTable` with hand-built mock tree-sitter nodes; here real
//! tree-sitter-javascript parses are used instead (per lane instructions to
//! use real fixtures), asserting the same node/edge shapes, visitation
//! order, qualified-name uniquing, evidence, and parse-status semantics.

use membrane_blueprint::ast_walker::{
    define_language_table, walk_table, FactPattern, FactProfile, ImportPattern, LanguageTable, NamePattern,
    WalkOptions,
};

fn parse(source: &str) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .expect("load javascript grammar");
    parser.parse(source, None).expect("parse javascript source")
}

fn name_field(field: &str) -> NamePattern {
    NamePattern {
        field: Some(field.to_string()),
        node_types: Vec::new(),
    }
}

#[test]
fn define_language_table_sorts_extensions_and_defaults_profile() {
    let table = define_language_table(LanguageTable {
        id: "test".into(),
        extensions: vec!["b".into(), "a".into()],
        grammar_file: "t.wasm".into(),
        ..Default::default()
    });
    assert_eq!(table.id, "test");
    assert_eq!(table.extensions, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(table.fact_profile, FactProfile::Code);
}

#[test]
fn walker_emits_function_nodes_with_evidence_and_confidence() {
    let source = "\n\nfunction foo() {}\n";
    let tree = parse(source);
    let table = LanguageTable {
        functions: vec![FactPattern {
            node_types: vec!["function_declaration".into()],
            name: name_field("name"),
            labels: vec!["Function".into()],
        }],
        ..Default::default()
    };
    let result = walk_table(WalkOptions {
        table: &table,
        source,
        root: Some(tree.root_node()),
        file_path: "src/a.t".into(),
        provider_id: Some("blueprint-treesitter".into()),
        grammar_hash: None,
        precision_tier: Some("AST".into()),
        has_error: tree.root_node().has_error(),
    });
    assert_eq!(result.nodes.len(), 1);
    assert_eq!(result.nodes[0].name.as_deref(), Some("foo"));
    assert_eq!(result.nodes[0].evidence[0]["startLine"], 3);
    assert_eq!(result.nodes[0].confidence_tier, "EXACT_RESOLUTION");
    assert!(result.nodes[0].evidence[0]["contentHash"].as_str().unwrap().len() > 0);
    assert_eq!(result.edges[0].kind, "CONTAINS");
}

#[test]
fn walker_emits_imports_with_unresolved_target_derived_from_specifier() {
    let source = "import x from 'y';\n";
    let tree = parse(source);
    let table = LanguageTable {
        imports: vec![ImportPattern {
            node_types: vec!["import_statement".into()],
            name: name_field("source"),
        }],
        ..Default::default()
    };
    let result = walk_table(WalkOptions {
        table: &table,
        source,
        root: Some(tree.root_node()),
        file_path: "src/a.t".into(),
        provider_id: None,
        grammar_hash: None,
        precision_tier: None,
        has_error: tree.root_node().has_error(),
    });
    assert_eq!(result.edges[0].kind, "IMPORTS");
    assert_eq!(result.edges[0].target.as_deref(), Some("file:y"));
}

#[test]
fn walker_reports_partial_parse_for_error_trees() {
    let source = "function ( {\n";
    let tree = parse(source);
    let table = LanguageTable {
        functions: Vec::new(),
        ..Default::default()
    };
    let has_error = tree.root_node().has_error();
    assert!(has_error, "fixture must actually contain a parse error");
    let result = walk_table(WalkOptions {
        table: &table,
        source,
        root: Some(tree.root_node()),
        file_path: "a.t".into(),
        provider_id: None,
        grammar_hash: None,
        precision_tier: None,
        has_error,
    });
    assert!(result.reports.iter().any(|r| r.kind == "partial_parse"));
}

#[test]
fn walker_reports_failed_parse_when_no_root_node() {
    let table = LanguageTable::default();
    let result = walk_table(WalkOptions {
        table: &table,
        source: "",
        root: None,
        file_path: "a.t".into(),
        provider_id: None,
        grammar_hash: None,
        precision_tier: None,
        has_error: false,
    });
    assert!(result.reports.iter().any(|r| r.kind == "parse_failed"));
}

#[test]
fn data_profile_table_with_no_function_or_import_patterns_never_fabricates_them() {
    let source = "({ a: 1 });\n";
    let tree = parse(source);
    let table = LanguageTable {
        fact_profile: FactProfile::Data,
        ..Default::default()
    };
    let result = walk_table(WalkOptions {
        table: &table,
        source,
        root: Some(tree.root_node()),
        file_path: "a.json".into(),
        provider_id: None,
        grammar_hash: None,
        precision_tier: None,
        has_error: tree.root_node().has_error(),
    });
    assert_eq!(result.nodes.iter().filter(|n| n.labels.contains(&"Function".to_string())).count(), 0);
    assert_eq!(result.edges.iter().filter(|e| e.kind == "CALLS").count(), 0);
}

#[test]
fn table_declarations_emit_bounded_data_facts_with_structural_evidence() {
    let source = "\n\n({ target: 1 });\n";
    let tree = parse(source);
    let table = LanguageTable {
        fact_profile: FactProfile::Data,
        declarations: vec![FactPattern {
            node_types: vec!["pair".into()],
            name: name_field("key"),
            labels: vec!["Key".into()],
        }],
        ..Default::default()
    };
    let result = walk_table(WalkOptions {
        table: &table,
        source,
        root: Some(tree.root_node()),
        file_path: "config.json".into(),
        provider_id: None,
        grammar_hash: None,
        precision_tier: None,
        has_error: tree.root_node().has_error(),
    });
    let declaration = result.nodes.iter().find(|n| n.name.as_deref() == Some("target"));
    assert!(declaration.is_some());
    assert_eq!(declaration.unwrap().labels, vec!["Key".to_string()]);
    assert_eq!(declaration.unwrap().evidence[0]["startLine"], 3);
    assert_eq!(result.edges[0].kind, "CONTAINS");
}

#[test]
fn nested_same_name_generic_functions_retain_distinct_qualified_ids() {
    // outer(){ inner(){} }  then a sibling top-level `same` again.
    let source = "\n\nfunction same() {\n  function same() {}\n}\nfunction same() {}\n";
    let tree = parse(source);
    let table = LanguageTable {
        functions: vec![FactPattern {
            node_types: vec!["function_declaration".into()],
            name: name_field("name"),
            labels: vec!["Function".into()],
        }],
        ..Default::default()
    };
    let result = walk_table(WalkOptions {
        table: &table,
        source,
        root: Some(tree.root_node()),
        file_path: "nested.test".into(),
        provider_id: None,
        grammar_hash: None,
        precision_tier: None,
        has_error: tree.root_node().has_error(),
    });
    let qualified: Vec<Option<String>> = result.nodes.iter().map(|n| n.qualified_name.clone()).collect();
    assert_eq!(
        qualified,
        vec![
            Some("same".to_string()),
            Some("same.same".to_string()),
            Some("same#6-2".to_string()),
        ]
    );
}

#[test]
fn walker_emits_comment_nodes_bound_to_file() {
    let source = "\n// why\n";
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .expect("load javascript grammar");
    // Comments are extra/hidden nodes; tree-sitter still exposes them as
    // named children of the program node in tree-sitter-javascript.
    let tree = parser.parse(source, None).expect("parse");
    let table = LanguageTable {
        comments: vec![membrane_blueprint::ast_walker::CommentPattern {
            node_types: vec!["comment".into()],
        }],
        ..Default::default()
    };
    let result = walk_table(WalkOptions {
        table: &table,
        source,
        root: Some(tree.root_node()),
        file_path: "a.t".into(),
        provider_id: None,
        grammar_hash: None,
        precision_tier: None,
        has_error: tree.root_node().has_error(),
    });
    let comment_node = result.nodes.iter().find(|n| n.kind == "comment");
    assert!(comment_node.is_some());
    assert_eq!(comment_node.unwrap().text.as_deref(), Some("// why"));
}
