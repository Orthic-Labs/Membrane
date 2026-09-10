// Port of blueprint/src/providers/bridges/seams.mjs: a regex-rule scan for
// explicit cross-language FFI/interop seams (extern "C", ctypes.CDLL, JNI
// native methods, JNIEXPORT/JNICALL, cgo `import "C"`, gRPC service/rpc,
// P/Invoke DllImport, wasm-bindgen/WebAssembly.instantiate, COM
// ComImport/CoCreateInstance). Emits CrossLanguageBridge nodes and CONTAINS
// edges from the owning file, never CALLS edges.

use std::sync::LazyLock;
use regex::Regex;

use serde_json::json;

use crate::model::{GraphEdge, GraphNode};

use super::{ProviderContext, ProviderOutput};

pub const BRIDGE_PROVIDER_ID: &str = "blueprint-bridge-seams";
pub const BRIDGE_PROVIDER_VERSION: &str = "explicit-seams-v1";

/// One detected bridge seam fact.
#[derive(Debug, Clone, PartialEq)]
pub struct BridgeSeam {
    pub bridge_kind: String,
    pub target: String,
    pub path: String,
    pub line: usize,
}

fn t_c_abi(_: &regex::Captures) -> String {
    "C ABI".to_string()
}
fn t_group1(caps: &regex::Captures) -> String {
    caps.get(1).map(|m| m.as_str().to_string()).unwrap_or_default()
}
fn t_c(_: &regex::Captures) -> String {
    "C".to_string()
}
fn t_wasm_bindgen(_: &regex::Captures) -> String {
    "wasm-bindgen".to_string()
}
fn t_webassembly(_: &regex::Captures) -> String {
    "WebAssembly".to_string()
}
fn t_com(_: &regex::Captures) -> String {
    "COM".to_string()
}
fn t_com_or_group1(caps: &regex::Captures) -> String {
    caps.get(1)
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| "COM".to_string())
}

static RE_FFI_RUST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\bextern\s+"C""#).unwrap());
static RE_FFI_PY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\bctypes\.(?:CDLL|PyDLL|WinDLL)\(\s*["']([^"']+)["']"#).unwrap());
static RE_JNI_JAVA: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\bnative\s+[A-Za-z_$][\w$<>\[\]]*\s+([A-Za-z_$][\w$]*)\s*\("#).unwrap());
static RE_JNI_C: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?s)\bJNIEXPORT\b.*?\bJNICALL\s+([A-Za-z_$][\w$]*)"#).unwrap());
static RE_CGO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^\s*import\s+"C"\s*$"#).unwrap());
static RE_GRPC_SERVICE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^\s*service\s+([A-Za-z_]\w*)\s*\{?"#).unwrap());
static RE_GRPC_RPC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^\s*rpc\s+([A-Za-z_]\w*)\s*\("#).unwrap());
static RE_PINVOKE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\[DllImport\(\s*["']([^"']+)["']"#).unwrap());
static RE_WASM_BINDGEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"#\[wasm_bindgen(?:\([^\]]*\))?\]"#).unwrap());
static RE_WASM_JS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\bWebAssembly\.(?:instantiate|instantiateStreaming|compile)\s*\("#).unwrap());
static RE_COM_IMPORT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\[(?:ComImport|Guid)\b"#).unwrap());
static RE_COM_CREATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\b(?:CoCreateInstance|CreateObject)\s*\(\s*["']?([^"')\s]+)?"#).unwrap());

struct RuleSpec {
    kind: &'static str,
    extensions: &'static [&'static str],
    regex: &'static LazyLock<Regex>,
    target: fn(&regex::Captures) -> String,
}

static RULES: &[RuleSpec] = &[
    RuleSpec { kind: "FFI", extensions: &["rs"], regex: &RE_FFI_RUST, target: t_c_abi },
    RuleSpec { kind: "FFI", extensions: &["py"], regex: &RE_FFI_PY, target: t_group1 },
    RuleSpec { kind: "JNI", extensions: &["java", "kt"], regex: &RE_JNI_JAVA, target: t_group1 },
    RuleSpec {
        kind: "JNI",
        extensions: &["c", "cc", "cpp", "cxx", "h", "hpp"],
        regex: &RE_JNI_C,
        target: t_group1,
    },
    RuleSpec { kind: "cgo", extensions: &["go"], regex: &RE_CGO, target: t_c },
    RuleSpec { kind: "gRPC", extensions: &["proto"], regex: &RE_GRPC_SERVICE, target: t_group1 },
    RuleSpec { kind: "gRPC", extensions: &["proto"], regex: &RE_GRPC_RPC, target: t_group1 },
    RuleSpec { kind: "PInvoke", extensions: &["cs"], regex: &RE_PINVOKE, target: t_group1 },
    RuleSpec { kind: "WASM", extensions: &["rs"], regex: &RE_WASM_BINDGEN, target: t_wasm_bindgen },
    RuleSpec {
        kind: "WASM",
        extensions: &["js", "jsx", "ts", "tsx", "mjs", "cjs"],
        regex: &RE_WASM_JS,
        target: t_webassembly,
    },
    RuleSpec { kind: "COM", extensions: &["cs"], regex: &RE_COM_IMPORT, target: t_com },
    RuleSpec {
        kind: "COM",
        extensions: &["cs", "cpp", "cc", "cxx", "c", "ps1", "vbs"],
        regex: &RE_COM_CREATE,
        target: t_com_or_group1,
    },
];

fn extension(path: &str) -> String {
    match path.rfind('.') {
        Some(idx) => path[idx + 1..].to_lowercase(),
        None => String::new(),
    }
}

/// Strip comment-only lines and inline comments, mirroring the legacy
/// `sourceLine` helper. Returns `None` when the line contributes no code.
fn source_line(raw: &str, ext: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("//")
        || (trimmed.starts_with('#') && !(ext == "rs" && trimmed.starts_with("#[")))
        || trimmed.starts_with("/*")
        || trimmed.starts_with('*')
    {
        return None;
    }
    static BLOCK_COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)/\*.*?\*/").unwrap());
    static LINE_COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"//.*$").unwrap());
    let no_block = BLOCK_COMMENT.replace_all(raw, "");
    let no_line = LINE_COMMENT.replace(&no_block, "");
    Some(no_line.trim_end().to_string())
}

/// Scan one file's text for explicit cross-language bridge seams.
pub fn scan_explicit_bridge_seams(path: &str, text: &str) -> Vec<BridgeSeam> {
    let ext = extension(path);
    let mut seams = Vec::new();
    for (index, raw_line) in text.split(['\n']).enumerate() {
        let raw_line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let Some(line) = source_line(raw_line, &ext) else { continue };
        for rule in RULES {
            if !rule.extensions.contains(&ext.as_str()) {
                continue;
            }
            if let Some(caps) = rule.regex.captures(&line) {
                seams.push(BridgeSeam {
                    bridge_kind: rule.kind.to_string(),
                    target: (rule.target)(&caps),
                    path: path.to_string(),
                    line: index + 1,
                });
            }
        }
    }
    seams
}

fn safe_id(value: &str) -> String {
    static UNSAFE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^A-Za-z0-9_.-]+").unwrap());
    let replaced = UNSAFE.replace_all(value, "-");
    let trimmed = replaced.trim_matches('-');
    let truncated: String = trimmed.chars().take(80).collect();
    if truncated.is_empty() { "seam".to_string() } else { truncated }
}

/// Provider entry point registered in `providers::registry()` under id
/// `"blueprint-bridges"` (see [`super::PROVIDER_ORDER`]).
pub fn run(ctx: &ProviderContext<'_>) -> ProviderOutput {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    for file in ctx.files {
        let text = file.text.as_deref().unwrap_or("");
        for seam in scan_explicit_bridge_seams(&file.path, text) {
            let node_id = format!(
                "bridge:{}:{}:{}:{}",
                seam.path,
                seam.line,
                seam.bridge_kind,
                safe_id(&seam.target)
            );
            let evidence = json!({
                "path": seam.path,
                "startLine": seam.line,
                "endLine": seam.line,
                "contentHash": file.content_hash,
                "bridgeKind": seam.bridge_kind,
                "target": seam.target,
                "labels": ["CrossLanguageBridge", seam.bridge_kind],
                "qualifiedName": format!("{}:{}:{}:{}", seam.path, seam.line, seam.bridge_kind, seam.target),
                "provider": BRIDGE_PROVIDER_ID,
                "providerVersion": BRIDGE_PROVIDER_VERSION,
                "confidenceTier": "EXACT_RESOLUTION",
                "confidence": 1.0,
            });
            nodes.push(GraphNode {
                id: node_id.clone(),
                kind: "bridge".to_string(),
                path: Some(seam.path.clone()),
                name: Some(format!("{}:{}", seam.bridge_kind, seam.target)),
                generation_id: String::new(),
                evidence: vec![evidence.clone()],
            });
            edges.push(GraphEdge {
                id: format!("edge:CONTAINS:file:{}->{}", seam.path, node_id),
                kind: "CONTAINS".to_string(),
                source: format!("file:{}", seam.path),
                target: Some(node_id),
                generation_id: String::new(),
                evidence: vec![evidence],
            });
        }
    }
    ProviderOutput { nodes, edges }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_rust_extern_c_and_wasm_bindgen() {
        let seams = scan_explicit_bridge_seams("native/lib.rs", "#[wasm_bindgen]\npub extern \"C\" fn start() {}\n");
        assert_eq!(seams.len(), 2);
        assert!(seams.iter().any(|s| s.bridge_kind == "WASM" && s.target == "wasm-bindgen"));
        assert!(seams.iter().any(|s| s.bridge_kind == "FFI" && s.target == "C ABI"));
    }

    #[test]
    fn cgo_import_c_detected_and_comments_ignored() {
        let seams = scan_explicit_bridge_seams("native/bridge.go", "package native\nimport \"C\"\n");
        assert_eq!(seams.len(), 1);
        assert_eq!(seams[0].bridge_kind, "cgo");

        let commented = scan_explicit_bridge_seams("comments.go", "package native\n// import \"C\"\n");
        assert!(commented.is_empty());
    }

    #[test]
    fn grpc_service_and_rpc_detected() {
        let seams = scan_explicit_bridge_seams(
            "api/service.proto",
            "service Greeter {\n  rpc Hello (Request) returns (Reply);\n}\n",
        );
        assert_eq!(seams.len(), 2);
        assert!(seams.iter().any(|s| s.bridge_kind == "gRPC" && s.target == "Greeter"));
        assert!(seams.iter().any(|s| s.bridge_kind == "gRPC" && s.target == "Hello"));
    }

    #[test]
    fn pinvoke_detected_and_commented_pinvoke_ignored() {
        let seams = scan_explicit_bridge_seams(
            "native/interop.cs",
            "[DllImport(\"kernel32.dll\")]\nstatic extern int Beep();\n",
        );
        assert_eq!(seams.len(), 1);
        assert_eq!(seams[0].bridge_kind, "PInvoke");
        assert_eq!(seams[0].target, "kernel32.dll");

        let commented = scan_explicit_bridge_seams("comments.cs", "// [DllImport(\"fake.dll\")]\n");
        assert!(commented.is_empty());
    }

    #[test]
    fn commented_ctypes_cdll_ignored() {
        let commented = scan_explicit_bridge_seams("comments.py", "#ctypes.CDLL('fake.so')\n");
        assert!(commented.is_empty());
    }

    #[test]
    fn run_emits_contains_edges_never_calls() {
        use crate::graph::FileRecord;
        use std::collections::BTreeMap;
        use std::path::PathBuf;

        let file = FileRecord {
            path: "native/bridge.go".to_string(),
            absolute_path: PathBuf::from("native/bridge.go"),
            bytes: b"package native\nimport \"C\"\n".to_vec(),
            text: Some("package native\nimport \"C\"\n".to_string()),
            content_hash: "h1".to_string(),
            semantic_content_hash: "h1".to_string(),
            size: 26,
        };
        let files = vec![file];
        let mut file_map: BTreeMap<String, &FileRecord> = BTreeMap::new();
        for f in &files {
            file_map.insert(f.path.clone(), f);
        }
        let ctx = ProviderContext { repo_root: std::path::Path::new("."), files: &files, file_map: &file_map };
        let output = run(&ctx);
        assert_eq!(output.nodes.len(), 1);
        let evidence = &output.nodes[0].evidence[0];
        assert_eq!(evidence["bridgeKind"], "cgo");
        assert!(output.edges.iter().all(|e| e.kind == "CONTAINS"));
        assert!(!output.edges.iter().any(|e| e.kind == "CALLS"));
    }
}
