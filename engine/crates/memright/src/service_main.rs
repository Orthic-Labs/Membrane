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

struct Runtime {
    db: PathBuf,
    token: PathBuf,
    ort: PathBuf,
    hf_home: PathBuf,
    port: u16,
}

fn prepare_runtime_identity(
    runtime: &Runtime,
) -> Result<
    (
        memright::installation_identity::InstallationIdentity,
        memright::installation_identity::StartupClaim,
    ),
    String,
> {
    let workspace_root = runtime
        .db
        .ancestors()
        .nth(4)
        .ok_or_else(|| "resolve workspace root from database path".to_string())?;
    memright::installation_identity::prepare_service_start(workspace_root)
        .map_err(|error| format!("prepare installation identity: {error}"))
}

fn runtime_from_exe(exe: &Path) -> Result<Runtime, String> {
    let bin = exe
        .parent()
        .filter(|path| path.file_name().is_some_and(|name| name == "bin"))
        .ok_or_else(|| "memright-service must run from <workspace>/tools/bin".to_string())?;
    let tools = bin
        .parent()
        .filter(|path| path.file_name().is_some_and(|name| name == "tools"))
        .ok_or_else(|| "memright-service could not locate the tools directory".to_string())?;
    let config_path = tools.join("lib/memory/runtime.json");
    let config: RuntimeConfig = serde_json::from_slice(
        &std::fs::read(&config_path)
            .map_err(|error| format!("read {}: {error}", config_path.display()))?,
    )
    .map_err(|error| format!("parse {}: {error}", config_path.display()))?;
    if config.schema_version != 1
        || config.service_id != "memright-local-v1"
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
        db: tools.join(".cache/memory/memright-engine.db"),
        token: tools.join(".cache/memory/api-token"),
        ort: bin.join(ort_name),
        hf_home: tools.join(".cache/fastembed"),
        port: config.port,
    })
}

fn main() -> Result<(), String> {
    let runtime = runtime_from_exe(
        &std::env::current_exe().map_err(|error| format!("resolve service binary: {error}"))?,
    )?;
    std::env::set_var("MEMRIGHT_DB", &runtime.db);
    std::env::set_var("MEMRIGHT_PORT", runtime.port.to_string());
    std::env::set_var("MEMRIGHT_API_TOKEN_FILE", &runtime.token);
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
    std::env::set_var("MEMRIGHT_INSTALLATION_ID", &identity.installation_id);
    std::env::set_var("MEMRIGHT_SERVICE_INSTANCE_ID", &claim.service_instance_id);
    memright::serve::run(
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
    fn deployed_service_resolves_canonical_runtime() {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("tools/bin");
        let config_dir = temp.path().join("tools/lib/memory");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("runtime.json"),
            r#"{"schemaVersion":1,"serviceId":"memright-local-v1","host":"127.0.0.1","port":47851}"#,
        )
        .unwrap();
        let runtime = runtime_from_exe(&bin.join("memright-service.exe")).unwrap();
        assert_eq!(runtime.port, 47851);
        assert_eq!(
            runtime.db,
            temp.path().join("tools/.cache/memory/memright-engine.db")
        );
        assert_eq!(
            runtime.token,
            temp.path().join("tools/.cache/memory/api-token")
        );
    }

    #[test]
    fn resident_startup_advances_identity_and_publishes_claim_before_serve() {
        let temp = tempfile::tempdir().unwrap();
        let runtime = Runtime {
            db: temp.path().join("tools/.cache/memory/memright-engine.db"),
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
