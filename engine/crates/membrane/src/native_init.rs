//! Explicit native `membrane init` enrollment.
//!
//! This is deliberately separate from activation: activation may reconcile client
//! bindings, while init is the operator-owned registry mutation.

use serde_json::{json, Map, Value};
use std::{fs, path::{Path, PathBuf}, time::{SystemTime, UNIX_EPOCH}};

#[derive(Debug, Clone)]
pub struct InitOptions {
    pub root: PathBuf,
    pub repository_id: String,
    pub scope_id: String,
    pub virtual_id: Option<String>,
    pub tenant_id: Option<String>,
    pub parents: Vec<String>,
    pub native: Option<String>,
    pub config: Option<PathBuf>,
    pub dry_run: bool,
}

pub fn init(options: &InitOptions) -> Result<Value, String> {
    let path = membrane_runtime::authorization::default_registry_path()
        .ok_or_else(|| "registry path is unresolvable".to_string())?;
    init_at(options, &path, None)
}

fn init_at(options: &InitOptions, path: &Path, injected_binding: Option<Value>) -> Result<Value, String> {
    let root = fs::canonicalize(&options.root)
        .map_err(|e| format!("canonicalize repository root {}: {e}", options.root.display()))?;
    if !root.is_dir() { return Err("project root must be a directory".into()); }
    if options.repository_id.trim().is_empty() || options.scope_id.trim().is_empty() {
        return Err("repository & scope are required".into());
    }
    let descriptor = if let Some(id) = &options.virtual_id {
        if id.trim().is_empty() { return Err("--virtual-id is invalid".into()); }
        let tenant = options.tenant_id.as_deref().filter(|v| !v.trim().is_empty())
            .ok_or_else(|| "--tenant-id is required with --virtual-id".to_string())?;
        json!({"kind":"virtual","id":id,"tenant_id":tenant,"parents":options.parents,"inherit_global":false})
    } else {
        if options.tenant_id.is_some() || !options.parents.is_empty() { return Err("--tenant-id/--parent require --virtual-id".into()); }
        json!({"kind":"filesystem","path":options.scope_id})
    };
    validate_descriptor(&descriptor)?;
    let provider = {
        let mut value = Map::from_iter([(String::from("transport"), json!("loopback"))]);
        if let Some(path) = &options.config { value.insert("config".into(), json!({"path":path})); }
        if let Some(name) = &options.native { value.insert("native".into(), json!({"name":name})); }
        if !options.dry_run {
            let binding = match injected_binding.clone() {
                Some(binding) => binding,
                None => membrane_runtime::service::installed_binding_projection()?,
            };
            value.insert("installation_binding".into(), binding);
        }
        Value::Object(value)
    };
    let receipt_installation_binding = provider.get("installation_binding").cloned();
    let _lock = (!options.dry_run).then(|| RegistryLock::acquire(&path)).transpose()?;
    let mut registry = match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice::<Value>(&bytes).map_err(|e| format!("registry is malformed: {e}"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({"schema_version":2,"bindings":{}}),
        Err(e) => return Err(format!("read registry {}: {e}", path.display())),
    };
    normalize_registry(&mut registry)?;
    let bindings = registry.get_mut("bindings").and_then(Value::as_object_mut)
        .ok_or_else(|| "registry bindings must be an object".to_string())?;
    let key = root.to_string_lossy().into_owned();
    #[cfg(windows)]
    let key = if let Some(unc) = key.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        key.strip_prefix(r"\\?\").unwrap_or(&key).to_string()
    };
    // JS realpath & Rust canonicalize spell Windows device paths differently.
    // Resolve existing keys before replacing so enrollment cannot fork authority
    // or lose token metadata solely because of spelling/case.
    let prior_keys = bindings.keys().filter(|candidate| {
        *candidate == &key || fs::canonicalize(candidate).ok().as_ref() == Some(&root)
    }).cloned().collect::<Vec<_>>();
    if prior_keys.len() > 1 { return Err("registry contains duplicate canonical repository roots".into()); }
    let prior = prior_keys.first().and_then(|prior_key| bindings.remove(prior_key));
    let prior_object = prior.as_ref().and_then(Value::as_object);
    let mut object = Map::new();
    object.insert("repository_id".into(), json!(options.repository_id));
    object.insert("scope_id".into(), json!(options.scope_id));
    object.insert("scope_descriptor".into(), descriptor);
    object.insert("provider_config".into(), provider);
    object.insert("grant_policy".into(), json!({"level":"read-only"}));
    for field in ["token_grant", "token_audit"] {
        if let Some(value) = prior_object.and_then(|prior| prior.get(field)) { object.insert(field.into(), value.clone()); }
    }
    bindings.insert(key.clone(), Value::Object(object));
    let installation = json!({"precedence":"native_then_config","selected":if options.native.is_some(){"native"}else if options.config.is_some(){"config"}else{"loopback"},"native":options.native.as_ref().map(|name| json!({"name":name})),"config":options.config.as_ref().map(|path| json!({"path":path}))});
    let mut receipt = json!({"action":"enroll","root":key,"repository_id":options.repository_id,"scope_id":options.scope_id,"registry":path,"installation":installation,"dry_run":options.dry_run});
    if let Some(binding) = receipt_installation_binding {
        receipt["installation_binding"] = binding.clone();
    }
    if !options.dry_run { write_atomic(&path, &registry)?; }
    Ok(receipt)
}

fn validate_descriptor(value: &Value) -> Result<(), String> {
    let object = value.as_object().ok_or("scope_descriptor must be an object")?;
    match object.get("kind").and_then(Value::as_str) {
        Some("filesystem") if object.get("path").and_then(Value::as_str).is_some() && object.keys().all(|k| k == "kind" || k == "path") => Ok(()),
        Some("virtual") => {
            if object.get("id").and_then(Value::as_str).map_or(true, |v| v.trim().is_empty()) || object.get("tenant_id").and_then(Value::as_str).map_or(true, |v| v.trim().is_empty()) { return Err("virtual scope_descriptor identity is invalid".into()); }
            let parents = object.get("parents").and_then(Value::as_array).ok_or("virtual scope_descriptor parents are invalid")?;
            if parents.len() > 7 { return Err("virtual scope_descriptor parents are invalid".into()); }
            let mut seen = std::collections::HashSet::new();
            if parents.iter().any(|v| v.as_str().map_or(true, |p| p.trim().is_empty() || p.starts_with("virtual:") || !seen.insert(p))) { return Err("virtual scope_descriptor parents are invalid".into()); }
            if object.get("inherit_global") != Some(&Value::Bool(false)) || object.keys().any(|k| !["kind","id","tenant_id","parents","inherit_global"].contains(&k.as_str())) { return Err("virtual scope_descriptor global inheritance is invalid".into()); }
            Ok(())
        }
        _ => Err("scope_descriptor kind is invalid".into()),
    }
}

fn normalize_registry(registry: &mut Value) -> Result<(), String> {
    let version = registry.get("schema_version").and_then(Value::as_u64).ok_or("schema_version must be 1 or 2")?;
    if version != 1 && version != 2 { return Err("schema_version must be 1 or 2".into()); }
    if version == 1 {
        registry["schema_version"] = json!(2);
        if let Some(bindings) = registry.get_mut("bindings").and_then(Value::as_object_mut) {
            for (root, binding) in bindings.iter_mut() {
                if !binding.is_object() { return Err(format!("binding {root} is malformed")); }
                if binding.get("scope_descriptor").is_none() {
                    let scope = binding.get("scope_id").and_then(Value::as_str).unwrap_or(root);
                    binding["scope_descriptor"] = json!({"kind":"filesystem","path":scope});
                }
            }
        }
    }
    if !registry.get("bindings").is_some_and(Value::is_object) { return Err("registry bindings must be an object".into()); }
    for (root, binding) in registry["bindings"].as_object().unwrap() {
        let object = binding.as_object().ok_or_else(|| format!("binding {root} is malformed"))?;
        if object.get("repository_id").and_then(Value::as_str).map_or(true, |v| v.trim().is_empty()) || object.get("scope_id").and_then(Value::as_str).map_or(true, |v| v.trim().is_empty()) { return Err(format!("binding {root} identity is invalid")); }
        if let Some(descriptor) = object.get("scope_descriptor") { validate_descriptor(descriptor)?; }
    }
    Ok(())
}

fn write_atomic(path: &Path, value: &Value) -> Result<(), String> {
    let parent = path.parent().ok_or("registry has no parent")?;
    fs::create_dir_all(parent).map_err(|e| format!("create registry directory: {e}"))?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let staged = parent.join(format!(".project-registry.{nonce}.tmp"));
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| format!("serialize registry: {e}"))?;
    if let Err(e) = fs::write(&staged, bytes) {
        return Err(format!("stage registry: {e}"));
    }
    let result = replace_file(&staged, path);
    if result.is_err() { let _ = fs::remove_file(&staged); }
    result
}

struct RegistryLock { path: PathBuf }
impl RegistryLock {
    fn acquire(registry: &Path) -> Result<Self, String> {
        let path = registry.with_extension("json.lock");
        fs::create_dir_all(registry.parent().ok_or("registry has no parent")?)
            .map_err(|e| format!("create registry directory: {e}"))?;
        fs::OpenOptions::new().write(true).create_new(true).open(&path)
            .map_err(|e| format!("registry is busy: {e}"))?;
        Ok(Self { path })
    }
}
impl Drop for RegistryLock { fn drop(&mut self) { let _ = fs::remove_file(&self.path); } }

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH};
    let s: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let d: Vec<u16> = destination.as_os_str().encode_wide().chain(Some(0)).collect();
    (unsafe { MoveFileExW(s.as_ptr(), d.as_ptr(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) } != 0)
        .then_some(()).ok_or_else(|| format!("replace registry: {}", std::io::Error::last_os_error()))
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> Result<(), String> { fs::rename(source, destination).map_err(|e| format!("replace registry: {e}")) }

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[test]
    fn schema_one_normalizes_without_dropping_token_audit() {
        let mut v = json!({"schema_version":1,"bindings":{"C:\\repo":{"repository_id":"r","scope_id":"s","token_grant":{"generation":2},"token_audit":[{"reason":"rotation"}]}}});
        normalize_registry(&mut v).unwrap();
        assert_eq!(v["schema_version"], 2);
        let key = "C:\\repo";
        assert_eq!(v["bindings"][key]["token_grant"]["generation"], 2);
        assert!(v["bindings"][key]["scope_descriptor"].is_object());
    }

    #[test]
    fn malformed_schema_one_binding_returns_error() {
        let mut registry = json!({"schema_version":1,"bindings":{"C:\\repo":42}});
        assert!(normalize_registry(&mut registry).unwrap_err().contains("malformed"));
    }

    #[test]
    fn enrollment_preserves_tokens_on_second_update_and_dry_run_writes_nothing() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo"); fs::create_dir(&root).unwrap();
        let registry = temp.path().join("project-registry.json");
        let options = |repository_id: &str, dry_run| InitOptions {
            root: root.clone(), repository_id: repository_id.into(), scope_id: "scope".into(),
            virtual_id: None, tenant_id: None, parents: Vec::new(), native: Some("membrane".into()), config: None, dry_run,
        };
        let fixture_binding = Some(json!({"workspaceRoot":"fixture","host":"127.0.0.1","port":47851,"endpoint":"http://127.0.0.1:47851","db":"fixture.db","tokenPath":"fixture.token","installationId":"i","serviceInstanceId":"s","stableCurrent":"fixture/current"}));
        assert_eq!(init_at(&options("one", false), &registry, fixture_binding.clone()).unwrap()["repository_id"], "one");
        let mut stored: Value = serde_json::from_slice(&fs::read(&registry).unwrap()).unwrap();
        let bindings = stored["bindings"].as_object_mut().unwrap();
        let old_key = bindings.keys().next().unwrap().clone();
        let binding = bindings.remove(&old_key).unwrap();
        bindings.insert(fs::canonicalize(&root).unwrap().to_string_lossy().into_owned(), binding);
        stored["bindings"].as_object_mut().unwrap().values_mut().next().unwrap().as_object_mut().unwrap().insert("token_grant".into(), json!({"generation":3}));
        fs::write(&registry, serde_json::to_vec(&stored).unwrap()).unwrap();
        init_at(&options("two", false), &registry, fixture_binding.clone()).unwrap();
        let before = fs::read(&registry).unwrap();
        init_at(&options("three", true), &registry, fixture_binding).unwrap();
        assert_eq!(fs::read(&registry).unwrap(), before);
        let final_value: Value = serde_json::from_slice(&before).unwrap();
        assert_eq!(final_value["bindings"].as_object().unwrap().len(), 1);
        let binding = final_value["bindings"].as_object().unwrap().values().next().unwrap();
        assert_eq!(binding["repository_id"], "two");
        assert_eq!(binding["token_grant"]["generation"], 3);
    }
}
