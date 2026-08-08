use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn package_crate(
    manifest: &Path,
    package: &str,
    version: &str,
    target: &Path,
    protocol_patch: Option<&Path>,
) -> PathBuf {
    let mut command = Command::new(env!("CARGO"));
    command
        .args([
            "package",
            "--allow-dirty",
            "--no-verify",
            "--offline",
            "--manifest-path",
        ])
        .arg(manifest)
        .env("CARGO_TARGET_DIR", target);
    if let Some(protocol_patch) = protocol_patch {
        command.arg("--config").arg(format!(
            "patch.crates-io.membrane-protocol.path=\"{}\"",
            toml_path(protocol_patch)
        ));
    }
    let status = command
        .status()
        .expect("cargo must package the downstream dependency");
    assert!(status.success(), "{package} package creation failed");

    let archive = target.join("package").join(format!("{package}-{version}.crate"));
    let unpacked = target.join("unpacked");
    fs::create_dir_all(&unpacked).expect("package extraction directory must be created");
    let status = Command::new("tar")
        .args(["-xzf"])
        .arg(&archive)
        .args(["-C"])
        .arg(&unpacked)
        .status()
        .expect("tar must extract the generated crate archive");
    assert!(status.success(), "{package} archive extraction failed");
    unpacked.join(format!("{package}-{version}"))
}

fn toml_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "\\\\")
}

#[test]
fn downstream_fixture_compiles_against_packaged_semver_compatible_sdk_releases() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after Unix epoch")
        .as_nanos();
    let target = std::env::temp_dir().join(format!(
        "membrane-provider-sdk-downstream-{}-{unique}",
        std::process::id()
    ));
    let protocol = package_crate(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../membrane-protocol/Cargo.toml").as_path(),
        "membrane-protocol",
        "0.1.0",
        &target.join("protocol-package"),
        None,
    );
    let sdk = package_crate(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml").as_path(),
        "membrane-provider-sdk",
        env!("CARGO_PKG_VERSION"),
        &target.join("sdk-package"),
        Some(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../membrane-protocol")),
    );
    let consumer = target.join("consumer");
    fs::create_dir_all(consumer.join("src")).expect("consumer source directory must be created");
    fs::write(
        consumer.join("Cargo.toml"),
        format!(
            "[package]\nname = \"membrane-provider-sdk-downstream-fixture\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n\n[dependencies]\nmembrane-provider-sdk = {{ version = \"0.1\", path = \"{}\" }}\nmembrane-protocol = {{ version = \"0.1\", path = \"{}\" }}\nserde_json = \"1\"\n\n[patch.crates-io]\nmembrane-protocol = {{ path = \"{}\" }}\n",
            toml_path(&sdk),
            toml_path(&protocol),
            toml_path(&protocol),
        ),
    )
    .expect("consumer manifest must be written");
    fs::write(
        consumer.join("src/lib.rs"),
        "use membrane_provider_sdk::{CapabilityV1, Provider, Result};\nuse membrane_provider_sdk::provider::{ProviderIdentityV1, ProviderReadinessV1};\nuse serde_json::{json, Value};\n\npub struct FixtureProvider;\n\nimpl Provider for FixtureProvider {\n    fn initialize(&mut self, _config: &Value) -> Result<()> { Ok(()) }\n\n    fn readiness(&self) -> ProviderReadinessV1 {\n        ProviderReadinessV1::unknown(ProviderIdentityV1 { provider_id: \"fixture\".into(), installation_id: \"fixture\".into(), service_id: \"fixture\".into(), release_generation: \"fixture\".into(), data_root_digest: \"fixture\".into() }, \"fixture\")\n    }\n\n    fn list_capabilities(&self) -> Vec<CapabilityV1> {\n        let operation = membrane_protocol::operations_slice().first().expect(\"published protocol registry contains an operation\");\n        vec![CapabilityV1 { name: operation.name.to_owned(), schema_version: operation.schema_version, error_version: operation.error_version }]\n    }\n\n    fn handle_operation(&self, _operation: &str, _request: &Value) -> Result<Value> {\n        Ok(json!({ \"kind\": \"success\", \"data\": { \"fixture\": true } }))\n    }\n}\n",
    )
    .expect("consumer source must be written");
    let status = Command::new(env!("CARGO"))
        .args(["check", "--quiet", "--offline", "--manifest-path"])
        .arg(consumer.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", target.join("consumer-target"))
        .status()
        .expect("cargo must compile the downstream fixture");
    assert!(status.success(), "packaged downstream fixture did not compile");
    let _ = std::fs::remove_dir_all(target);
}
