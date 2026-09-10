//! A resident startup failure must not revoke explicit installed agent access.
#![cfg(windows)]

use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{Command, Output},
    thread,
};
use serde_json::Value;

struct Fixture {
    temp: tempfile::TempDir,
    current: PathBuf,
    port: u16,
}

fn setup() -> Fixture {
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
    Fixture { temp, current, port }
}

impl Fixture {
    fn command(&self, mode: &str) -> Command {
        let mut command = Command::new(self.current.join("membrane.exe"));
        command.args([mode, "--install-root"]).arg(&self.current)
            .args(["--client", "cursor", "--timeout-ms", "1000"])
            .env("HOME", self.temp.path()).env("USERPROFILE", self.temp.path())
            .env("LOCALAPPDATA", self.temp.path().join("local"))
            .env("APPDATA", self.temp.path().join("roaming"))
            .env("MEMBRANE_TEST_USER_BINDINGS_ROOT", self.temp.path().join("user-bindings"))
            .env("MEMBRANE_TEST_INSTALLED_PORT", self.port.to_string())
            .env_remove("MEMBRANE_RUNTIME_ORIGIN").env_remove("WORKSPACE_ROOT")
            .env_remove("CORTEX_DB").creation_flags(0x08000000);
        command
    }

    fn bindings_binding(&self) -> std::io::Result<Vec<u8>> {
        fs::read(self.temp.path().join(".cursor/mcp.json"))
    }
}

fn run(command: &mut Command) -> Output {
    command.output().unwrap()
}

/// No resident answers the Membrane port at all (the common case: no prior
/// installed process, foreign or otherwise). Activation must still attempt
/// to launch the resident tray -- and, since the fixture's tray binary is
/// not a valid executable, must fail deterministically at that launch --
/// while explicit installed MCP/CLI bindings are reconciled regardless.
#[test]
fn failed_hub_start_preserves_installed_client_binding() {
    let fixture = setup();
    let bindings = run(fixture.command("activate").arg("--bindings-only"));
    let activation = run(&mut fixture.command("activate"));
    let binding = fixture.bindings_binding();
    // Remove only this fixture's binding/PATH entry before any assertion can fail.
    let cleanup = run(fixture.command("deactivate").arg("--bindings-only"));
    assert!(cleanup.status.success(), "{}", String::from_utf8_lossy(&cleanup.stderr));

    assert!(bindings.status.success(), "{}", String::from_utf8_lossy(&bindings.stderr));
    let receipt: Value = serde_json::from_slice(&bindings.stdout).unwrap();
    assert_eq!(receipt["dryRun"], false);
    assert_eq!(receipt["service"]["port"], fixture.port);
    assert_eq!(receipt["clients"][0]["changed"], true);

    assert!(!activation.status.success(), "invalid tray unexpectedly launched");
    let activation_error = String::from_utf8_lossy(&activation.stderr);
    assert!(activation_error.contains("launch installed tray"), "{activation_error}");
    assert!(activation_error.contains("--activate"), "{activation_error}");

    let config: Value = serde_json::from_slice(&binding.expect("explicit binding survives Hub failure")).unwrap();
    assert_eq!(config["mcpServers"]["membrane"]["command"], fixture.current.join("membrane.exe").to_string_lossy().as_ref());
    assert_eq!(config["mcpServers"]["membrane"]["args"], serde_json::json!(["stdio-mcp"]));
}

/// A service answers the Membrane port but cannot present the installed
/// health credential (e.g. an unrelated process, or a listener predating
/// credential provisioning). Activation must refuse to treat it as a
/// verified Membrane resident and must not attempt to launch a competing
/// tray against it -- but explicit installed MCP/CLI bindings, which
/// precede any resident-start attempt, must still be reconciled.
#[test]
fn unverified_service_on_membrane_port_is_refused_but_bindings_still_reconcile() {
    let fixture = setup();
    let bindings = run(fixture.command("activate").arg("--bindings-only"));
    let listener = TcpListener::bind(("127.0.0.1", fixture.port)).unwrap();
    let health = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"ok\":true}";
    let health_server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request).unwrap();
        let _ = stream.write_all(health.as_bytes());
    });
    let activation = run(&mut fixture.command("activate"));
    health_server.join().unwrap();
    let binding = fixture.bindings_binding();
    // Remove only this fixture's binding/PATH entry before any assertion can fail.
    let cleanup = run(fixture.command("deactivate").arg("--bindings-only"));
    assert!(cleanup.status.success(), "{}", String::from_utf8_lossy(&cleanup.stderr));

    assert!(bindings.status.success(), "{}", String::from_utf8_lossy(&bindings.stderr));

    assert!(!activation.status.success(), "unverified service on Membrane port unexpectedly accepted");
    let activation_error = String::from_utf8_lossy(&activation.stderr);
    assert!(
        activation_error.contains("refusing to activate against unverified service on Membrane port"),
        "{activation_error}"
    );
    assert!(!activation_error.contains("launch installed tray"), "{activation_error}");

    let config: Value = serde_json::from_slice(
        &binding.expect("explicit binding still reconciles ahead of a refused resident start"),
    )
    .unwrap();
    assert_eq!(config["mcpServers"]["membrane"]["command"], fixture.current.join("membrane.exe").to_string_lossy().as_ref());
    assert_eq!(config["mcpServers"]["membrane"]["args"], serde_json::json!(["stdio-mcp"]));
}
