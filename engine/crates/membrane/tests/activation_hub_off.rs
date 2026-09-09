//! A resident startup failure must not revoke explicit installed agent access.
#![cfg(windows)]

use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    os::windows::process::CommandExt,
    process::Command,
    thread,
};
use serde_json::Value;

#[test]
fn failed_hub_start_preserves_installed_client_binding() {
    let temp = tempfile::tempdir().unwrap();
    let local = temp.path().join("local");
    let product = local.join("Orthic Labs").join("Membrane");
    let version = product.join("versions").join("test");
    let current = product.join("current");
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    fs::create_dir_all(&version).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_membrane"), version.join("membrane.exe")).unwrap();
    // Present but not executable: fail deterministically at resident launch.
    fs::write(version.join("membrane-tray.exe"), b"invalid native executable").unwrap();
    let installation_id = "fixture-installation";
    let identity = product.join("state/tools/.cache/memory/installation.json");
    fs::create_dir_all(identity.parent().unwrap()).unwrap();
    fs::write(
        &identity,
        format!(
            r#"{{"schema_version":2,"installation_id":"{installation_id}","created_at":"now","startup_generation":0,"legacy_labels":[],"lineage":[],"current_service_instance_id":null,"current_claimed_at":null}}"#,
        ),
    )
    .unwrap();
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
            .env("MEMBRANE_TEST_USER_BINDINGS_ROOT", temp.path().join("user-bindings"))
            .env("MEMBRANE_TEST_INSTALLED_PORT", port.to_string())
            .env_remove("MEMBRANE_RUNTIME_ORIGIN").env_remove("WORKSPACE_ROOT")
            .env_remove("CORTEX_DB").creation_flags(0x08000000);
        command
    };
    let bindings = command("activate").arg("--bindings-only").output().unwrap();
    let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
    let health = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{{\"ok\":true,\"serviceId\":\"membrane-hub\",\"nativeOnly\":true,\"runtimeOrigin\":\"installed\",\"releaseGeneration\":\"fixture-prior-generation\",\"installationId\":\"{installation_id}\"}}"
    );
    let health_server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request).unwrap();
        stream.write_all(health.as_bytes()).unwrap();
    });
    let activation = command("activate").output().unwrap();
    health_server.join().unwrap();
    let binding = fs::read(temp.path().join(".cursor/mcp.json"));
    // Remove only this fixture's binding/PATH entry before any assertion can fail.
    let cleanup = command("deactivate").arg("--bindings-only").output().unwrap();
    assert!(cleanup.status.success(), "{}", String::from_utf8_lossy(&cleanup.stderr));
    assert!(bindings.status.success(), "{}", String::from_utf8_lossy(&bindings.stderr));
    let receipt: Value = serde_json::from_slice(&bindings.stdout).unwrap();
    assert_eq!(receipt["dryRun"], false);
    assert_eq!(receipt["service"]["port"], port);
    assert_eq!(receipt["clients"][0]["changed"], true);
    assert!(!activation.status.success(), "invalid tray unexpectedly launched");
    let activation_error = String::from_utf8_lossy(&activation.stderr);
    assert!(activation_error.contains("launch installed tray"), "{activation_error}");
    assert!(activation_error.contains("--replace"), "{activation_error}");
    let config: Value = serde_json::from_slice(&binding.expect("explicit binding survives Hub failure")).unwrap();
    assert_eq!(config["mcpServers"]["membrane"]["command"], current.join("membrane.exe").to_string_lossy().as_ref());
    assert_eq!(config["mcpServers"]["membrane"]["args"], serde_json::json!(["stdio-mcp"]));
}
