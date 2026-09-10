//! Parity tests ported from the provider/module-resolution behavior of
//! `blueprint/src/providers/build.mjs`, `.../modules/javascript.mjs`, and
//! `.../modules/python-resolver.mjs`. Real temp-dir file trees, matching
//! the fixture style already used by `parity_dependency_dag.rs` /
//! `native_engine.rs` (`tempfile::tempdir`), not mocked filesystems.

use membrane_blueprint::module_resolution::{
    extract_javascript_module_specifiers, extract_python_module_specifiers, resolve_exact_first, resolve_js_module,
    resolve_python_module, IdentityCandidate, IdentityResolution, JsResolution, JsResolveInput, PyMiss, PyResolution,
    PyResolveInput, PY_STDLIB,
};
use std::fs;
use tempfile::tempdir;

// --- exact-first / same-tier-ambiguity-stops (build.mjs discipline) ---

#[test]
fn exact_first_prefers_exact_over_heuristic() {
    let candidates = vec![
        IdentityCandidate { target_id: "heuristic_target".into(), confidence_tier: "CROSS_FILE_HEURISTIC".into() },
        IdentityCandidate { target_id: "exact_target".into(), confidence_tier: "EXACT_RESOLUTION".into() },
    ];
    match resolve_exact_first(&candidates) {
        IdentityResolution::Resolved { target_id, confidence_tier } => {
            assert_eq!(target_id, "exact_target");
            assert_eq!(confidence_tier, "EXACT_RESOLUTION");
        }
        other => panic!("expected Resolved, got {other:?}"),
    }
}

#[test]
fn tied_exact_candidates_stop_resolution_as_ambiguous_not_arbitrary_pick() {
    let candidates = vec![
        IdentityCandidate { target_id: "target_b".into(), confidence_tier: "EXACT_RESOLUTION".into() },
        IdentityCandidate { target_id: "target_a".into(), confidence_tier: "EXACT_RESOLUTION".into() },
        // A worse-tier candidate must never break the tie or get promoted.
        IdentityCandidate { target_id: "target_c".into(), confidence_tier: "SAME_FILE_LEXICAL".into() },
    ];
    match resolve_exact_first(&candidates) {
        IdentityResolution::Ambiguous { confidence_tier, candidates } => {
            assert_eq!(confidence_tier, "EXACT_RESOLUTION");
            assert_eq!(candidates, vec!["target_a".to_string(), "target_b".to_string()]);
        }
        other => panic!("expected Ambiguous, got {other:?}"),
    }
}

// --- python-resolver.mjs: full field-for-field port ---

#[test]
fn python_stdlib_set_is_ported_completely_not_abbreviated() {
    // Spot-check breadth across the alphabet, plus the exact legacy count.
    assert_eq!(PY_STDLIB.len(), 113);
    for name in [
        "__future__", "asyncio", "collections", "dataclasses", "enum", "functools", "graphlib", "hashlib",
        "importlib", "json", "logging", "multiprocessing", "os", "pathlib", "queue", "re", "socket", "sqlite3",
        "subprocess", "sys", "tempfile", "threading", "typing", "unittest", "urllib", "warnings", "zoneinfo",
    ] {
        assert!(PY_STDLIB.contains(&name), "expected {name} in ported STDLIB set");
    }
}

#[test]
fn extract_python_specifiers_matches_from_and_import_lines() {
    let text = "from pkg.sub import Thing\nimport os\nimport a.b.c\nnot an import line\n";
    let found = extract_python_module_specifiers(text);
    assert_eq!(found.len(), 3);
    assert_eq!(found[0].specifier, "pkg.sub");
    assert_eq!(found[0].imported_name.as_deref(), Some("Thing"));
    assert_eq!(found[0].line, 1);
    assert_eq!(found[1].specifier, "os");
    assert_eq!(found[2].specifier, "a.b.c");
}

#[test]
fn resolve_python_module_relative_import_finds_sibling_module() {
    let repo = tempdir().unwrap();
    let root = repo.path();
    fs::create_dir_all(root.join("pkg")).unwrap();
    fs::write(root.join("pkg").join("__init__.py"), "").unwrap();
    fs::write(root.join("pkg").join("a.py"), "").unwrap();
    fs::write(root.join("pkg").join("b.py"), "from .a import thing\n").unwrap();

    let result = resolve_python_module(PyResolveInput {
        specifier: ".a",
        from_file: root.join("pkg").join("b.py").to_str().unwrap(),
        repo_root: root.to_str().unwrap(),
        imported_name: None,
    });
    match result {
        PyResolution::Resolved(hit) => {
            assert!(hit.resolved.ends_with("a.py"), "expected a.py, got {:?}", hit.resolved);
            assert_eq!(hit.reason, "relative_module");
        }
        other => panic!("expected Resolved, got {other:?}"),
    }
}

#[test]
fn resolve_python_module_relative_above_root_is_typed_miss() {
    let repo = tempdir().unwrap();
    let root = repo.path().join("nested");
    fs::create_dir_all(&root).unwrap();
    let file = root.join("mod.py");
    fs::write(&file, "").unwrap();
    // Two dots from a top-level-of-root file tries to escape above root.
    let result = resolve_python_module(PyResolveInput {
        specifier: "..sibling",
        from_file: file.to_str().unwrap(),
        repo_root: root.to_str().unwrap(),
        imported_name: None,
    });
    assert_eq!(result, PyResolution::Unresolved(PyMiss { reason: "relative_above_root".into() }));
}

#[test]
fn resolve_python_module_stdlib_import_is_typed_miss_not_resolved() {
    let repo = tempdir().unwrap();
    let root = repo.path();
    let file = root.join("main.py");
    fs::write(&file, "").unwrap();
    let result = resolve_python_module(PyResolveInput {
        specifier: "collections",
        from_file: file.to_str().unwrap(),
        repo_root: root.to_str().unwrap(),
        imported_name: None,
    });
    assert_eq!(result, PyResolution::Unresolved(PyMiss { reason: "stdlib:collections".into() }));
}

#[test]
fn resolve_python_module_finds_repo_package_and_not_repo_module_is_typed() {
    let repo = tempdir().unwrap();
    let root = repo.path();
    fs::create_dir_all(root.join("mypkg")).unwrap();
    fs::write(root.join("mypkg").join("__init__.py"), "").unwrap();
    let file = root.join("main.py");
    fs::write(&file, "").unwrap();

    let found = resolve_python_module(PyResolveInput {
        specifier: "mypkg",
        from_file: file.to_str().unwrap(),
        repo_root: root.to_str().unwrap(),
        imported_name: None,
    });
    match found {
        PyResolution::Resolved(hit) => assert!(hit.resolved.ends_with("__init__.py")),
        other => panic!("expected Resolved, got {other:?}"),
    }

    let missing = resolve_python_module(PyResolveInput {
        specifier: "totally_unknown_package",
        from_file: file.to_str().unwrap(),
        repo_root: root.to_str().unwrap(),
        imported_name: None,
    });
    assert_eq!(missing, PyResolution::Unresolved(PyMiss { reason: "not_repo_module:totally_unknown_package".into() }));
}

#[test]
fn resolve_python_module_follows_init_reexport_named_target() {
    let repo = tempdir().unwrap();
    let root = repo.path();
    fs::create_dir_all(root.join("mypkg").join("impl")).unwrap();
    fs::write(root.join("mypkg").join("impl.py"), "").unwrap();
    fs::write(
        root.join("mypkg").join("__init__.py"),
        "from .impl import Thing\n",
    )
    .unwrap();
    let file = root.join("main.py");
    fs::write(&file, "").unwrap();

    let result = resolve_python_module(PyResolveInput {
        specifier: "mypkg",
        from_file: file.to_str().unwrap(),
        repo_root: root.to_str().unwrap(),
        imported_name: Some("Thing"),
    });
    match result {
        PyResolution::Resolved(hit) => {
            assert!(hit.resolved.ends_with("impl.py"), "expected impl.py, got {:?}", hit.resolved);
            assert_eq!(hit.reason, "init_reexport");
        }
        other => panic!("expected Resolved via init_reexport, got {other:?}"),
    }
}

#[test]
fn resolve_python_module_invalid_module_name_is_typed_miss() {
    let repo = tempdir().unwrap();
    let root = repo.path();
    let file = root.join("main.py");
    fs::write(&file, "").unwrap();
    let result = resolve_python_module(PyResolveInput {
        specifier: "1invalid",
        from_file: file.to_str().unwrap(),
        repo_root: root.to_str().unwrap(),
        imported_name: None,
    });
    assert_eq!(result, PyResolution::Unresolved(PyMiss { reason: "invalid_module_name:1invalid".into() }));
}

#[test]
fn resolve_python_module_missing_input_is_typed_miss() {
    let result = resolve_python_module(PyResolveInput { specifier: "os", from_file: "", repo_root: "/r", imported_name: None });
    assert_eq!(result, PyResolution::Unresolved(PyMiss { reason: "missing_input".into() }));
}

// --- javascript.mjs: core relative/absolute resolution (scoped port) ---

#[test]
fn extract_javascript_specifiers_covers_import_export_and_require() {
    let text = "import { x } from \"./a.js\";\nconst y = require('./b.js');\nimport \"./c.js\";\n";
    let found = extract_javascript_module_specifiers(text);
    let specs: Vec<&str> = found.iter().map(|s| s.specifier.as_str()).collect();
    assert!(specs.contains(&"./a.js"));
    assert!(specs.contains(&"./b.js"));
    assert!(specs.contains(&"./c.js"));
}

#[test]
fn resolve_js_module_relative_exact_file_resolves() {
    let repo = tempdir().unwrap();
    let root = repo.path();
    fs::write(root.join("worker.js"), "export function work(){}\n").unwrap();
    let entry = root.join("entry.js");
    fs::write(&entry, "").unwrap();

    let result = resolve_js_module(JsResolveInput {
        specifier: "./worker.js",
        from_file: entry.to_str().unwrap(),
        repo_root: Some(root.to_str().unwrap()),
        is_typescript: false,
    });
    match result {
        JsResolution::Resolved { resolved, reason } => {
            assert!(resolved.ends_with("worker.js"));
            assert_eq!(reason, "relative");
        }
        other => panic!("expected Resolved, got {other:?}"),
    }
}

#[test]
fn resolve_js_module_relative_extension_resolution() {
    let repo = tempdir().unwrap();
    let root = repo.path();
    fs::write(root.join("worker.ts"), "export const x = 1;\n").unwrap();
    let entry = root.join("entry.ts");
    fs::write(&entry, "").unwrap();

    let result = resolve_js_module(JsResolveInput {
        specifier: "./worker",
        from_file: entry.to_str().unwrap(),
        repo_root: Some(root.to_str().unwrap()),
        is_typescript: true,
    });
    match result {
        JsResolution::Resolved { resolved, .. } => assert!(resolved.ends_with("worker.ts")),
        other => panic!("expected Resolved, got {other:?}"),
    }
}

#[test]
fn resolve_js_module_ambiguous_extension_is_typed_not_arbitrary() {
    let repo = tempdir().unwrap();
    let root = repo.path();
    fs::write(root.join("worker.ts"), "").unwrap();
    fs::write(root.join("worker.js"), "").unwrap();
    let entry = root.join("entry.ts");
    fs::write(&entry, "").unwrap();

    let result = resolve_js_module(JsResolveInput {
        specifier: "./worker",
        from_file: entry.to_str().unwrap(),
        repo_root: Some(root.to_str().unwrap()),
        is_typescript: true,
    });
    match result {
        JsResolution::Ambiguous { candidates, reason } => {
            assert_eq!(reason, "ambiguous_extension");
            assert_eq!(candidates.len(), 2);
        }
        other => panic!("expected Ambiguous, got {other:?}"),
    }
}

#[test]
fn resolve_js_module_index_resolution_for_directory_specifier() {
    let repo = tempdir().unwrap();
    let root = repo.path();
    fs::create_dir_all(root.join("lib")).unwrap();
    fs::write(root.join("lib").join("index.js"), "").unwrap();
    let entry = root.join("entry.js");
    fs::write(&entry, "").unwrap();

    let result = resolve_js_module(JsResolveInput {
        specifier: "./lib",
        from_file: entry.to_str().unwrap(),
        repo_root: Some(root.to_str().unwrap()),
        is_typescript: false,
    });
    match result {
        JsResolution::Resolved { resolved, reason } => {
            assert!(resolved.ends_with("index.js"));
            assert_eq!(reason, "relative");
        }
        other => panic!("expected Resolved, got {other:?}"),
    }
}

#[test]
fn resolve_js_module_bare_specifier_is_typed_unsupported_not_silently_resolved() {
    let repo = tempdir().unwrap();
    let root = repo.path();
    let entry = root.join("entry.js");
    fs::write(&entry, "").unwrap();
    let result = resolve_js_module(JsResolveInput {
        specifier: "lodash",
        from_file: entry.to_str().unwrap(),
        repo_root: Some(root.to_str().unwrap()),
        is_typescript: false,
    });
    assert_eq!(result, JsResolution::Unsupported("bare_specifier_resolution_not_ported"));
}

#[test]
fn resolve_js_module_missing_relative_target_is_typed_unresolved() {
    let repo = tempdir().unwrap();
    let root = repo.path();
    let entry = root.join("entry.js");
    fs::write(&entry, "").unwrap();
    let result = resolve_js_module(JsResolveInput {
        specifier: "./does-not-exist",
        from_file: entry.to_str().unwrap(),
        repo_root: Some(root.to_str().unwrap()),
        is_typescript: false,
    });
    assert_eq!(result, JsResolution::Unresolved { reason: "missing" });
}
