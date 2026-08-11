use std::fs;
use std::path::Path;
use orthic::manifest_validate::{validate_manifest_bytes, validate_manifest_value};

#[cfg(unix)]
use std::os::unix::fs::symlink as symlink_file;
#[cfg(windows)]
use std::os::windows::fs::symlink_file;

fn valid_base(dir: &Path) -> serde_json::Value {
    serde_json::json!({
        "schemaVersion":1,"productId":"membrane","displayName":"Membrane","productVersion":"1.0.0","hubCompatRange":">=0.1.0","installRoot": dir.to_string_lossy(),"serviceStart":[format!("{}/bin", dir.to_string_lossy())],"serviceStop":[],"statusEndpoint":{"host":"127.0.0.1","port":8080,"authHeader":"X-Token","authToken":"secret"},"icon": format!("{}/icon.png", dir.to_string_lossy())
    })
}

#[test]
fn case1_path_traversal_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let install = dir.path().join("install");
    fs::create_dir_all(&install).unwrap();
    let mut v = valid_base(&install);
    v["serviceStart"] = serde_json::json!(["../outside/bin"]);
    assert_eq!(validate_manifest_value(v).unwrap_err(), "serviceStart[0] escapes installRoot");
}

#[test]
fn case2_symlink_escape_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let install = dir.path().join("install");
    let outside = dir.path().join("outside");
    fs::create_dir_all(&install).unwrap();
    fs::create_dir_all(&outside).unwrap();
    let real = outside.join("bin");
    fs::write(&real, b"x").unwrap();
    let link = install.join("linkbin");
    symlink_file(&real, &link).unwrap();
    let mut v = valid_base(&install);
    v["serviceStart"] = serde_json::json!([link.to_string_lossy()]);
    assert_eq!(validate_manifest_value(v).unwrap_err(), "serviceStart[0] resolves outside installRoot");
}

#[test]
fn case3_absolute_outside_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let install = dir.path().join("install");
    fs::create_dir_all(&install).unwrap();
    let mut v = valid_base(&install);
    v["serviceStart"] = serde_json::json!(["/tmp/outside/bin"]);
    assert_eq!(validate_manifest_value(v).unwrap_err(), "serviceStart[0] escapes installRoot");
}

#[test]
fn case4_shell_metachars_accepted_as_literal() {
    let dir = tempfile::tempdir().unwrap();
    let install = dir.path().join("install");
    fs::create_dir_all(&install).unwrap();
    let evil = install.join("bin; rm -rf");
    fs::write(&evil, b"x").unwrap();
    let mut v = valid_base(&install);
    v["serviceStart"] = serde_json::json!([evil.to_string_lossy()]);
    // Should be accepted (literal argv, no shell interpolation), not rejected for metachars
    let res = validate_manifest_value(v);
    assert!(res.is_ok(), "shell metachars should be accepted as literal argv, got {:?}", res);
}

#[test]
fn case5_oversized_rejected() {
    let big = vec![b'a'; 1024*1024 + 1];
    assert_eq!(validate_manifest_bytes(&big).unwrap_err(), "manifest_unparseable");
    assert_eq!(validate_manifest_bytes(b"{ truncated").unwrap_err(), "manifest_unparseable");
}

#[test]
fn case6_missing_fields_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let install = dir.path().join("install");
    fs::create_dir_all(&install).unwrap();
    let mut v = valid_base(&install);
    // Remove required field
    let obj = v.as_object_mut().unwrap();
    obj.remove("productId");
    assert_eq!(validate_manifest_value(v).unwrap_err(), "manifest_schema_invalid");
}

#[test]
fn case7_wrong_types_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let install = dir.path().join("install");
    fs::create_dir_all(&install).unwrap();
    let mut v = valid_base(&install);
    v["schemaVersion"] = serde_json::json!("one");
    assert_eq!(validate_manifest_value(v).unwrap_err(), "manifest_schema_invalid");
    let mut extra = valid_base(&install);
    extra["untrustedExtension"] = serde_json::json!(true);
    assert_eq!(validate_manifest_value(extra).unwrap_err(), "manifest_schema_invalid");
}

#[test]
fn case8_icon_escape_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let install = dir.path().join("install");
    fs::create_dir_all(&install).unwrap();
    let mut v = valid_base(&install);
    v["icon"] = serde_json::json!("/etc/passwd");
    assert_eq!(validate_manifest_value(v).unwrap_err(), "icon resolves outside installRoot");
}

#[test]
fn case9_host_non_loopback_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let install = dir.path().join("install");
    fs::create_dir_all(&install).unwrap();
    let mut v = valid_base(&install);
    v["statusEndpoint"] = serde_json::json!({"host":"0.0.0.0","port":8080,"authHeader":"H","authToken":"T"});
    assert_eq!(validate_manifest_value(v).unwrap_err(), "statusEndpoint_not_loopback");
    let mut v2 = valid_base(&install);
    v2["statusEndpoint"] = serde_json::json!({"host":"192.168.1.1","port":8080,"authHeader":"H","authToken":"T"});
    assert_eq!(validate_manifest_value(v2).unwrap_err(), "statusEndpoint_not_loopback");
}
