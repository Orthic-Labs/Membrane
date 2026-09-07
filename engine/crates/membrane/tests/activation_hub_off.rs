//! A resident startup failure must not revoke explicit installed agent access.
#![cfg(windows)]

use std::{fs, os::windows::process::CommandExt, process::Command};
use serde_json::Value;

#[test]
fn failed_hub_start_preserves_installed_client_binding() {
    let temp = tempfile::tempdir().unwrap();
    let local = temp.path().join("local");
    let product = local.join("Orthic Labs").join("Membrane");
    let version = product.join("versions").join("test");
    let current = product.join("current");
    fs::create_dir_all(&version).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_membrane"), version.join("membrane.exe")).unwrap();
    // Present but not executable: fail deterministically at resident launch.
    fs::write(version.join("membrane-tray.exe"), b"invalid native executable").unwrap();
    fs::create_dir_all(version.join("runtime/blueprint/lib")).unwrap();
    fs::create_dir_all(version.join("mcp/hooks")).unwrap();
    // Bindings reference these installed assets; no hook is executed here.
    fs::write(version.join("runtime/blueprint/lib/node.exe"), b"fixture").unwrap();
    fs::write(version.join("mcp/hooks/membrane-hook-entrypoint.mjs"), b"// fixture").unwrap();
    let linked = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command",
            "New-Item -ItemType Junction -Path $env:MEMBRANE_TEST_LINK -Target $env:MEMBRANE_TEST_TARGET -ErrorAction Stop | Out-Null"])
        .env("MEMBRANE_TEST_LINK", &current).env("MEMBRANE_TEST_TARGET", &version)
        .creation_flags(0x08000000).output().unwrap();
    assert!(linked.status.success(), "{}", String::from_utf8_lossy(&linked.stderr));
    let command = |mode: &str| {
        let mut command = Command::new(current.join("membrane.exe"));
        command.args([mode, "--install-root"]).arg(&current)
            .args(["--client", "cursor", "--timeout-ms", "1000"])
            .env("HOME", temp.path()).env("USERPROFILE", temp.path())
            .env("LOCALAPPDATA", &local).env("APPDATA", temp.path().join("roaming"))
            .env_remove("MEMBRANE_RUNTIME_ORIGIN").env_remove("WORKSPACE_ROOT")
            .env_remove("CORTEX_DB").creation_flags(0x08000000);
        command
    };
    let bindings = command("activate").arg("--bindings-only").output().unwrap();
    let activation = command("activate").output().unwrap();
    let binding = fs::read(temp.path().join(".cursor/mcp.json"));
    // Remove only this fixture's binding/PATH entry before any assertion can fail.
    let cleanup = command("deactivate").output().unwrap();
    assert!(cleanup.status.success(), "{}", String::from_utf8_lossy(&cleanup.stderr));
    assert!(bindings.status.success(), "{}", String::from_utf8_lossy(&bindings.stderr));
    let receipt: Value = serde_json::from_slice(&bindings.stdout).unwrap();
    assert_eq!(receipt["dryRun"], false);
    assert_eq!(receipt["clients"][0]["changed"], true);
    assert!(!activation.status.success(), "invalid tray unexpectedly launched");
    let config: Value = serde_json::from_slice(&binding.expect("explicit binding survives Hub failure")).unwrap();
    assert_eq!(config["mcpServers"]["membrane"]["command"], current.join("membrane.exe").to_string_lossy().as_ref());
    assert_eq!(config["mcpServers"]["membrane"]["args"], serde_json::json!(["stdio-mcp"]));
}
