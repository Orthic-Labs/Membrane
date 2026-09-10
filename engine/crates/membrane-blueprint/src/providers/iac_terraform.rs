//! Native Rust port of `blueprint/src/providers/iac/terraform.mjs` (D29):
//! Terraform/HCL `resource "type" "name"` extraction and Kubernetes
//! manifest `kind:` extraction, profile-selected by file extension.
//!
//! No new dependency: the legacy source used a plain multi-line regex scan
//! over raw text (no `hcl`/YAML parser), so this port matches that exactly
//! — a structural regex, not a real HCL/YAML parser. It reads whichever
//! resource/kind lines are syntactically present, and does not validate the
//! surrounding block.

use regex::{Regex, RegexBuilder};
use std::fs;
use std::io;
use std::path::Path;
use std::sync::OnceLock;

use serde_json::json;
use crate::model::{GraphEdge, GraphNode};
use super::{ProviderContext, ProviderOutput};

/// A single extracted Terraform resource fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerraformFact {
    pub kind: String, // "resource"
    pub resource_type: String,
    pub name: String,
    pub line: usize,
}

/// A single extracted Kubernetes manifest fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KubernetesFact {
    pub kind: String, // "manifest"
    pub kind_name: String,
    pub line: usize,
}

fn tf_resource_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        RegexBuilder::new(r#"^\s*resource\s+"([^"]+)"\s+"([^"]+)""#)
            .multi_line(true)
            .build()
            .unwrap()
    })
}

fn k8s_kind_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        RegexBuilder::new(r"^\s*kind:\s*([A-Za-z]+)")
            .multi_line(true)
            .build()
            .unwrap()
    })
}

/// Line number (1-based) of the character offset `idx` within `text`,
/// matching legacy's `text.slice(0, match.index).split(/\r?\n/).length`.
fn line_at(text: &str, byte_idx: usize) -> usize {
    text.as_bytes()[..byte_idx]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

/// Port of `extractTerraformFacts(text)`.
pub fn extract_terraform_facts(text: &str) -> Vec<TerraformFact> {
    let mut facts = Vec::new();
    for cap in tf_resource_re().captures_iter(text) {
        let m = cap.get(0).unwrap();
        facts.push(TerraformFact {
            kind: "resource".into(),
            resource_type: cap[1].to_string(),
            name: cap[2].to_string(),
            line: line_at(text, m.start()),
        });
    }
    facts
}

/// Port of `extractKubernetesFacts(text)`.
pub fn extract_kubernetes_facts(text: &str) -> Vec<KubernetesFact> {
    let mut facts = Vec::new();
    for cap in k8s_kind_re().captures_iter(text) {
        let m = cap.get(0).unwrap();
        facts.push(KubernetesFact {
            kind: "manifest".into(),
            kind_name: cap[1].to_string(),
            line: line_at(text, m.start()),
        });
    }
    facts
}

/// Port of `profileFile(path)`: extension-based profile selection.
pub fn profile_file(path: &str) -> Option<&'static str> {
    let lower = path.to_lowercase();
    if lower.ends_with(".tf") {
        Some("terraform")
    } else if lower.ends_with(".yaml") || lower.ends_with(".yml") {
        Some("kubernetes")
    } else {
        None
    }
}

/// Facts extracted for a given profile — mirrors legacy's mixed-shape
/// return of `extractProfileFacts` by carrying both fact kinds in one enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileFact {
    Terraform(TerraformFact),
    Kubernetes(KubernetesFact),
}

/// Port of `extractProfileFacts(profile, filePath)`: reads the file from
/// disk and dispatches to the matching extractor.
pub fn extract_profile_facts(profile: &str, file_path: &Path) -> io::Result<Vec<ProfileFact>> {
    let text = fs::read_to_string(file_path)?;
    Ok(match profile {
        "terraform" => extract_terraform_facts(&text)
            .into_iter()
            .map(ProfileFact::Terraform)
            .collect(),
        "kubernetes" => extract_kubernetes_facts(&text)
            .into_iter()
            .map(ProfileFact::Kubernetes)
            .collect(),
        _ => Vec::new(),
    })
}

pub const PROVIDER_ID: &str = "blueprint-terraform";
pub const PROVIDER_VERSION: &str = "resource-facts-v1";

fn safe_name(value: &str) -> String {
    let mut out: String = value.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') { c } else { '-' }).collect();
    out.truncate(100);
    if out.is_empty() { "fact".into() } else { out }
}

/// Registry entry for IaC facts. The closed graph model has no extensible
/// domain fields, so Terraform/Kubernetes attributes stay in evidence.
pub fn run(ctx: &ProviderContext<'_>) -> ProviderOutput {
    let mut output = ProviderOutput::default();
    for file in ctx.files {
        let Some(profile) = profile_file(&file.path) else { continue };
        let text = file.text.as_deref().unwrap_or("");
        match profile {
            "terraform" => for fact in extract_terraform_facts(text) {
                let name = format!("{}.{}", fact.resource_type, fact.name);
                let id = format!("domain:{PROVIDER_ID}:{}:TerraformResource:{}:{}", file.path, safe_name(&name), fact.line);
                let evidence = json!({"path": file.path, "startLine": fact.line, "endLine": fact.line, "contentHash": file.content_hash, "provider": PROVIDER_ID, "providerVersion": PROVIDER_VERSION, "resourceType": fact.resource_type, "resourceName": fact.name, "confidenceTier": "EXACT_RESOLUTION"});
                output.nodes.push(GraphNode { id: id.clone(), kind: "domain".into(), path: Some(file.path.clone()), name: Some(name), generation_id: String::new(), evidence: vec![evidence.clone()] });
                output.edges.push(GraphEdge { id: format!("edge:CONFIGURES:file:{}->{id}:{PROVIDER_ID}:{}", file.path, fact.line), kind: "CONFIGURES".into(), source: format!("file:{}", file.path), target: Some(id), generation_id: String::new(), evidence: vec![evidence] });
            },
            // Kubernetes extraction remains available as a direct profile
            // helper, matching the legacy export; build integration admits
            // Terraform facts only.
            "kubernetes" => {}
            _ => {}
        }
    }
    output
}
