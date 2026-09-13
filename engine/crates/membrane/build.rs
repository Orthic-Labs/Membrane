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
    // Transport-only client embeds its build target as packaging metadata
    // without runtime dispatch; every binary in this package may read it.
    println!(
        "cargo:rustc-env=TARGET_TRIPLE={}",
        env::var("TARGET").unwrap_or_else(|_| "unknown".to_owned())
    );
    emit_release_identity(&manifest_dir);
}

// Release identity for binaries that must not link the runtime
// (transport-only `membrane-client`): same source file
// (apps/membrane-hub/dist/release-identity.json), same sccache-safe
// generated-source mechanism as membrane-runtime's build script, so the
// client reports the exact source generation it was compiled from without
// constructing runtime state to ask.
fn emit_release_identity(manifest_dir: &PathBuf) {
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("out directory"))
        .join("client_release_identity_generated.rs");
    let identity = manifest_dir.join("../../../apps/membrane-hub/dist/release-identity.json");
    println!("cargo:rerun-if-changed={}", identity.display());
    let emit = |commit: &str, tree: &str| {
        fs::write(
            &out,
            format!(
                "pub const SOURCE_COMMIT: Option<&str> = {};\npub const SOURCE_TREE_SHA256: Option<&str> = {};\n",
                match commit { "" => "None".to_string(), value => format!("Some({value:?})") },
                match tree { "" => "None".to_string(), value => format!("Some({value:?})") },
            ),
        )
        .expect("write generated client release identity");
    };
    let Ok(bytes) = fs::read(&identity) else {
        println!(
            "cargo:warning=release identity missing at {}; transport client will report releaseGeneration sha256:unknown. Run `pnpm --dir apps/membrane-hub run release:identity` before compiling a release.",
            identity.display()
        );
        emit("", "");
        return;
    };
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).expect("release identity must be valid JSON");
    let commit = value.get("commit").and_then(serde_json::Value::as_str).unwrap_or("");
    let tree = value
        .get("sourceTreeSha256")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    emit(commit, tree);
}
