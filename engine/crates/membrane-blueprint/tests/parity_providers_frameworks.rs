//! Parity tests for `providers::frameworks`, ported from the legacy
//! `blueprint/src/providers/frameworks/index.mjs` and
//! `blueprint/src/providers/frameworks/http/index.mjs`, exercised by
//! `blueprint/tests/http-framework-providers.test.mjs`. Fixtures and
//! assertions are taken directly from that legacy test file so behavior
//! (including gate semantics and comment-stripping) matches exactly.

use membrane_blueprint::providers::frameworks::{
    domain_gate_active, extract_database_facts, extract_deployment_facts, extract_event_facts,
    extract_routes, framework_gate_active, imported_packages, CROSS_STACK_EDGES,
};

#[test]
fn framework_gates_activate_only_on_imports() {
    assert!(framework_gate_active(&["express".to_string()], "next-express"));
    assert!(!framework_gate_active(&["react".to_string()], "next-express"));
    assert!(framework_gate_active(&["fastapi".to_string()], "fastapi-django"));
    assert!(framework_gate_active(&["tauri".to_string()], "tauri-axum"));
}

#[test]
fn express_route_extraction_with_evidence_positive() {
    let routes = extract_routes("next-express", "app.get('/orders', handler);", "routes.ts");
    assert!(routes
        .iter()
        .any(|r| r.kind == "route" && r.method.as_deref() == Some("GET") && r.path.as_deref() == Some("/orders")));
    assert_eq!(routes[0].confidence, "CROSS_FILE_HEURISTIC");
    assert!(!routes[0].evidence.is_empty());
}

#[test]
fn fastapi_decorator_routes_positive() {
    let routes = extract_routes(
        "fastapi-django",
        "@app.post('/orders')\ndef create_order(): pass",
        "app.py",
    );
    assert!(routes
        .iter()
        .any(|r| r.kind == "route" && r.method.as_deref() == Some("POST") && r.path.as_deref() == Some("/orders")));
    assert!(routes
        .iter()
        .any(|r| r.kind == "handler" && r.name.as_deref() == Some("create_order")));
}

#[test]
fn similar_names_without_framework_import_produce_no_route_negative() {
    let routes = extract_routes(
        "next-express",
        "const orders = []; // app.get('/fake')",
        "x.ts",
    );
    // A comment containing app.get must not match (line regex only matches code).
    assert_eq!(routes.iter().filter(|r| r.kind == "route").count(), 0);
}

#[test]
fn cross_stack_facts_carry_edges_and_confidence() {
    let events = extract_event_facts("publish('orders.created');", "svc.ts");
    assert!(events
        .iter()
        .any(|f| f.edge.as_deref() == Some("PRODUCES") && f.topic.as_deref() == Some("orders.created")));
    let db = extract_database_facts("orders.find({ id }); orders.save(order);", "repo.ts");
    assert!(db.iter().any(|f| f.edge.as_deref() == Some("READS")));
    assert!(db.iter().any(|f| f.edge.as_deref() == Some("WRITES")));
    let deploy = extract_deployment_facts("deploy: aws-ecs-deploy", "ci.yml");
    assert!(deploy.iter().any(|f| f.edge.as_deref() == Some("DEPLOYS")));
}

#[test]
fn cross_stack_edges_vocabulary_is_complete() {
    for edge in ["PRODUCES", "CONSUMES", "GENERATES", "READS", "WRITES", "CONFIGURES", "DEPLOYS"] {
        assert!(CROSS_STACK_EDGES.contains(&edge));
    }
}

#[test]
fn polyglot_fixture_traces_client_route_handler_queue_consumer_db() {
    let ui = "fetch('/api/orders');";
    let api = "app.post('/api/orders', createOrder);";
    let svc = "publish('orders.created', order);";
    let worker = "consume('orders.created', persist);";
    let repo = "orders.save(order);";
    let ui_routes = extract_routes("next-express", api, "api.ts");
    let events = extract_event_facts(svc, "svc.ts");
    let consumed = extract_event_facts(worker, "worker.ts");
    let db = extract_database_facts(repo, "repo.ts");
    assert!(ui.contains("/api/orders"));
    assert!(ui_routes.iter().any(|r| r.path.as_deref() == Some("/api/orders")));
    assert!(events
        .iter()
        .any(|f| f.topic.as_deref() == Some("orders.created") && f.edge.as_deref() == Some("PRODUCES")));
    assert!(consumed
        .iter()
        .any(|f| f.topic.as_deref() == Some("orders.created") && f.edge.as_deref() == Some("CONSUMES")));
    assert!(db.iter().any(|f| f.edge.as_deref() == Some("WRITES")));
}

// Additional coverage (not in the legacy suite, added here since native
// gate helpers `imported_packages`/`domain_gate_active` had no dedicated
// legacy test but back `frameworkGateActive`/`domainGateActive`).

#[test]
fn imported_packages_extracts_and_dedupes_lowercased_specifiers() {
    let imports = imported_packages("import express from 'Express';\nconst x = require(\"Prisma\");\nimport kafka");
    assert!(imports.contains(&"express".to_string()));
    assert!(imports.contains(&"prisma".to_string()));
    assert!(imports.contains(&"kafka".to_string()));
}

#[test]
fn domain_gate_active_matches_exact_and_subpath_imports() {
    assert!(domain_gate_active(&["prisma".to_string()], "database"));
    assert!(domain_gate_active(&["@prisma/client".to_string()], "database"));
    assert!(!domain_gate_active(&["mongoose".to_string()], "database"));
    assert!(domain_gate_active(&["kafkajs".to_string()], "event"));
}
