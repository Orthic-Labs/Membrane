//! Impact seed resolution (ported from
//! `blueprint/src/graph/analytics/change-impact.mjs`). Resolves all
//! supported impact seed families -- explicit node/anchor, file(s), stack
//! locations, unified diff, and git treeish -- into an advisory
//! `ImpactSeedEnvelope` without silently promoting an ambiguous lexical
//! candidate. The envelope is input to traversal; it never becomes a second
//! source of graph truth.

use crate::graph::GraphGeneration;
use crate::recall_circuit::resolve_seeds_native;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashSet};
use std::process::Command;
use std::sync::OnceLock;

fn normalize_path(value: &str) -> String {
    let replaced = value.trim().replace('\\', "/");
    replaced
        .strip_prefix("a/")
        .or_else(|| replaced.strip_prefix("b/"))
        .unwrap_or(&replaced)
        .to_owned()
}

fn git_changed_paths(root: &str, base: &str, head: &str) -> (Vec<String>, Option<Value>) {
    let from = base.trim();
    let to = if head.trim().is_empty() { "HEAD" } else { head.trim() };
    if from.is_empty() {
        return (Vec::new(), Some(json!({"reason": "treeish_base_required"})));
    }
    let output = Command::new("git")
        .args(["diff", "--name-only", "--diff-filter=ACDMRTUXB", from, to, "--"])
        .current_dir(root)
        .output();
    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let paths = text.lines().map(normalize_path).filter(|p| !p.is_empty()).collect();
            (paths, None)
        }
        _ => (Vec::new(), Some(json!({"reason": "treeish_unavailable", "base": from, "head": to}))),
    }
}

fn diff_paths(diff: &str) -> Vec<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^(?:\+\+\+|---)\s+([^\t ]+)").unwrap());
    let mut paths = Vec::new();
    for line in diff.lines() {
        if let Some(caps) = re.captures(line) {
            let path = &caps[1];
            if path != "/dev/null" {
                paths.push(normalize_path(path));
            }
        }
    }
    paths
}

struct StackLocation {
    path: String,
    line: u64,
}

fn stack_locations(stack: &str) -> Vec<StackLocation> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(?:^|\s|\()([A-Za-z0-9_./\\@ -]+\.[A-Za-z0-9]+):(\d+)(?::\d+)?\)?").unwrap());
    let mut locations = Vec::new();
    for caps in re.captures_iter(stack) {
        if let (Some(path), Some(line)) = (caps.get(1), caps.get(2)) {
            if let Ok(line_no) = line.as_str().parse::<u64>() {
                locations.push(StackLocation { path: normalize_path(path.as_str()), line: line_no });
            }
        }
    }
    locations
}

/// Find the smallest-span node whose evidence covers `path:line`, mirroring
/// the legacy SQL `symbols` lookup (tie-break: smaller span, then id).
fn symbol_at_line<'a>(generation: &'a GraphGeneration, path: &str, line: u64) -> Option<&'a str> {
    if line < 1 {
        return None;
    }
    let mut best: Option<(&str, u64)> = None;
    for node in &generation.nodes {
        if node.path.as_deref().map(normalize_path).as_deref() != Some(path) {
            continue;
        }
        let Some(evidence) = node.evidence.first() else { continue };
        let start = evidence.get("startLine").and_then(Value::as_u64).unwrap_or(0);
        let end = evidence.get("endLine").and_then(Value::as_u64).unwrap_or(start);
        if start == 0 || line < start || line > end {
            continue;
        }
        let span = end - start;
        match &best {
            Some((best_id, best_span)) if *best_span < span || (*best_span == span && *best_id <= node.id.as_str()) => {}
            _ => best = Some((node.id.as_str(), span)),
        }
    }
    best.map(|(id, _)| id)
}

pub struct SeedEnvelopeInput<'a> {
    pub file: Option<&'a str>,
    pub files: Vec<&'a str>,
    pub diff: Option<&'a str>,
    pub stack: Option<&'a str>,
    pub treeish_base: Option<&'a str>,
    pub treeish_head: Option<&'a str>,
    pub line: Option<u64>,
    pub node_id: Option<&'a str>,
    pub anchor: Option<&'a str>,
    pub test: Option<&'a str>,
    pub max_seeds: u64,
}

pub fn resolve_impact_seed_envelope(generation: &GraphGeneration, root: &str, generation_id: &str, input: SeedEnvelopeInput) -> Value {
    let mut omissions: Vec<Value> = Vec::new();
    let locations = input.stack.map(stack_locations).unwrap_or_default();

    let mut paths: Vec<String> = Vec::new();
    if let Some(file) = input.file {
        paths.push(normalize_path(file));
    }
    for file in &input.files {
        paths.push(normalize_path(file));
    }
    if let Some(diff) = input.diff {
        paths.extend(diff_paths(diff));
    }
    paths.extend(locations.iter().map(|loc| loc.path.clone()));

    let treeish = input
        .treeish_base
        .filter(|base| !base.trim().is_empty())
        .map(|base| (base.to_owned(), input.treeish_head.unwrap_or("HEAD").to_owned()));
    if let Some((base, head)) = &treeish {
        let (changed, omission) = git_changed_paths(root, base, head);
        paths.extend(changed);
        if let Some(omission) = omission {
            omissions.push(omission);
        }
    }

    let mut locations = locations;
    if let (Some(file), Some(line)) = (input.file, input.line) {
        if line > 0 {
            locations.push(StackLocation { path: normalize_path(file), line });
        }
    }

    // Explicit seed IDs retain discovery order (stack locations, node ID,
    // anchor), matching `resolveSeeds`' requested-order lane. Paths are a
    // sorted set in the public envelope, matching its changed-path shape.
    let mut node_ids_vec = Vec::new();
    let mut seen_seed_ids = HashSet::new();
    for id in locations.iter().filter_map(|loc| symbol_at_line(generation, &loc.path, loc.line)) {
        if seen_seed_ids.insert(id.to_owned()) {
            node_ids_vec.push(id.to_owned());
        }
    }
    if let Some(node_id) = input.node_id.filter(|id| !id.trim().is_empty()) {
        if seen_seed_ids.insert(node_id.to_owned()) {
            node_ids_vec.push(node_id.to_owned());
        }
    }
    if let Some(anchor) = input.anchor.filter(|anchor| !anchor.trim().is_empty()) {
        if seen_seed_ids.insert(anchor.to_owned()) {
            node_ids_vec.push(anchor.to_owned());
        }
    }

    let anchors: BTreeSet<String> = paths.into_iter().filter(|p| !p.is_empty()).collect();
    let mut anchors = anchors;
    if let Some(anchor) = input.anchor.filter(|anchor| !anchor.trim().is_empty()) {
        anchors.insert(anchor.to_owned());
    }

    let anchors_vec: Vec<String> = anchors.iter().cloned().collect();
    // Preserve the established diagnostic for an explicitly requested file
    // that has no corresponding graph address. Native `resolveSeeds` still
    // contributes its canonical `no_relevant_seed` resolution below.
    for requested in input
        .file
        .into_iter()
        .chain(input.files.iter().copied())
        .map(normalize_path)
        .filter(|path| !path.is_empty())
    {
        if !generation.nodes.iter().any(|node| node.path.as_deref().map(normalize_path).as_deref() == Some(requested.as_str())) {
            omissions.push(json!({"reason": "anchor_not_found", "anchor": requested}));
        }
    }
    let task = input.test.unwrap_or_else(|| input.anchor.unwrap_or(""));
    let resolution = resolve_seeds_native(
        &generation.nodes,
        &node_ids_vec,
        &anchors_vec,
        task,
        input.max_seeds.max(1) as usize,
        false,
    );
    if resolution.state == "ambiguous" {
        omissions.push(json!({"reason": "ambiguous_seed", "candidates": resolution.json.get("candidates").cloned().unwrap_or_else(|| json!([]))}));
    } else if resolution.state == "unresolved" {
        omissions.push(json!({"reason": resolution.json.get("reason").and_then(Value::as_str).unwrap_or("no_relevant_seed")}));
    }

    json!({
        "schemaVersion": 1,
        "kind": "ImpactSeedEnvelope",
        "generationId": generation_id,
        "families": {
            "node": input.node_id.is_some_and(|id| !id.trim().is_empty()),
            "file": !anchors_vec.is_empty(),
            "line": !locations.is_empty(),
            "diff": input.diff.is_some_and(|value| !value.is_empty()),
            "stack": input.stack.is_some_and(|value| !value.is_empty()),
            "test": input.test.is_some_and(|value| !value.is_empty()),
            "treeish": treeish.is_some(),
        },
        "changedPaths": anchors_vec,
        "seeds": resolution.json.get("seeds").cloned().unwrap_or_else(|| json!([])),
        "resolution": resolution.json,
        "omissions": omissions,
    })
}
