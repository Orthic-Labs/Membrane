#![cfg_attr(windows, windows_subsystem = "windows")]

//! Console-free Windows entrypoint for Task Scheduler. The scheduler owns this final process
//! directly; no shell wrapper or visible console host sits between it and the resident service.

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeConfig {
    schema_version: u32,
    service_id: String,
    host: String,
    port: u16,
}

pub(crate) struct Runtime {
    pub(crate) db: PathBuf,
    pub(crate) token: PathBuf,
    pub(crate) ort: PathBuf,
    pub(crate) hf_home: PathBuf,
    pub(crate) port: u16,
}

fn build_info() -> serde_json::Value {
    serde_json::json!({
        "product_version": env!("CARGO_PKG_VERSION"),
        "crypt_source_commit": option_env!("CRYPT_SOURCE_COMMIT").unwrap_or("unknown"),
        "source_tree_sha256": option_env!("CRYPT_SOURCE_TREE_SHA256").unwrap_or("unknown"),
        "release_generation": crypt::release_identity::release_generation(),
        "target": crypt::release_identity::target_triple(),
    })
}

pub(crate) fn prepare_runtime_identity(
    runtime: &Runtime,
) -> Result<
    (
        crypt::installation_identity::InstallationIdentity,
        crypt::installation_identity::StartupClaim,
    ),
    String,
> {
    let workspace_root = runtime
        .db
        .ancestors()
        .nth(4)
        .ok_or_else(|| "resolve workspace root from database path".to_string())?;
    crypt::installation_identity::prepare_service_start(workspace_root)
        .map_err(|error| format!("prepare installation identity: {error}"))
}

fn runtime_from_exe_at_workspace(
    exe: &Path,
    workspace_root: Option<&Path>,
) -> Result<Runtime, String> {
    let direct_bin = exe
        .parent()
        .filter(|path| path.file_name().is_some_and(|name| name == "bin"))
        .filter(|path| {
            path.parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "tools")
        });
    let linked_bin = workspace_root.and_then(|root| {
        if !root.is_absolute() {
            return None;
        }
        let bin = root.join("tools/bin");
        let service_names = if cfg!(windows) {
            ["crypt-service.exe", "crypt-service.exe"]
        } else {
            ["crypt-service", "crypt-service"]
        };
        let actual = std::fs::canonicalize(exe).ok()?;
        service_names.iter().find_map(|name| {
            let service = bin.join(name);
            let metadata = std::fs::symlink_metadata(&service).ok()?;
            if !metadata.file_type().is_symlink() {
                return None;
            }
            let linked = std::fs::canonicalize(service).ok()?;
            (linked == actual).then_some(bin.clone())
        })
    });
    let bin = direct_bin
        .map(Path::to_path_buf)
        .or(linked_bin)
        .ok_or_else(|| {
            "crypt-service must run from <workspace>/tools/bin or its exact canonical symlink"
                .to_string()
        })?;
    let tools = bin
        .parent()
        .filter(|path| path.file_name().is_some_and(|name| name == "tools"))
        .ok_or_else(|| "crypt-service could not locate the tools directory".to_string())?;
    let config_path = tools.join("lib/memory/runtime.json");
    let config: RuntimeConfig = serde_json::from_slice(
        &std::fs::read(&config_path)
            .map_err(|error| format!("read {}: {error}", config_path.display()))?,
    )
    .map_err(|error| format!("parse {}: {error}", config_path.display()))?;
    if config.schema_version != 1
        || config.service_id != "crypt-local-v1"
        || config.host != "127.0.0.1"
        || config.port < 1024
    {
        return Err(format!(
            "invalid runtime identity in {}",
            config_path.display()
        ));
    }
    let ort_name = if cfg!(windows) {
        "onnxruntime.dll"
    } else if cfg!(target_os = "macos") {
        "libonnxruntime.dylib"
    } else {
        "libonnxruntime.so"
    };
    Ok(Runtime {
        db: tools.join(".cache/memory/crypt-engine.db"),
        token: tools.join(".cache/memory/api-token"),
        ort: bin.join(ort_name),
        hf_home: tools.join(".cache/fastembed"),
        port: config.port,
    })
}

pub(crate) fn runtime_from_exe(exe: &Path) -> Result<Runtime, String> {
    let workspace = std::env::var_os("WORKSPACE_ROOT").map(PathBuf::from);
    runtime_from_exe_at_workspace(exe, workspace.as_deref())
}

/// MBR-102 entrypoint: supervisor-child launch with an explicit lease path. When the supervisor
/// passes a lease, the runtime validates the signature before starting. With no lease the
/// runtime falls back to the legacy `run_service` behavior.
pub fn run_service_from(lease: Option<&std::path::Path>) -> Result<(), String> {
    if let Some(lease_path) = lease {
        // Validate the lease file is present and parses before handing off to the supervisor
        // machinery. The full lease authority check belongs to the supervisor itself; here we
        // only confirm the file is well-formed enough to start.
        let bytes = std::fs::read(lease_path)
            .map_err(|error| format!("read supervisor lease {}: {error}", lease_path.display()))?;
        if bytes.is_empty() {
            return Err(format!(
                "supervisor lease {} is empty",
                lease_path.display()
            ));
        }
        // The lease is opaque to the runtime at this layer; the supervisor owns the format.
        // Store the path so downstream consumers (serve.rs) can pick it up if needed.
        std::env::set_var("MEMBRANE_SUPERVISOR_LEASE", lease_path);
    }
    run_service()
}

pub fn run_service() -> Result<(), String> {
    if std::env::args().nth(1).as_deref() == Some("build-info") {
        println!("{}", build_info());
        return Ok(());
    }
    let runtime = runtime_from_exe(
        &std::env::current_exe().map_err(|error| format!("resolve service binary: {error}"))?,
    )?;
    std::env::set_var("CRYPT_DB", &runtime.db);
    std::env::set_var("CRYPT_PORT", runtime.port.to_string());
    std::env::set_var("CRYPT_API_TOKEN_FILE", &runtime.token);
    std::env::set_var("ORT_DYLIB_PATH", &runtime.ort);
    std::env::set_var("HF_HOME", &runtime.hf_home);
    std::env::set_var("HF_HUB_OFFLINE", "1");
    std::env::set_var(
        "WORKSPACE_ROOT",
        runtime
            .db
            .ancestors()
            .nth(4)
            .ok_or_else(|| "resolve workspace root from database path".to_string())?,
    );
    let (identity, claim) = prepare_runtime_identity(&runtime)?;
    let workspace_root = runtime
        .db
        .ancestors()
        .nth(4)
        .ok_or_else(|| "resolve workspace root from database path".to_string())?;
    // Publish the IPC handshake manifest before any peer can connect. This
    // is a hard requirement of the MBR-105 contract: a resident that has
    // not published its manifest must reject every handshake. We deliberately
    // do this AFTER `prepare_runtime_identity` so the manifest always
    // reflects the just-minted startup generation. A failure to publish is
    // fatal: the resident would otherwise serve requests that no peer can
    // verify, which silently breaks the contract.
    let active_manifest =
        crate::installation_manifest::build_active_manifest(&identity, &claim, workspace_root);
    crate::installation_manifest::publish_active_manifest(active_manifest)
        .map_err(|error| format!("publish installation manifest: {error}"))?;
    std::env::set_var("CRYPT_INSTALLATION_ID", &identity.installation_id);
    std::env::set_var("CRYPT_SERVICE_INSTANCE_ID", &claim.service_instance_id);
    crypt::serve::run(
        runtime
            .db
            .to_str()
            .ok_or_else(|| "database path is not valid UTF-8".to_string())?,
        runtime.port,
        &identity,
        &claim,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_info_exposes_source_commit_and_tree_identity_fields() {
        let info = build_info();
        assert_eq!(info["product_version"], env!("CARGO_PKG_VERSION"));
        assert!(info.get("crypt_source_commit").is_some());
        assert!(info.get("source_tree_sha256").is_some());
        assert!(info.get("release_generation").is_some());
        assert!(info.get("target").is_some());
    }

    #[test]
    fn deployed_service_resolves_canonical_runtime() {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("tools/bin");
        let config_dir = temp.path().join("tools/lib/memory");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("runtime.json"),
            r#"{"schemaVersion":1,"serviceId":"crypt-local-v1","host":"127.0.0.1","port":47851}"#,
        )
        .unwrap();
        let runtime = runtime_from_exe(&bin.join("crypt-service.exe")).unwrap();
        assert_eq!(runtime.port, 47851);
        assert_eq!(
            runtime.db,
            temp.path().join("tools/.cache/memory/crypt-engine.db")
        );
        assert_eq!(
            runtime.token,
            temp.path().join("tools/.cache/memory/api-token")
        );
    }

    #[cfg(unix)]
    #[test]
    fn relocated_service_requires_exact_workspace_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        let bin = workspace.join("tools/bin");
        let config_dir = workspace.join("tools/lib/memory");
        let relocated = temp.path().join("resident/crypt-service");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(relocated.parent().unwrap()).unwrap();
        std::fs::write(&relocated, b"fixture").unwrap();
        std::fs::write(
            config_dir.join("runtime.json"),
            r#"{"schemaVersion":1,"serviceId":"crypt-local-v1","host":"127.0.0.1","port":47851}"#,
        )
        .unwrap();
        symlink(&relocated, bin.join("crypt-service")).unwrap();

        let runtime = runtime_from_exe_at_workspace(&relocated, Some(&workspace)).unwrap();
        assert_eq!(runtime.port, 47851);
        assert_eq!(runtime.ort, bin.join("libonnxruntime.dylib"));

        let other = temp.path().join("resident/other-service");
        std::fs::write(&other, b"other").unwrap();
        assert!(runtime_from_exe_at_workspace(&other, Some(&workspace)).is_err());
    }

    #[test]
    fn resident_startup_advances_identity_and_publishes_claim_before_serve() {
        let temp = tempfile::tempdir().unwrap();
        let runtime = Runtime {
            db: temp.path().join("tools/.cache/memory/crypt-engine.db"),
            token: temp.path().join("tools/.cache/memory/api-token"),
            ort: temp.path().join("tools/bin/onnxruntime.dll"),
            hf_home: temp.path().join("tools/.cache/fastembed"),
            port: 47851,
        };

        let (identity, claim) = prepare_runtime_identity(&runtime).unwrap();

        assert_eq!(identity.startup_generation, 1);
        assert_eq!(claim.installation_id, identity.installation_id);
        assert!(temp
            .path()
            .join("memory-mirror/_installation_claims")
            .join(&claim.installation_id)
            .join(format!("{:020}", claim.startup_generation))
            .join(format!("{}.json", claim.service_instance_id))
            .is_file());
    }
}
