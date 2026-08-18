use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let index = manifest_dir.join("../../../schemas/registry/operations/operations-index.v1.golden.json");
    println!("cargo:rerun-if-changed={}", index.display());

    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&index).expect("read canonical operation index"))
            .expect("parse canonical operation index");
    let ids: Vec<&str> = value["operations"]
        .as_array()
        .expect("canonical operation index has operations")
        .iter()
        .map(|entry| entry["name"].as_str().expect("operation has name"))
        .collect();
    let generated = manifest_dir.join("src/generated_cli_subcommands.rs");
    println!("cargo:rerun-if-changed={}", generated.display());

    let canonical = serde_json::to_string(&ids).expect("serialize operation ids");
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    let digest = format!("sha256:{:x}", hasher.finalize());
    if env::var_os("MEMBRANE_REGENERATE_CLI").is_none() {
        let generated_source =
            fs::read_to_string(&generated).expect("read generated CLI subcommands");
        let marker = format!("// operation_registry_version: {digest}");
        assert!(
            generated_source.lines().any(|line| line == marker),
            "operation registry changed; regenerate CLI parity with MEMBRANE_REGENERATE_CLI=1 cargo run -p membrane --bin cli-parity -- --generate"
        );
    }
    println!("cargo:rustc-env=MEMBRANE_OPERATION_REGISTRY_VERSION={digest}");
}
