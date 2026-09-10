// Parity test for src/framework_intelligence.rs against the legacy
// blueprint/src/graph/framework-intelligence.mjs, using the real fixture
// from blueprint/tests/framework-process-contracts.test.mjs
// ("framework intelligence emits distinct DI ORM config RPC and UI
// evidence").

use membrane_blueprint::framework_intelligence::{augment_framework_intelligence, augment_graph_generation, FiFile, FiGeneration, FiNode};
use membrane_blueprint::graph::{FileRecord, GraphGeneration};

fn symbol(id: &str, path: &str, name: &str) -> FiNode {
    FiNode {
        id: id.to_string(),
        kind: "symbol".to_string(),
        labels: vec!["Function".to_string()],
        name: name.to_string(),
        qualified_name: name.to_string(),
        path: Some(path.to_string()),
        evidence: vec![],
        entry_point: None,
        contract_kind: None,
        contract_roles: vec![],
    }
}

fn base_generation() -> FiGeneration {
    FiGeneration {
        nodes: vec![
            symbol("symbol:src/api.ts::handlePing", "src/api.ts", "handlePing"),
            symbol("symbol:src/api.ts::dependency", "src/api.ts", "dependency"),
            symbol("symbol:src/screen.tsx::Home", "src/screen.tsx", "Home"),
            symbol("symbol:src/api.ts::main", "src/api.ts", "main"),
        ],
        edges: vec![],
    }
}

fn fixture_files() -> Vec<FiFile> {
    vec![
        FiFile {
            path: "src/api.ts".to_string(),
            content_hash: Some("h:api".to_string()),
            text: r#"
export function main() { handlePing(); }
export function handlePing() {}
export function dependency() {}
const x = process.env.API_URL;
const users = prisma.user.findMany();
@Inject("mailer")
const dep = Depends(dependency);
mcp.tool("ping", handlePing);
callTool("remote-tool");
"#
            .to_string(),
        },
        FiFile {
            path: "src/screen.tsx".to_string(),
            content_hash: Some("h:screen".to_string()),
            text: r#"
export function Home() { return null; }
<Route path="/" element={<Home />} />
navigate("/settings");
"#
            .to_string(),
        },
    ]
}

#[test]
fn framework_intelligence_emits_distinct_di_orm_config_rpc_and_ui_evidence() {
    let mut generation = base_generation();
    let files = fixture_files();
    let summary = augment_framework_intelligence(&mut generation, &files);

    assert!(summary.di >= 2, "di count was {}", summary.di);
    assert!(summary.orm >= 1, "orm count was {}", summary.orm);
    assert!(summary.config >= 1, "config count was {}", summary.config);
    assert!(summary.rpc >= 2, "rpc count was {}", summary.rpc);
    assert!(summary.ui >= 2, "ui count was {}", summary.ui);

    assert!(generation
        .nodes
        .iter()
        .any(|n| n.labels.contains(&"ConfigKey".to_string()) && n.name == "API_URL"));
    assert!(generation
        .nodes
        .iter()
        .any(|n| n.labels.contains(&"DatabaseModel".to_string()) && n.name == "user"));

    let ping = generation
        .nodes
        .iter()
        .find(|n| n.labels.contains(&"ToolContract".to_string()) && n.name == "ping")
        .expect("ping tool contract node");
    assert!(generation.edges.iter().any(|e| e.kind == "HANDLES"
        && e.source == ping.id
        && e.target == "symbol:src/api.ts::handlePing"));

    let home_route = generation
        .nodes
        .iter()
        .find(|n| n.labels.contains(&"UiRoute".to_string()) && n.name == "/")
        .expect("home route node");
    assert!(generation.edges.iter().any(|e| e.kind == "ROUTES_TO"
        && e.source == home_route.id
        && e.target == "symbol:src/screen.tsx::Home"));
}

#[test]
fn unresolved_bindings_are_reported_as_sorted_frontiers() {
    // "remote-tool" is a consumer callTool with no matching provider —
    // exercised implicitly above via rpc consumer counting; here we assert
    // an actually-unresolvable DI target produces a frontier, matching the
    // legacy module's `dependency_binding_unresolved` reason.
    let mut generation = FiGeneration { nodes: vec![], edges: vec![] };
    let files = vec![FiFile {
        path: "src/x.ts".to_string(),
        content_hash: None,
        text: "const dep = Depends(missingSymbol);\n".to_string(),
    }];
    let summary = augment_framework_intelligence(&mut generation, &files);
    assert_eq!(summary.di, 0);
    assert_eq!(summary.frontiers.len(), 1);
    assert_eq!(summary.frontiers[0].relation, "USES");
    assert_eq!(summary.frontiers[0].target_name, "missingSymbol");
    assert_eq!(summary.frontiers[0].reason, "dependency_binding_unresolved");
}

#[test]
fn graph_adapter_publishes_framework_nodes_and_edges() {
    let mut graph = GraphGeneration {
        schema_version: 1,
        provider: "native-rust".into(),
        provider_version: "test".into(),
        generation_id: "g1".into(),
        source_hash: "h:source".into(),
        repo_root: ".".into(),
        complete: true,
        nodes: base_generation().nodes.into_iter().map(|node| membrane_blueprint::model::GraphNode {
            id: node.id,
            kind: node.kind,
            path: node.path,
            name: Some(node.name),
            generation_id: "g1".into(),
            evidence: vec![serde_json::json!({"labels": node.labels, "qualifiedName": node.qualified_name})],
        }).collect(),
        edges: vec![],
        files: vec![],
        truncation_reasons: vec![],
    };
    let files = vec![FileRecord {
        path: "src/api.ts".into(),
        absolute_path: std::path::PathBuf::from("src/api.ts"),
        bytes: b"mcp.tool(\"ping\", handlePing);".to_vec(),
        text: Some("export function handlePing() {}\nmcp.tool(\"ping\", handlePing);".into()),
        content_hash: "h:api".into(),
        semantic_content_hash: "h:api".into(),
        size: 52,
    }];
    let summary = augment_graph_generation(&mut graph, &files);
    assert_eq!(summary.rpc, 1);
    assert!(graph.nodes.iter().any(|node| node.name.as_deref() == Some("ping")));
    assert!(graph.edges.iter().any(|edge| edge.kind == "HANDLES" && edge.target.as_deref().is_some_and(|target| target.contains("handlePing"))));
}
