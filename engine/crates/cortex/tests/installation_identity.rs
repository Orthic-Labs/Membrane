use membrane_runtime::installation_identity::{
    assert_installation_not_quarantined, load_or_create_installation, rotate_installation,
    start_and_publish, InstallationIdentity,
};
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

fn valid_uuid4(value: &str) -> bool {
    let bytes = value.as_bytes();
    value.len() == 36
        && [8, 13, 18, 23].iter().all(|index| bytes[*index] == b'-')
        && bytes[14] == b'4'
        && matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| [8, 13, 18, 23].contains(&index) || byte.is_ascii_hexdigit())
}

fn read_identity(path: &Path) -> InstallationIdentity {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn creates_schema_v2_opaque_identity_and_reuses_it() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("cache/installation.json");

    let first = load_or_create_installation(&path, &["old-host".to_string()]).unwrap();
    let second = load_or_create_installation(&path, &["ignored-on-reload".to_string()]).unwrap();

    assert_eq!(first, second);
    assert_eq!(first.schema_version, 2);
    assert!(valid_uuid4(&first.installation_id));
    assert_eq!(first.startup_generation, 0);
    assert_eq!(first.legacy_labels, vec!["old-host"]);
    assert!(first.lineage.is_empty());
    assert_eq!(first.current_service_instance_id, None);
    assert_eq!(read_identity(&path), first);
}

#[test]
fn concurrent_starts_get_unique_monotonic_generations_and_append_only_claims() {
    let temp = tempfile::tempdir().unwrap();
    let identity = Arc::new(temp.path().join("cache/installation.json"));
    let mirror = Arc::new(temp.path().join("memory-mirror"));
    let workers = 8usize;

    let handles: Vec<_> = (0..workers)
        .map(|_| {
            let identity = Arc::clone(&identity);
            let mirror = Arc::clone(&mirror);
            std::thread::spawn(move || start_and_publish(&identity, &mirror, None, &[]).unwrap().1)
        })
        .collect();
    let claims: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();

    let generations: BTreeSet<_> = claims
        .iter()
        .map(|claim| claim.startup_generation)
        .collect();
    assert_eq!(generations, (1..=workers as u64).collect());
    assert_eq!(
        claims
            .iter()
            .map(|claim| claim.service_instance_id.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        workers
    );
    for claim in &claims {
        let path = mirror
            .join("_installation_claims")
            .join(&claim.installation_id)
            .join(format!("{:020}", claim.startup_generation))
            .join(format!("{}.json", claim.service_instance_id));
        assert!(path.is_file(), "missing {}", path.display());
    }
    assert_eq!(read_identity(&identity).startup_generation, workers as u64);
}

#[test]
fn cloned_identity_is_quarantined_until_explicit_rotation() {
    let temp = tempfile::tempdir().unwrap();
    let original = temp.path().join("machine-a/installation.json");
    let clone = temp.path().join("machine-b/installation.json");
    let mirror = temp.path().join("memory-mirror");
    let identity = load_or_create_installation(&original, &[]).unwrap();
    std::fs::create_dir_all(clone.parent().unwrap()).unwrap();
    std::fs::copy(&original, &clone).unwrap();

    start_and_publish(
        &original,
        &mirror,
        Some("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"),
        &[],
    )
    .unwrap();
    let collision = start_and_publish(
        &clone,
        &mirror,
        Some("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"),
        &[],
    )
    .unwrap_err();
    assert!(collision.to_string().contains("quarantines installation"));
    assert!(assert_installation_not_quarantined(&mirror, &identity.installation_id).is_err());

    let generation_before_retry = read_identity(&clone).startup_generation;
    assert!(start_and_publish(&clone, &mirror, None, &[]).is_err());
    assert_eq!(
        read_identity(&clone).startup_generation,
        generation_before_retry
    );

    let rotated = rotate_installation(&clone, "clone").unwrap();
    assert_ne!(rotated.installation_id, identity.installation_id);
    assert_eq!(rotated.startup_generation, 0);
    assert_eq!(rotated.lineage, vec![identity.installation_id.clone()]);
    assert!(clone
        .parent()
        .unwrap()
        .join("installation-archive")
        .join(format!("{}.json", identity.installation_id))
        .is_file());
    start_and_publish(&clone, &mirror, None, &[]).unwrap();
}

#[test]
fn rotation_rejects_any_reason_except_exact_clone() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("installation.json");
    load_or_create_installation(&path, &[]).unwrap();

    assert!(rotate_installation(&path, "snapshot").is_err());
    assert!(rotate_installation(&path, " clone ").is_err());
}

#[test]
fn shipped_cli_rotates_the_selected_installation_without_a_database() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("cache/installation.json");
    let mirror = temp.path().join("memory-mirror");
    let old = load_or_create_installation(&path, &[]).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_cortex"))
        .args(["installation", "rotate", "--reason", "clone"])
        .env("MEMBRANE_INSTALLATION_FILE", &path)
        .env("MEMBRANE_MIRROR", &mirror)
        .env_remove("CORTEX_DB")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let rotated = read_identity(&path);
    assert_eq!(receipt["installation_id"], rotated.installation_id);
    assert_eq!(receipt["startup_generation"], 0);
    assert_eq!(rotated.lineage, vec![old.installation_id]);
}

#[test]
fn identity_reader_matches_python_iso_timestamp_validation() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("installation.json");
    let base = serde_json::json!({
        "schema_version": 2,
        "installation_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        "created_at": "2026-07-20T12:34:56.123456Z",
        "startup_generation": 0,
        "legacy_labels": [],
        "lineage": [],
        "current_service_instance_id": null,
        "current_claimed_at": null,
    });
    std::fs::write(&path, serde_json::to_vec_pretty(&base).unwrap()).unwrap();
    assert!(load_or_create_installation(&path, &[]).is_ok());

    for invalid in ["2026-07-20T12:34:56.Z", "2026-99-20T12:34:56Z"] {
        let mut value = base.clone();
        value["created_at"] = invalid.into();
        std::fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
        assert!(
            load_or_create_installation(&path, &[]).is_err(),
            "accepted {invalid}"
        );
    }
}
