use serde_json::Value;
use std::{env, fs, path::PathBuf};

// The identity must reach the compiler as SOURCE, not as a process env var.
// `cargo:rustc-env` + `option_env!` looks equivalent, but sccache does not hash
// arbitrary process env, so a cached object compiled before the identity
// existed is reused verbatim and the binary ships "sha256:unknown" while the
// build log shows the real digest. A generated file lands in the dep-info
// sccache does hash, so a changed identity forces a real recompile.
fn emit(commit: &str, tree: &str) {
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("out directory"))
        .join("release_identity_generated.rs");
    fs::write(
        &out,
        format!(
            "pub const SOURCE_COMMIT: Option<&str> = {};
pub const SOURCE_TREE_SHA256: Option<&str> = {};
",
            match commit { "" => "None".to_string(), value => format!("Some({value:?})") },
            match tree { "" => "None".to_string(), value => format!("Some({value:?})") },
        ),
    )
    .expect("write generated release identity");
}

fn lowercase_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let identity = manifest.join("../../../apps/membrane-hub/dist/release-identity.json");
    println!("cargo:rerun-if-changed={}", identity.display());

    let Ok(bytes) = fs::read(&identity) else {
        // Without this file the binary embeds "unknown" as its release
        // generation, and every consumer that checks release identity is
        // silently degraded: /health, the activation receipt and the shipped
        // manifest all reported sha256:unknown on the 0.1.24 builds because
        // this returned quietly. Building without an identity stays allowed
        // (a plain `cargo build` must still work), but it is no longer silent.
        println!(
            "cargo:warning=release identity missing at {}; MEMBRANE_SOURCE_TREE_SHA256 is unset and this build will report releaseGeneration sha256:unknown. Run `pnpm --dir apps/membrane-hub run release:identity` before compiling a release.",
            identity.display()
        );
        emit("", "");
        return;
    };
    let value: Value = serde_json::from_slice(&bytes).expect("release identity must be valid JSON");
    let commit = value
        .get("commit")
        .and_then(Value::as_str)
        .filter(|value| lowercase_hex(value, 40))
        .expect("release identity commit must be 40 lowercase hex characters");
    let tree = value
        .get("sourceTreeSha256")
        .and_then(Value::as_str)
        .filter(|value| lowercase_hex(value, 64))
        .expect("release identity source tree must be 64 lowercase hex characters");
    let generation = value
        .get("releaseGeneration")
        .and_then(Value::as_str)
        .expect("release identity generation is required");
    assert_eq!(
        generation,
        format!("sha256:{tree}"),
        "release identity generation mismatch"
    );
    println!("cargo:rustc-env=MEMBRANE_SOURCE_COMMIT={commit}");
    println!("cargo:rustc-env=MEMBRANE_SOURCE_TREE_SHA256={tree}");
    emit(commit, tree);
}
