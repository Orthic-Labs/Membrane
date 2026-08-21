use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let index =
        manifest_dir.join("../../../schemas/registry/operations/operations-index.v1.golden.json");
    println!("cargo:rerun-if-changed={}", index.display());
    let generator = manifest_dir.join("scripts/generate-cli-subcommands.mjs");
    println!("cargo:rerun-if-changed={}", generator.display());
    let generated = manifest_dir.join("src/generated_cli_subcommands.rs");
    println!("cargo:rerun-if-changed={}", generated.display());

    let output = Command::new("node")
        .arg(&generator)
        .arg("--check")
        .output()
        .expect("run deterministic Node CLI projection generator");
    assert!(
        output.status.success(),
        "generated CLI projection drifted; run `node {} --write`\n{}",
        generator.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    let generated_source = fs::read_to_string(&generated).expect("read generated CLI subcommands");
    let digest = generated_source
        .lines()
        .find_map(|line| line.strip_prefix("// operation_registry_version: "))
        .expect("generated CLI projection has registry version marker");
    assert!(
        digest.starts_with("sha256:"),
        "invalid operation registry digest"
    );
    println!("cargo:rustc-env=MEMBRANE_OPERATION_REGISTRY_VERSION={digest}");
}
