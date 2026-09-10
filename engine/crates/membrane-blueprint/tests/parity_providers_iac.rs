//! Parity tests for `providers::iac_terraform`, ported from the legacy
//! `blueprint/src/providers/iac/terraform.mjs`, exercised by the
//! Terraform/Kubernetes/profileFile cases of
//! `blueprint/tests/iac-profiles.test.mjs` (the SQL/Dockerfile/profileForPath
//! cases in that legacy file belong to `schemas/sql.mjs`, out of this
//! lane's domain, and are not ported here).

use std::fs;

use membrane_blueprint::providers::iac_terraform::{
    extract_kubernetes_facts, extract_profile_facts, extract_terraform_facts, profile_file,
    ProfileFact,
};

#[test]
fn terraform_facts_extract_resources_only_from_real_declarations_ambiguous_safe() {
    let tf = [
        "resource \"aws_db_instance\" \"orders\" {",
        "  # a comment mentioning resource \"fake\" \"thing\"",
        "}",
    ]
    .join("\n");
    let facts = extract_terraform_facts(&tf);
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].resource_type, "aws_db_instance");
    assert_eq!(facts[0].name, "orders");
}

#[test]
fn kubernetes_facts_extract_kinds_from_manifests() {
    let yaml = "apiVersion: v1\nkind: Service\nmetadata:\n  name: api\n---\nkind: Deployment\n";
    let facts = extract_kubernetes_facts(yaml);
    let kinds: Vec<&str> = facts.iter().map(|f| f.kind_name.as_str()).collect();
    assert_eq!(kinds, vec!["Service", "Deployment"]);
}

#[test]
fn profile_file_and_extract_profile_facts_work_from_disk() {
    let root = std::env::temp_dir().join(format!("blueprint-iac-rs-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let main_tf = root.join("main.tf");
    fs::write(&main_tf, "resource \"aws_s3_bucket\" \"data\" {}\n").unwrap();
    assert_eq!(profile_file("main.tf"), Some("terraform"));
    let facts = extract_profile_facts("terraform", &main_tf).unwrap();
    assert_eq!(facts.len(), 1);
    match &facts[0] {
        ProfileFact::Terraform(f) => assert_eq!(f.name, "data"),
        _ => panic!("expected terraform fact"),
    }
    fs::remove_dir_all(&root).ok();
}

#[test]
fn profile_file_routes_terraform_and_kubernetes_extensions() {
    assert_eq!(profile_file("infra/main.tf"), Some("terraform"));
    assert_eq!(profile_file("k8s/deployment.yaml"), Some("kubernetes"));
    assert_eq!(profile_file("k8s/deployment.yml"), Some("kubernetes"));
    assert_eq!(profile_file("notes.txt"), None);
}

#[test]
fn terraform_line_numbers_match_legacy_regex_semantics() {
    // Legacy `resourcePattern` is `^\s*resource\s+"..."\s+"..."` with the
    // multi-line flag; `\s` also matches newlines, so the match's *start*
    // (used for `match.index`/line computation, both here and in the
    // legacy `.slice(0, match.index)` line count) is the earliest position
    // where `^` succeeds and the leading `\s*` can absorb blank lines --
    // not necessarily the `resource` keyword's own line. A single leading
    // blank line still yields line 1 because `^` matches at offset 0 and
    // `\s*` swallows it, exactly mirroring the legacy JS regex engine.
    let tf = "\n\nresource \"aws_iam_role\" \"deployer\" {\n}\n";
    let facts = extract_terraform_facts(tf);
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].line, 1);

    // A resource declaration on its own line with no leading blank lines
    // reports that same line.
    let tf2 = "resource \"aws_iam_role\" \"deployer\" {\n}\n";
    let facts2 = extract_terraform_facts(tf2);
    assert_eq!(facts2[0].line, 1);
}
