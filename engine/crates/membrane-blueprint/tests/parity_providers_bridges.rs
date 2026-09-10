// Parity test for src/providers/bridges.rs against the legacy
// blueprint/src/providers/bridges/seams.mjs, using real fixture text
// lifted from blueprint/tests/production-provider-cluster.test.mjs
// ("production generation consumes module, framework, SQL, Terraform,
// SCIP, & explicit bridge providers" and "framework facts stay absent
// without explicit gates & seam comments never become evidence").

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use membrane_blueprint::graph::FileRecord;
use membrane_blueprint::providers::bridges::{
    run, scan_explicit_bridge_seams, BRIDGE_PROVIDER_ID, BRIDGE_PROVIDER_VERSION,
};
use membrane_blueprint::providers::ProviderContext;

fn file_record(path: &str, text: &str, content_hash: &str) -> FileRecord {
    FileRecord {
        path: path.to_string(),
        absolute_path: PathBuf::from(path),
        bytes: text.as_bytes().to_vec(),
        text: Some(text.to_string()),
        content_hash: content_hash.to_string(),
        semantic_content_hash: content_hash.to_string(),
        size: text.len() as u64,
    }
}

fn with_context<T>(files: &[FileRecord], f: impl FnOnce(&ProviderContext<'_>) -> T) -> T {
    let mut file_map: BTreeMap<String, &FileRecord> = BTreeMap::new();
    for file in files {
        file_map.insert(file.path.clone(), file);
    }
    let ctx = ProviderContext { repo_root: Path::new("."), files, file_map: &file_map };
    f(&ctx)
}

#[test]
fn provider_identity_matches_legacy() {
    assert_eq!(BRIDGE_PROVIDER_ID, "blueprint-bridge-seams");
    assert_eq!(BRIDGE_PROVIDER_VERSION, "explicit-seams-v1");
}

#[test]
fn detects_ffi_cgo_grpc_pinvoke_wasm_across_fixture_files() {
    // Fixture text copied verbatim from production-provider-cluster.test.mjs.
    let files = vec![
        file_record("native/lib.rs", "#[wasm_bindgen]\npub extern \"C\" fn start() {}\n", "h:lib.rs"),
        file_record("native/bridge.go", "package native\nimport \"C\"\n", "h:bridge.go"),
        file_record(
            "api/service.proto",
            "service Greeter {\n  rpc Hello (Request) returns (Reply);\n}\n",
            "h:service.proto",
        ),
        file_record("native/interop.cs", "[DllImport(\"kernel32.dll\")]\nstatic extern int Beep();\n", "h:interop.cs"),
    ];

    let output = with_context(&files, |ctx| run(ctx));

    // Legacy assertion: bridgeKind set is exactly {FFI, cgo, gRPC, PInvoke, WASM}.
    let mut kinds: Vec<String> = output
        .nodes
        .iter()
        .map(|n| n.evidence[0]["bridgeKind"].as_str().unwrap().to_string())
        .collect();
    kinds.sort();
    kinds.dedup();
    assert_eq!(kinds, vec!["FFI", "PInvoke", "WASM", "cgo", "gRPC"].into_iter().collect::<std::collections::BTreeSet<_>>().into_iter().collect::<Vec<_>>());

    // Legacy assertion: >= 5 seam nodes (FFI extern"C", WASM wasm-bindgen, cgo,
    // gRPC service, gRPC rpc, PInvoke == 6 total facts across these fixtures).
    assert!(output.nodes.len() >= 5, "expected >=5 bridge nodes, got {}", output.nodes.len());

    // Legacy assertion: every bridge edge is CONTAINS, never CALLS.
    assert!(output.edges.len() >= output.nodes.len());
    assert!(output.edges.iter().all(|e| e.kind == "CONTAINS"));
    assert!(!output.edges.iter().any(|e| e.kind == "CALLS"));

    // Legacy assertion: every bridge node carries evidence with a contentHash.
    for node in &output.nodes {
        assert!(node.evidence[0]["contentHash"].as_str().is_some_and(|s| !s.is_empty()));
    }

    // Provider identity stamped on every fact.
    assert!(output.nodes.iter().all(|n| n.evidence[0]["provider"] == BRIDGE_PROVIDER_ID));
    assert!(output.edges.iter().all(|e| e.evidence[0]["provider"] == BRIDGE_PROVIDER_ID));
}

#[test]
fn framework_facts_stay_absent_and_seam_comments_never_become_evidence() {
    // Fixture text copied verbatim from production-provider-cluster.test.mjs
    // "framework facts stay absent without explicit gates & seam comments
    // never become evidence".
    let files = vec![
        file_record("plain.ts", "app.get('/fake', handler);\npublish('fake.topic');\norders.save(value);\n", "h:plain"),
        file_record("comments.go", "package native\n// import \"C\"\n", "h:comments.go"),
        file_record("comments.cs", "// [DllImport(\"fake.dll\")]\n", "h:comments.cs"),
        file_record("comments.py", "#ctypes.CDLL('fake.so')\n", "h:comments.py"),
    ];

    let output = with_context(&files, |ctx| run(ctx));
    assert_eq!(output.nodes.len(), 0, "commented-out seam syntax must never become evidence");
    assert_eq!(output.edges.len(), 0);
}

#[test]
fn incremental_single_file_build_emits_cgo_without_calls_edges() {
    // Fixture text copied verbatim from production-provider-cluster.test.mjs
    // "incremental file build emits same local provider facts without seam
    // CALLS".
    let files = vec![file_record("native/bridge.go", "package native\nimport \"C\"\n", "h:bridge.go")];
    let output = with_context(&files, |ctx| run(ctx));
    assert!(output
        .nodes
        .iter()
        .any(|n| n.evidence[0]["provider"] == BRIDGE_PROVIDER_ID && n.evidence[0]["bridgeKind"] == "cgo"));
    assert!(!output.edges.iter().any(|e| e.evidence[0]["provider"] == BRIDGE_PROVIDER_ID && e.kind == "CALLS"));
}

#[test]
fn scan_matches_legacy_per_rule_targets() {
    // Direct rule-level parity checks against seams.mjs RULES table.
    let cases: &[(&str, &str, &str, &str)] = &[
        ("a.rs", "extern \"C\" fn f();", "FFI", "C ABI"),
        ("a.py", "ctypes.CDLL(\"libfoo.so\")", "FFI", "libfoo.so"),
        ("a.java", "native void doThing();", "JNI", "doThing"),
        ("a.cpp", "JNIEXPORT void JNICALL Java_Foo_bar(JNIEnv* env) {}", "JNI", "Java_Foo_bar"),
        ("a.go", "import \"C\"", "cgo", "C"),
        ("a.proto", "service Greeter {", "gRPC", "Greeter"),
        ("a.proto", "rpc Hello (Req) returns (Res);", "gRPC", "Hello"),
        ("a.cs", "[DllImport(\"user32.dll\")]", "PInvoke", "user32.dll"),
        ("a.rs", "#[wasm_bindgen]", "WASM", "wasm-bindgen"),
        ("a.js", "WebAssembly.instantiate(buf);", "WASM", "WebAssembly"),
        ("a.cs", "[ComImport]", "COM", "COM"),
        ("a.cs", "CoCreateInstance(CLSID_Foo)", "COM", "CLSID_Foo"),
    ];
    for (path, line, expected_kind, expected_target) in cases {
        let seams = scan_explicit_bridge_seams(path, line);
        assert!(
            seams.iter().any(|s| s.bridge_kind == *expected_kind && s.target == *expected_target),
            "expected {} seam with target {:?} from {:?} on {}, got {:?}",
            expected_kind,
            expected_target,
            line,
            path,
            seams
        );
    }
}
