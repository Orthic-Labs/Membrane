#!/usr/bin/env python3
"""One-shot, exact-context wiring for push-end-to-end. Removed after application.
No network, no credentials, no command execution beyond the owning CI checkout.
All changes are planned in memory before any existing source is written.
"""
from pathlib import Path
import json

ROOT = Path(__file__).resolve().parents[2]
changed = {}
def get(p):
    if p not in changed: changed[p] = (ROOT / p).read_text()
    return changed[p]
def put(p, text): changed[p] = text
def replace(p, old, new, count=1):
    text = get(p)
    if text.count(old) != count: raise RuntimeError(f'{p}: expected {count} exact contexts, got {text.count(old)}: {old[:100]!r}')
    put(p, text.replace(old, new))
R = 'engine/crates/membrane-runtime/src/'
P = R + 'push/'
M = 'engine/crates/membrane-mcp/src/'

# Register new mechanisms without introducing another crate/protocol owner.
put(P+'mod.rs', get(P+'mod.rs') + '\npub mod recovery;\npub mod fidelity;\npub mod delivery;\npub mod ast;\npub mod api;\n')
replace(P+'recovery.rs', '    pub fn local() -> Result<Self, RecoveryError> {', '    pub fn binding(&self) -> &str { &self.id }\n    pub fn local() -> Result<Self, RecoveryError> {')
replace(P+'recovery.rs', '    fn reference(connection: &Connection,', '    pub fn identity(&self) -> Result<String, RecoveryError> {\n        self.connection()?.query_row("SELECT identity FROM push_store WHERE id=1", [], |r| r.get(0)).map_err(db_error)\n    }\n    fn reference(connection: &Connection,')
replace(P+'recovery.rs', '        tx.execute("DELETE FROM push_originals WHERE expires <= ?1", [now]).map_err(db_error)?;', '''        // Expired/invalidation tombstones prevent an old handle silently becoming
        // valid again. Explicit retention maintenance is separate from publication.
        let old_size: Option<(usize, usize, u64)> = tx.query_row(
            "SELECT size,length(content),expires FROM push_originals WHERE scope=?1 AND digest=?2",
            params![scope.id, hash], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db_error)?;
        if let Some((size, actual, expires)) = old_size {
            if now >= expires { return Err(RecoveryError::Expired); }
            if size != actual || size > MAX_ARTIFACT_BYTES { return Err(RecoveryError::Corrupt); }
        }''')
replace(P+'delivery.rs', 'last == line', 'last.as_str() == line')
replace(P+'delivery.rs', '#[derive(Debug, Clone, Serialize, Deserialize)]\n#[serde(rename_all = "camelCase")]\npub struct DeliveryReceipt', '#[derive(Debug, Clone, Serialize)]\n#[serde(rename_all = "camelCase")]\npub struct DeliveryReceipt')

# Preserve the existing skeleton tests; replace only the obsolete renderer.
s = get(P+'skel.rs')
start = s.index('#[derive(Debug, Clone, Copy, PartialEq, Eq)]')
end = s.index('#[cfg(test)]')
s = s[:start] + '''pub fn skeletonize_with_spans(path: &Path, src: &str) -> (String, Vec<super::fidelity::SpanMapping>) {
    super::ast::render(path, src)
}
pub fn skeletonize(path: &Path, src: &str) -> String { skeletonize_with_spans(path, src).0 }
pub fn skeletonize_to_budget(path: &Path, src: &str, budget_tokens: usize) -> BudgetSkeleton {
    let input_tokens = super::compress::estimate_tokens(src);
    let candidate = skeletonize(path, src);
    let candidate_tokens = super::compress::estimate_tokens(&candidate);
    let (text, level) = if candidate_tokens <= budget_tokens {
        (candidate, "signature")
    } else {
        // A path alone is not a retained original. Keep exact source and make
        // the unsatisfied budget observable to the final delivery owner.
        (src.to_owned(), "exact-over-budget")
    };
    let output_tokens = super::compress::estimate_tokens(&text);
    BudgetSkeleton { text, input_tokens, output_tokens, budget_tokens, level,
        budget_met:output_tokens <= budget_tokens, recovery_marker:None }
}

''' + s[end:]
s = s.replace('use tree_sitter::{Language, Node, Parser, TreeCursor};\n', '')
s = s.replace('fn budget_skeleton_degrades_to_path_stub()', 'fn budget_skeleton_keeps_exact_when_no_faithful_view_fits()')
s = s.replace('assert_eq!(out.level, "path-stub");', 'assert_eq!(out.level, "exact-over-budget");')
s = s.replace('assert!(out.text.contains("src/internal.rs"));', 'assert_eq!(out.text, src);')
put(P+'skel.rs', s)

# Native discovery uses the canonical additive Push schemas. Default V1
# discovery remains unchanged; hosts explicitly request the push toolset.
replace(M+'tools.rs', '    "membrane_feedback",\n];', '    "membrane_feedback",\n    "membrane_push_prepare",\n    "membrane_push_resolve",\n];')
replace(M+'tools.rs', 'fn schema(name: &str) -> Value {', '''fn schema(name: &str) -> Value {
    if name.starts_with("membrane_push_") {
        let definitions: Value = serde_json::from_str(include_str!("../../../../schemas/registry/push-tools.v1.json")).expect("Push schemas parse");
        return definitions.as_array().unwrap().iter().find(|v| v["name"] == name).expect("Push tool registered")["inputSchema"].clone();
    }''')
replace(M+'tools.rs', '"default" | "memory" | "blueprint" | "diagnostic"', '"default" | "memory" | "blueprint" | "diagnostic" | "push"')
replace(M+'tools.rs', '"memory" => &CORE[3..],', '"memory" => &CORE[3..10],\n            "push" => &CORE[10..],')
replace(M+'tools.rs', '        "membrane_context"\n        | "membrane_source_read"', '        "membrane_push_resolve"\n        | "membrane_context"\n        | "membrane_source_read"')
replace(R+'mcp_executor.rs', '        "membrane_source_read" => "source_read",', '        "membrane_source_read" | "membrane_push_prepare" | "membrane_push_resolve" => "source_read",')
replace(R+'mcp_executor.rs', '        match name {\n            "membrane_context" => {', '        match name {\n            "membrane_push_prepare" | "membrane_push_resolve" => crate::push::api::execute(name, arguments),\n            "membrane_context" => {')
# Pass only the content-free decision, not full alternate representation bodies.
replace(R+'mcp_executor.rs', '                        "receipts":receipts,', '                        "receipts":receipts,\n                        "packetReduction":federated.get("packetReduction").and_then(|v| v.get("selectionReceipt")).cloned(),')

# Shared authenticated HTTP operation owner. Legacy unscoped /expand is refused;
# callers migrate to caller/repository binding and exact selectors.
replace(R+'serve.rs', '    if method == "POST" && path == "/expand" {\n        return expand_anchor_response(body, &configured_anchor_directory());\n    }', '''    if method == "POST" && matches!(path, "/expand" | "/push/resolve") {
        return crate::push::api::http_response("membrane_push_resolve", body);
    }
    if method == "POST" && path == "/push/prepare" {
        return crate::push::api::http_response("membrane_push_prepare", body);
    }''')
# Defense-in-depth for direct/test legacy expansion: no unverified return.
replace(R+'serve.rs', '    (\n        200,\n        json!({"anchor":anchor,"sha256":sha256_bytes(content.as_bytes()),"content":content})', '''    let marker = std::fs::read_to_string(&metadata).ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| value.get("recovery").cloned())
        .and_then(|value| serde_json::from_value::<crate::push::compress::RecoveryMarkerV1>(value).ok());
    if sha256_bytes(content.as_bytes()) != digest || !marker.as_ref().is_some_and(|marker| crate::push::compress::verify_recovery_marker(marker, content.as_bytes(), crate::push::recovery::now_ms())) {
        return (409, json!({"error":"anchor integrity or metadata invalid"}).to_string());
    }
    (
        200,
        json!({"anchor":anchor,"sha256":sha256_bytes(content.as_bytes()),"content":content})''')
# Preserve supported transport inputs and reduction receipts. No H8 is invented.
replace('mcp/client.mjs', '    sufficiencyContract: parsed.sufficiencyContract,', '    sufficiencyContract: parsed.sufficiencyContract,\n    remainingContextCeiling: parsed.remainingContextCeiling,')
c = get('mcp/client.mjs')
c = c.replace('    packet: parsed.packet ?? null,', '    packet: parsed.packet ?? null,\n    packetReduction: parsed.packetReduction?.selectionReceipt ?? parsed.packetReduction ?? null,')
put('mcp/client.mjs', c)
replace('mcp/host/context-adapter.cjs', '    maxTokens: Number.isInteger(event.max_tokens) ? event.max_tokens : 6420,', '    maxTokens: Number.isInteger(event.max_tokens) ? event.max_tokens : 6420,\n    remainingContextCeiling: event.remainingContextCeiling,')
replace('mcp/host/context-adapter.cjs', "    anchors: request.anchors || '',", "    anchors: request.anchors || '',\n    remainingContextCeiling: request.remainingContextCeiling,")

# Refusal/unchanged code is terminal, not permission to use a prose compressor.
p = get(P+'selection.rs')
a = p.index('        if skeletonized.trim().is_empty() || skeletonized == block.text {')
b = p.index('    } else {\n        block.text = match policy', a)
p = p[:a] + '''        if skeletonized.trim().is_empty() || skeletonized == block.text {
            "kept-exact"
        } else {
            block.text = skeletonized;
            "skel"
        }
''' + p[b:]
put(P+'selection.rs', p)
replace(R+'pull/federation.rs', 'crate::push::prep::PushPolicy::query_aware(task.to_owned(), true, true)', '''// A mode request is not admission/freshness proof. Until the owner
        // supplies a receipt-bound policy, this is a terminal exact refusal.
        crate::push::prep::PushPolicy::query_aware(task.to_owned(), false, false)''')

# Small command captures have identities but no published recovery handles.
replace(P+'runc.rs', '        anchor: format!("mr://anchor/{digest}"),\n        exit_code,', '        anchor: if recovery_marker.is_some() { format!("mr://anchor/{digest}") } else { String::new() },\n        exit_code,')
replace(R+'cli.rs', '            println!("[anchor] {}", result.anchor);', '            if !result.anchor.is_empty() { println!("[anchor] {}", result.anchor); }')
a = get(R+'cli.rs').index('            let directory = spill_dir.map(PathBuf::from).unwrap_or_else(|| {')
b = get(R+'cli.rs').index('            let command_line = cmd.join(" ");', a)
put(R+'cli.rs', get(R+'cli.rs')[:a] + '            let directory = spill_dir.map(PathBuf::from).unwrap_or_else(crate::push::recovery::default_directory);\n' + get(R+'cli.rs')[b:])

# Do not leave a reusable source-mutating applicator in the finished branch.
for path, content in changed.items():
    (ROOT / path).write_text(content)
    print('wired', path)
Path(__file__).unlink()
