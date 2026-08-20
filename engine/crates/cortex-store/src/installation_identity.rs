//! Stable per-installation identity and append-only startup claims.
//!
//! The JSON schema and mirror paths intentionally match
//! `tools/lib/memory/installation_identity.py` so the resident Rust service and batch Python
//! synchronizer share one N-machine identity contract.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const IDENTITY_SCHEMA_VERSION: u32 = 2;
const CLAIM_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallationIdentity {
    pub schema_version: u32,
    pub installation_id: String,
    pub created_at: String,
    pub startup_generation: u64,
    pub legacy_labels: Vec<String>,
    pub lineage: Vec<String>,
    pub current_service_instance_id: Option<String>,
    pub current_claimed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartupClaim {
    pub schema_version: u32,
    pub installation_id: String,
    pub startup_generation: u64,
    pub service_instance_id: String,
    pub claimed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallationPaths {
    pub identity: PathBuf,
    pub mirror: PathBuf,
}

impl InstallationPaths {
    pub fn defaults_for_workspace(workspace_root: &Path) -> Self {
        Self {
            identity: workspace_root.join("tools/.cache/memory/installation.json"),
            mirror: workspace_root.join("memory-mirror"),
        }
    }

    pub fn for_workspace(workspace_root: &Path) -> Self {
        let defaults = Self::defaults_for_workspace(workspace_root);
        Self {
            identity: std::env::var_os("CORTEX_INSTALLATION_FILE")
                .map(PathBuf::from)
                .unwrap_or(defaults.identity),
            mirror: std::env::var_os("CORTEX_MIRROR")
                .map(PathBuf::from)
                .unwrap_or(defaults.mirror),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InstallationIdentityError {
    #[error("{operation} {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid installation identity: {0}")]
    InvalidIdentity(PathBuf),
    #[error("invalid startup claim: {0}")]
    InvalidClaim(PathBuf),
    #[error("{0}")]
    Invalid(String),
    #[error(
        "clone/snapshot collision quarantines installation {installation_id}; startup_generation={generations}; rotate identity with reason clone"
    )]
    Quarantined {
        installation_id: String,
        generations: String,
    },
}

fn io_error(
    operation: &'static str,
    path: &Path,
    source: std::io::Error,
) -> InstallationIdentityError {
    InstallationIdentityError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

fn unique_trimmed(values: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for value in values {
        let value = value.trim();
        if !value.is_empty() && seen.insert(value.to_string()) {
            result.push(value.to_string());
        }
    }
    result
}

fn valid_uuid4(value: &str) -> bool {
    let bytes = value.as_bytes();
    value.len() == 36
        && [8, 13, 18, 23].iter().all(|index| bytes[*index] == b'-')
        && bytes[14] == b'4'
        && matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
        && bytes.iter().enumerate().all(|(index, byte)| {
            [8, 13, 18, 23].contains(&index) || byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')
        })
}

fn require_uuid4(value: &str, field: &str) -> Result<(), InstallationIdentityError> {
    if valid_uuid4(value) {
        Ok(())
    } else {
        Err(InstallationIdentityError::Invalid(format!(
            "{field} must be a lowercase UUIDv4"
        )))
    }
}

fn new_uuid4() -> Result<String, InstallationIdentityError> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| InstallationIdentityError::Invalid(format!("generate UUIDv4: {error}")))?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11],
        bytes[12], bytes[13], bytes[14], bytes[15]
    ))
}

fn valid_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || !bytes.ends_with(b"Z")
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return false;
    }
    let decimal = |slice: &[u8]| -> Option<u32> {
        if slice.iter().all(u8::is_ascii_digit) {
            std::str::from_utf8(slice).ok()?.parse().ok()
        } else {
            None
        }
    };
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) = (
        decimal(&bytes[0..4]),
        decimal(&bytes[5..7]),
        decimal(&bytes[8..10]),
        decimal(&bytes[11..13]),
        decimal(&bytes[14..16]),
        decimal(&bytes[17..19]),
    ) else {
        return false;
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    if year == 0 || day == 0 || day > max_day || hour > 23 || minute > 59 || second > 59 {
        return false;
    }
    bytes.len() == 20
        || (bytes.len() > 21
            && bytes.get(19) == Some(&b'.')
            && bytes[20..bytes.len() - 1].iter().all(u8::is_ascii_digit))
}

fn validate_identity(identity: &InstallationIdentity) -> Result<(), InstallationIdentityError> {
    if identity.schema_version != IDENTITY_SCHEMA_VERSION {
        return Err(InstallationIdentityError::Invalid(format!(
            "installation schema_version must be {IDENTITY_SCHEMA_VERSION}"
        )));
    }
    require_uuid4(&identity.installation_id, "installation_id")?;
    if !valid_timestamp(&identity.created_at) {
        return Err(InstallationIdentityError::Invalid(
            "created_at must be an ISO-8601 UTC timestamp".to_string(),
        ));
    }
    if identity
        .legacy_labels
        .iter()
        .any(|value| value.trim().is_empty())
    {
        return Err(InstallationIdentityError::Invalid(
            "legacy_labels must contain non-empty strings".to_string(),
        ));
    }
    for lineage_id in &identity.lineage {
        require_uuid4(lineage_id, "lineage")?;
    }
    match (
        identity.current_service_instance_id.as_deref(),
        identity.current_claimed_at.as_deref(),
    ) {
        (None, None) => {}
        (Some(service_id), Some(claimed_at)) => {
            require_uuid4(service_id, "current_service_instance_id")?;
            if !valid_timestamp(claimed_at) {
                return Err(InstallationIdentityError::Invalid(
                    "current_claimed_at must be an ISO-8601 UTC timestamp".to_string(),
                ));
            }
        }
        _ => {
            return Err(InstallationIdentityError::Invalid(
                "current startup claim is partial".to_string(),
            ));
        }
    }
    Ok(())
}

fn validate_claim(claim: &StartupClaim) -> Result<(), InstallationIdentityError> {
    if claim.schema_version != CLAIM_SCHEMA_VERSION {
        return Err(InstallationIdentityError::Invalid(format!(
            "startup claim schema_version must be {CLAIM_SCHEMA_VERSION}"
        )));
    }
    require_uuid4(&claim.installation_id, "installation_id")?;
    require_uuid4(&claim.service_instance_id, "service_instance_id")?;
    if claim.startup_generation == 0 {
        return Err(InstallationIdentityError::Invalid(
            "startup_generation must be positive".to_string(),
        ));
    }
    if !valid_timestamp(&claim.claimed_at) {
        return Err(InstallationIdentityError::Invalid(
            "claimed_at must be an ISO-8601 UTC timestamp".to_string(),
        ));
    }
    Ok(())
}

/// Validate that a startup claim is the one currently held by this installation identity.
/// A structurally valid historical claim is not active after the identity advances to a new
/// startup generation.
pub fn validate_active_startup_claim(
    identity: &InstallationIdentity,
    claim: &StartupClaim,
) -> Result<(), InstallationIdentityError> {
    validate_identity(identity)?;
    validate_claim(claim)?;
    if claim.installation_id != identity.installation_id
        || claim.startup_generation != identity.startup_generation
        || identity.current_service_instance_id.as_deref()
            != Some(claim.service_instance_id.as_str())
        || identity.current_claimed_at.as_deref() != Some(claim.claimed_at.as_str())
    {
        return Err(InstallationIdentityError::Invalid(
            "startup claim is not the installation's active lease".to_string(),
        ));
    }
    Ok(())
}

fn encoded<T: Serialize>(value: &T) -> Result<Vec<u8>, InstallationIdentityError> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| InstallationIdentityError::Invalid(format!("encode JSON: {error}")))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn read_identity(path: &Path) -> Result<InstallationIdentity, InstallationIdentityError> {
    let bytes = fs::read(path).map_err(|error| io_error("read", path, error))?;
    let identity: InstallationIdentity = serde_json::from_slice(&bytes)
        .map_err(|_| InstallationIdentityError::InvalidIdentity(path.to_path_buf()))?;
    validate_identity(&identity)
        .map_err(|_| InstallationIdentityError::InvalidIdentity(path.to_path_buf()))?;
    Ok(identity)
}

fn temporary_path(path: &Path) -> Result<PathBuf, InstallationIdentityError> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("identity");
    Ok(path.with_file_name(format!(".{name}.{}.tmp", new_uuid4()?)))
}

fn write_completed_temp(path: &Path, data: &[u8]) -> Result<PathBuf, InstallationIdentityError> {
    let parent = path.parent().ok_or_else(|| {
        InstallationIdentityError::Invalid(format!("path has no parent: {}", path.display()))
    })?;
    fs::create_dir_all(parent).map_err(|error| io_error("create directory", parent, error))?;
    let temporary = temporary_path(path)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| io_error("create temporary file", &temporary, error))?;
    if let Err(error) = file.write_all(data).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&temporary);
        return Err(io_error("write temporary file", &temporary, error));
    }
    Ok(temporary)
}

fn write_exclusive(path: &Path, data: &[u8]) -> Result<bool, InstallationIdentityError> {
    let temporary = write_completed_temp(path, data)?;
    let result = match fs::hard_link(&temporary, path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(io_error("publish immutable file", path, error)),
    };
    let _ = fs::remove_file(&temporary);
    result
}

#[cfg(windows)]
fn replace_file(temporary: &Path, path: &Path) -> Result<(), InstallationIdentityError> {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "Kernel32")]
    extern "system" {
        fn MoveFileExW(existing: *const u16, target: *const u16, flags: u32) -> i32;
    }

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    let existing: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let result = unsafe {
        MoveFileExW(
            existing.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io_error(
            "atomically replace",
            path,
            std::io::Error::last_os_error(),
        ))
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file(temporary: &Path, path: &Path) -> Result<(), InstallationIdentityError> {
    fs::rename(temporary, path).map_err(|error| io_error("atomically replace", path, error))
}

fn atomic_replace(path: &Path, data: &[u8]) -> Result<(), InstallationIdentityError> {
    let temporary = write_completed_temp(path, data)?;
    let result = replace_file(&temporary, path);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

struct IdentityLock {
    file: File,
}

impl IdentityLock {
    fn acquire(identity_path: &Path) -> Result<Self, InstallationIdentityError> {
        let lock_path = identity_path.with_extension(format!(
            "{}lock",
            identity_path
                .extension()
                .and_then(|value| value.to_str())
                .map(|value| format!("{value}."))
                .unwrap_or_default()
        ));
        let parent = lock_path.parent().ok_or_else(|| {
            InstallationIdentityError::Invalid(format!(
                "lock path has no parent: {}",
                lock_path.display()
            ))
        })?;
        fs::create_dir_all(parent)
            .map_err(|error| io_error("create lock directory", parent, error))?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|error| io_error("open identity lock", &lock_path, error))?;
        lock_file(&file).map_err(|error| io_error("lock identity", &lock_path, error))?;
        let mut guard = Self { file };
        if guard
            .file
            .metadata()
            .map_err(|error| io_error("inspect identity lock", &lock_path, error))?
            .len()
            == 0
        {
            guard
                .file
                .seek(SeekFrom::Start(0))
                .and_then(|_| guard.file.write_all(&[0]))
                .and_then(|_| guard.file.sync_all())
                .map_err(|error| io_error("initialize identity lock", &lock_path, error))?;
        }
        Ok(guard)
    }
}

impl Drop for IdentityLock {
    fn drop(&mut self) {
        let _ = unlock_file(&self.file);
    }
}

#[cfg(windows)]
fn lock_file(file: &File) -> std::io::Result<()> {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;

    #[repr(C)]
    struct Overlapped {
        internal: usize,
        internal_high: usize,
        offset: u32,
        offset_high: u32,
        event: *mut c_void,
    }
    #[link(name = "Kernel32")]
    extern "system" {
        fn LockFileEx(
            file: *mut c_void,
            flags: u32,
            reserved: u32,
            bytes_low: u32,
            bytes_high: u32,
            overlapped: *mut Overlapped,
        ) -> i32;
    }
    const LOCKFILE_EXCLUSIVE_LOCK: u32 = 0x2;
    let mut overlapped = Overlapped {
        internal: 0,
        internal_high: 0,
        offset: 0,
        offset_high: 0,
        event: std::ptr::null_mut(),
    };
    let result = unsafe {
        LockFileEx(
            file.as_raw_handle(),
            LOCKFILE_EXCLUSIVE_LOCK,
            0,
            1,
            0,
            &mut overlapped,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn unlock_file(file: &File) -> std::io::Result<()> {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;

    #[repr(C)]
    struct Overlapped {
        internal: usize,
        internal_high: usize,
        offset: u32,
        offset_high: u32,
        event: *mut c_void,
    }
    #[link(name = "Kernel32")]
    extern "system" {
        fn UnlockFileEx(
            file: *mut c_void,
            reserved: u32,
            bytes_low: u32,
            bytes_high: u32,
            overlapped: *mut Overlapped,
        ) -> i32;
    }
    let mut overlapped = Overlapped {
        internal: 0,
        internal_high: 0,
        offset: 0,
        offset_high: 0,
        event: std::ptr::null_mut(),
    };
    let result = unsafe { UnlockFileEx(file.as_raw_handle(), 0, 1, 0, &mut overlapped) };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(unix)]
fn lock_file(file: &File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }
    let result = unsafe { flock(file.as_raw_fd(), 2) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(unix)]
fn unlock_file(file: &File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }
    let result = unsafe { flock(file.as_raw_fd(), 8) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

fn new_identity(
    legacy_labels: &[String],
    lineage: Vec<String>,
) -> Result<InstallationIdentity, InstallationIdentityError> {
    let identity = InstallationIdentity {
        schema_version: IDENTITY_SCHEMA_VERSION,
        installation_id: new_uuid4()?,
        created_at: crate::time::now_iso(),
        startup_generation: 0,
        legacy_labels: unique_trimmed(legacy_labels),
        lineage,
        current_service_instance_id: None,
        current_claimed_at: None,
    };
    validate_identity(&identity)?;
    Ok(identity)
}

pub fn load_or_create_installation(
    path: &Path,
    legacy_labels: &[String],
) -> Result<InstallationIdentity, InstallationIdentityError> {
    if !path.exists() {
        let candidate = new_identity(legacy_labels, Vec::new())?;
        if write_exclusive(path, &encoded(&candidate)?)? {
            return Ok(candidate);
        }
    }
    read_identity(path)
}

fn claim_path(mirror_root: &Path, claim: &StartupClaim) -> PathBuf {
    mirror_root
        .join("_installation_claims")
        .join(&claim.installation_id)
        .join(format!("{:020}", claim.startup_generation))
        .join(format!("{}.json", claim.service_instance_id))
}

fn publish_claim(
    mirror_root: &Path,
    claim: &StartupClaim,
) -> Result<PathBuf, InstallationIdentityError> {
    validate_claim(claim)?;
    let path = claim_path(mirror_root, claim);
    let data = encoded(claim)?;
    if !write_exclusive(&path, &data)? {
        let existing =
            fs::read(&path).map_err(|error| io_error("read startup claim", &path, error))?;
        if existing != data {
            return Err(InstallationIdentityError::Invalid(format!(
                "immutable startup claim collision: {}",
                path.display()
            )));
        }
    }
    Ok(path)
}

fn read_claim(path: &Path) -> Result<StartupClaim, InstallationIdentityError> {
    let bytes = fs::read(path).map_err(|error| io_error("read startup claim", path, error))?;
    let claim: StartupClaim = serde_json::from_slice(&bytes)
        .map_err(|_| InstallationIdentityError::InvalidClaim(path.to_path_buf()))?;
    validate_claim(&claim)
        .map_err(|_| InstallationIdentityError::InvalidClaim(path.to_path_buf()))?;
    Ok(claim)
}

pub fn assert_installation_not_quarantined(
    mirror_root: &Path,
    installation_id: &str,
) -> Result<(), InstallationIdentityError> {
    require_uuid4(installation_id, "installation_id")?;
    let root = mirror_root
        .join("_installation_claims")
        .join(installation_id);
    if !root.exists() {
        return Ok(());
    }
    let mut collisions = Vec::new();
    for generation_entry in
        fs::read_dir(&root).map_err(|error| io_error("read claim generations", &root, error))?
    {
        let generation_entry =
            generation_entry.map_err(|error| io_error("read claim generation", &root, error))?;
        if !generation_entry
            .file_type()
            .map_err(|error| io_error("inspect claim generation", &generation_entry.path(), error))?
            .is_dir()
        {
            continue;
        }
        let generation_name = generation_entry.file_name().to_string_lossy().to_string();
        if generation_name.len() != 20 || !generation_name.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(InstallationIdentityError::InvalidClaim(
                generation_entry.path(),
            ));
        }
        let generation: u64 = generation_name
            .parse()
            .map_err(|_| InstallationIdentityError::InvalidClaim(generation_entry.path()))?;
        let generation_path = generation_entry.path();
        let mut service_ids = BTreeSet::new();
        for claim_entry in fs::read_dir(&generation_path)
            .map_err(|error| io_error("read startup claims", &generation_path, error))?
        {
            let claim_entry = claim_entry
                .map_err(|error| io_error("read startup claim", &generation_path, error))?;
            let path = claim_entry.path();
            if !claim_entry
                .file_type()
                .map_err(|error| io_error("inspect startup claim", &path, error))?
                .is_file()
                || path.extension().and_then(|value| value.to_str()) != Some("json")
            {
                continue;
            }
            let claim = read_claim(&path)?;
            if claim.installation_id != installation_id
                || claim.startup_generation != generation
                || path.file_stem().and_then(|value| value.to_str())
                    != Some(claim.service_instance_id.as_str())
                || claim_path(mirror_root, &claim) != path
            {
                return Err(InstallationIdentityError::InvalidClaim(path));
            }
            service_ids.insert(claim.service_instance_id);
        }
        if service_ids.len() > 1 {
            collisions.push(generation);
        }
    }
    if collisions.is_empty() {
        Ok(())
    } else {
        collisions.sort_unstable();
        Err(InstallationIdentityError::Quarantined {
            installation_id: installation_id.to_string(),
            generations: collisions
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(","),
        })
    }
}

pub fn start_and_publish(
    identity_path: &Path,
    mirror_root: &Path,
    service_instance_id: Option<&str>,
    legacy_labels: &[String],
) -> Result<(InstallationIdentity, StartupClaim), InstallationIdentityError> {
    let service_instance_id = match service_instance_id {
        Some(value) => {
            require_uuid4(value, "service_instance_id")?;
            value.to_string()
        }
        None => new_uuid4()?,
    };
    let _lock = IdentityLock::acquire(identity_path)?;
    let identity = load_or_create_installation(identity_path, legacy_labels)?;
    assert_installation_not_quarantined(mirror_root, &identity.installation_id)?;
    let startup_generation = identity.startup_generation.checked_add(1).ok_or_else(|| {
        InstallationIdentityError::Invalid("startup_generation overflow".to_string())
    })?;
    let claimed_at = crate::time::now_iso();
    let advanced = InstallationIdentity {
        startup_generation,
        current_service_instance_id: Some(service_instance_id.clone()),
        current_claimed_at: Some(claimed_at.clone()),
        ..identity
    };
    atomic_replace(identity_path, &encoded(&advanced)?)?;
    let claim = StartupClaim {
        schema_version: CLAIM_SCHEMA_VERSION,
        installation_id: advanced.installation_id.clone(),
        startup_generation,
        service_instance_id,
        claimed_at,
    };
    publish_claim(mirror_root, &claim)?;
    assert_installation_not_quarantined(mirror_root, &advanced.installation_id)?;
    Ok((advanced, claim))
}

pub fn prepare_service_start(
    workspace_root: &Path,
) -> Result<(InstallationIdentity, StartupClaim), InstallationIdentityError> {
    let paths = InstallationPaths::for_workspace(workspace_root);
    let legacy_labels = std::env::var("CORTEX_LEGACY_MACHINE_LABEL")
        .ok()
        .into_iter()
        .collect::<Vec<_>>();
    start_and_publish(&paths.identity, &paths.mirror, None, &legacy_labels)
}

pub fn rotate_installation(
    path: &Path,
    reason: &str,
) -> Result<InstallationIdentity, InstallationIdentityError> {
    if reason != "clone" {
        return Err(InstallationIdentityError::Invalid(
            "rotation reason must be exactly 'clone'".to_string(),
        ));
    }
    let _lock = IdentityLock::acquire(path)?;
    let old = read_identity(path)?;
    let archive = path
        .parent()
        .ok_or_else(|| {
            InstallationIdentityError::Invalid(format!("path has no parent: {}", path.display()))
        })?
        .join("installation-archive")
        .join(format!("{}.json", old.installation_id));
    let mut archive_value = serde_json::to_value(&old)
        .map_err(|error| InstallationIdentityError::Invalid(format!("encode archive: {error}")))?;
    let object = archive_value.as_object_mut().ok_or_else(|| {
        InstallationIdentityError::Invalid("installation archive must be an object".to_string())
    })?;
    object.insert(
        "rotation_reason".to_string(),
        serde_json::Value::String("clone".to_string()),
    );
    object.insert(
        "rotated_at".to_string(),
        serde_json::Value::String(crate::time::now_iso()),
    );
    let archive_data = encoded(&archive_value)?;
    if !write_exclusive(&archive, &archive_data)? {
        let existing: serde_json::Value = serde_json::from_slice(
            &fs::read(&archive).map_err(|error| io_error("read archive", &archive, error))?,
        )
        .map_err(|_| InstallationIdentityError::InvalidIdentity(archive.clone()))?;
        if existing
            .get("installation_id")
            .and_then(|value| value.as_str())
            != Some(old.installation_id.as_str())
            || existing
                .get("rotation_reason")
                .and_then(|value| value.as_str())
                != Some("clone")
        {
            return Err(InstallationIdentityError::InvalidIdentity(archive));
        }
    }
    let mut lineage = old.lineage.clone();
    if !lineage.contains(&old.installation_id) {
        lineage.push(old.installation_id);
    }
    let rotated = new_identity(&old.legacy_labels, lineage)?;
    atomic_replace(path, &encoded(&rotated)?)?;
    Ok(rotated)
}
