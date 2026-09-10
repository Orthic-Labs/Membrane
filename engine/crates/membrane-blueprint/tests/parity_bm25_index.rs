//! Parity port of `blueprint/tests/bm25-code-index.test.mjs` against the
//! native `bm25_index` module. Same corpus, same queries, same expected
//! result sets/orderings as the legacy JS suite.

use membrane_blueprint::bm25_index::{tokenize, Bm25CodeIndex, CodeDocument, SearchOptions};

fn make_document(name: &str) -> CodeDocument {
    CodeDocument {
        id: format!("symbol:{name}"),
        name: name.to_string(),
        qualified_name: name.to_string(),
        path: format!("src/{name}.js"),
        signature: String::new(),
        identifiers: Vec::new(),
        node: serde_json::json!({ "id": format!("symbol:{name}"), "name": name }),
    }
}

fn index(names: &[&str]) -> Bm25CodeIndex {
    let mut idx = Bm25CodeIndex::default();
    idx.replace(names.iter().map(|n| make_document(n)).collect());
    idx
}

fn corpus() -> Vec<&'static str> {
    vec![
        "UserAccountRepo",
        "UserRepo",
        "OrderService",
        "getUserByIdentifier",
        "placeOrder",
        "oldValue",
        "stableValue",
        "getOldValue",
        "getStableValue",
    ]
}

fn hits(query: &str) -> Vec<String> {
    index(&corpus())
        .search(query, SearchOptions { limit: 10 })
        .into_iter()
        .map(|row| row.document.name)
        .collect()
}

#[test]
fn over_specified_query_still_finds_stored_identifier() {
    assert_eq!(hits("UserAccountRepository"), vec!["UserAccountRepo"]);
    assert_eq!(hits("getUserById"), vec!["getUserByIdentifier"]);
    assert_eq!(hits("OrderServiceImpl"), vec!["OrderService"]);
    assert_eq!(hits("placeOrderCommand"), vec!["placeOrder"]);
}

#[test]
fn document_not_named_by_query_is_never_a_hit() {
    for query in ["oldValue", "getOldValue", "src/oldValue", "oldValue.js"] {
        let found = hits(query);
        assert!(found.contains(&"oldValue".to_string()), "{query} must still find oldValue");
        assert!(!found.contains(&"stableValue".to_string()), "{query} must not report stableValue");
        assert!(!found.contains(&"getStableValue".to_string()), "{query} must not report getStableValue");
    }
}

#[test]
fn over_specified_query_finds_shorter_identifier_at_two_subtokens() {
    assert!(hits("UserRepository").contains(&"UserRepo".to_string()));
    assert!(hits("OrderServices").contains(&"OrderService".to_string()));
    assert!(hits("oldValues").contains(&"oldValue".to_string()));
}

#[test]
fn query_naming_nothing_returns_nothing() {
    assert!(hits("ZzzQqqWidget").is_empty());
    for query in ["frobnicatedValue", "brandNewValue", "obsoleteValue"] {
        assert!(hits(query).is_empty(), "{query} names nothing in the corpus");
    }
}

#[test]
fn admission_does_not_depend_on_what_else_is_indexed() {
    let of = |names: &[&str], query: &str| {
        index(names)
            .search(query, SearchOptions { limit: 20 })
            .into_iter()
            .map(|row| row.document.name)
            .collect::<Vec<_>>()
    };
    let base = corpus();
    let baseline = of(&base, "oldValue");
    assert!(baseline.contains(&"oldValue".to_string()) && !baseline.contains(&"stableValue".to_string()));
    let extras: Vec<Vec<&str>> = vec![
        vec!["oldHandler", "oldParser"],
        vec!["oldHandler", "oldParser", "oldCache"],
        vec!["valueOne", "valueTwo", "valueThree"],
    ];
    for extra in extras {
        let mut grown = base.clone();
        grown.extend(extra.iter());
        let result = of(&grown, "oldValue");
        assert!(result.contains(&"oldValue".to_string()), "oldValue must still match after indexing {extra:?}");
        assert!(!result.contains(&"stableValue".to_string()), "stableValue must stay excluded after indexing {extra:?}");
        assert!(!result.contains(&"getStableValue".to_string()), "getStableValue must stay excluded after indexing {extra:?}");
    }
}

#[test]
fn shared_path_segment_alone_does_not_qualify() {
    assert!(!hits("src/oldValue").contains(&"placeOrder".to_string()));
    assert_eq!(hits("src").len(), corpus().len());
}

#[test]
fn prose_and_single_subtoken_queries_keep_plain_or() {
    let mut order_hits = hits("order");
    order_hits.sort();
    assert_eq!(order_hits, vec!["OrderService".to_string(), "placeOrder".to_string()]);

    let mut compound_hits = hits("user account repo");
    compound_hits.sort();
    assert_eq!(
        compound_hits,
        vec!["UserAccountRepo".to_string(), "UserRepo".to_string(), "getUserByIdentifier".to_string()]
    );
}

#[test]
fn exact_name_matches_outrank_partial_ones() {
    let mut names = corpus();
    names.push("order");
    let rows = index(&names).search("order", SearchOptions { limit: 10 });
    assert_eq!(rows[0].document.name, "order");
    assert!(rows[0].exact_name);
}

#[test]
fn tokenizer_splits_camel_snake_and_paths() {
    assert_eq!(tokenize("getUserById"), vec!["get", "user", "by", "id"]);
    assert_eq!(tokenize("user_account_repo"), vec!["user", "account", "repo"]);
    assert_eq!(tokenize("src/a.b#c"), vec!["src", "a", "b", "c"]);
    assert_eq!(tokenize("kebab-case-name"), vec!["kebab", "case", "name"]);
    assert_eq!(tokenize("pkg.mod:Class#method"), vec!["pkg", "mod", "class", "method"]);
    assert_eq!(tokenize("parseUtf8Bytes"), vec!["parse", "utf8", "bytes"]);
}

#[test]
fn tokenizer_splits_acronym_runs_from_following_word() {
    assert_eq!(tokenize("HTTPServer"), vec!["http", "server"]);
    assert_eq!(tokenize("XMLHttpRequest"), vec!["xml", "http", "request"]);
    assert_eq!(tokenize("parseJSONResponse"), vec!["parse", "json", "response"]);
    assert_eq!(tokenize("IOError"), vec!["io", "error"]);
    assert_eq!(tokenize("HTTP"), vec!["http"]);
    assert_eq!(tokenize("httpServer"), vec!["http", "server"]);
}

#[test]
fn acronym_prefixed_symbol_reachable_by_hidden_word() {
    let idx = index(&["HTTPServer", "XMLHttpRequest", "placeOrder"]);
    assert!(idx
        .search("server", SearchOptions { limit: 5 })
        .into_iter()
        .map(|r| r.document.name)
        .any(|n| n == "HTTPServer"));
    assert!(idx
        .search("request", SearchOptions { limit: 5 })
        .into_iter()
        .map(|r| r.document.name)
        .any(|n| n == "XMLHttpRequest"));
}

#[test]
fn non_ascii_identifiers_survive_tokenization() {
    assert_eq!(tokenize("cafeteríaService"), vec!["cafetería", "service"]);
    assert_eq!(tokenize("Ünicode_Wert"), vec!["ünicode", "wert"]);
    assert_eq!(tokenize("données"), vec!["données"]);
    assert_eq!(tokenize("読み込みHandler"), vec!["読み込みhandler"]);
    assert_eq!(tokenize("読み込み_Handler"), vec!["読み込み", "handler"]);

    let idx = index(&["cafeteríaService", "placeOrder"]);
    assert_eq!(
        idx.search("cafetería", SearchOptions { limit: 5 })
            .into_iter()
            .map(|r| r.document.name)
            .collect::<Vec<_>>(),
        vec!["cafeteríaService".to_string()]
    );
}

#[test]
fn single_character_identifiers_are_indexed_and_findable() {
    assert_eq!(tokenize("x"), vec!["x"]);
    assert_eq!(tokenize("point_x_y"), vec!["point", "x", "y"]);

    let idx = index(&["x", "placeOrder", "maxValue"]);
    assert_eq!(
        idx.search("x", SearchOptions { limit: 5 }).into_iter().map(|r| r.document.name).collect::<Vec<_>>(),
        vec!["x".to_string()]
    );
    assert!(!idx
        .search("x", SearchOptions { limit: 5 })
        .into_iter()
        .map(|r| r.document.name)
        .any(|n| n == "maxValue"));
    assert!(idx.search("a", SearchOptions { limit: 5 }).is_empty());

    let mixed = index(&["maxValue", "oldValue", "x"]);
    let names: Vec<String> = mixed
        .search("oldValue x", SearchOptions { limit: 5 })
        .into_iter()
        .map(|r| r.document.name)
        .collect();
    assert!(names.contains(&"oldValue".to_string()));
    assert!(names.contains(&"x".to_string()), "the real 1-character symbol is named by the query");
    assert!(!names.contains(&"maxValue".to_string()), "a letter appearing inside an identifier does not name it");
}

#[test]
fn one_character_admission_does_not_depend_on_what_else_is_indexed() {
    let of = |names: &[&str], query: &str| {
        index(names)
            .search(query, SearchOptions { limit: 20 })
            .into_iter()
            .map(|row| row.document.name)
            .collect::<Vec<_>>()
    };
    let base = vec!["maxValue", "oldValue", "x"];
    let baseline = of(&base, "oldValue x");
    let extras: Vec<Vec<&str>> = vec![vec!["xRay", "xAxis"], vec!["indexValue", "boxValue", "fixValue"]];
    for extra in extras {
        let mut grown = base.clone();
        grown.extend(extra.iter());
        let result: Vec<String> = of(&grown, "oldValue x")
            .into_iter()
            .filter(|n| base.contains(&n.as_str()))
            .collect();
        assert_eq!(result, baseline);
    }
}

fn extra_document(name: &str, extra: &str) -> CodeDocument {
    CodeDocument {
        id: format!("symbol:{name}"),
        name: name.to_string(),
        qualified_name: format!("mod.{name}"),
        path: format!("src/{name}.js"),
        signature: extra.to_string(),
        identifiers: if extra.is_empty() { Vec::new() } else { vec![extra.to_string()] },
        node: serde_json::json!({ "id": format!("symbol:{name}"), "name": name }),
    }
}

#[test]
fn incremental_mutation_is_equivalent_to_cold_rebuild() {
    let mut warm = Bm25CodeIndex::default();
    warm.replace(vec![extra_document("alpha", ""), extra_document("beta", ""), extra_document("gamma", "")]);
    warm.replace_document(extra_document("delta", ""));
    warm.replace_document(extra_document("beta", "HTTPServer handler"));
    warm.remove_document("symbol:alpha");
    warm.replace_document(extra_document("epsilon", "XMLHttpRequest"));
    warm.remove_document("symbol:gamma");
    warm.replace_document(extra_document("delta", "renamedPayload"));
    warm.remove_document("symbol:does-not-exist");

    let mut cold = Bm25CodeIndex::default();
    cold.replace(vec![
        extra_document("beta", "HTTPServer handler"),
        extra_document("delta", "renamedPayload"),
        extra_document("epsilon", "XMLHttpRequest"),
    ]);

    assert_eq!(warm.document_ids(), cold.document_ids());
    assert_eq!(warm.df_entries(), cold.df_entries(), "incremental df must equal a rebuilt df, including having no zero-count entries");
    assert_eq!(warm.avgdl(), cold.avgdl());

    for query in ["beta", "server", "request", "renamedPayload", "delta", "handler", "src"] {
        let strip = |rows: Vec<membrane_blueprint::bm25_index::SearchHit>| {
            rows.into_iter()
                .map(|r| (r.id, r.score.to_bits(), r.exact_name, r.contributions.into_iter().map(|c| (c.term, c.score.to_bits())).collect::<Vec<_>>()))
                .collect::<Vec<_>>()
        };
        let warm_rows = strip(warm.search(query, SearchOptions { limit: 20 }));
        let cold_rows = strip(cold.search(query, SearchOptions { limit: 20 }));
        assert_eq!(warm_rows, cold_rows, "{query}");
    }
}

#[test]
fn removing_every_document_leaves_index_equivalent_to_empty() {
    let mut warm = Bm25CodeIndex::default();
    warm.replace(vec![make_document("alpha"), make_document("beta")]);
    warm.remove_document("symbol:alpha");
    warm.remove_document("symbol:beta");

    assert!(warm.df_entries().is_empty(), "df must be empty, not full of zero counts");

    let mut cold = Bm25CodeIndex::default();
    cold.replace(vec![]);
    assert_eq!(warm.avgdl(), cold.avgdl());
    assert!(warm.search("alpha", SearchOptions { limit: 5 }).is_empty());
}

#[test]
fn index_built_only_from_symbol_like_generation_nodes() {
    use membrane_blueprint::bm25_index::{build_bm25_code_index, GenerationNodeView};

    let nodes = vec![
        GenerationNodeView {
            id: "f".to_string(),
            kind: "file".to_string(),
            name: Some("manifestoDocument".to_string()),
            path: Some("docs/manifestoDocument.md".to_string()),
            ..Default::default()
        },
        GenerationNodeView {
            id: "s".to_string(),
            kind: "symbol".to_string(),
            name: Some("handler".to_string()),
            path: Some("src/a.js".to_string()),
            ..Default::default()
        },
    ];
    let built = build_bm25_code_index(&nodes);
    assert_eq!(
        built.search("handler", SearchOptions { limit: 5 }).into_iter().map(|r| r.id).collect::<Vec<_>>(),
        vec!["s".to_string()]
    );
    assert!(built.search("manifestoDocument", SearchOptions { limit: 5 }).is_empty());
}

#[test]
fn empty_or_operator_only_query_returns_nothing() {
    assert!(hits("").is_empty());
    assert!(hits("   ").is_empty());
}
