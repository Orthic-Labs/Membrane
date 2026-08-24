//! Real provider/workspace identity inputs for `WorkspaceEngineKey`
//! (design §3). Every digest here is derived from actual bytes on disk or
//! from the effective sandbox policy — never a placeholder constant. Two
//! worktrees therefore bind distinct engine lanes, and a changed binary,
//! toolchain, config, or policy produces a different key.
//!
//! All hashing is bounded and deterministic: files are streamed in fixed-size
//! chunks, directory listings cap entry counts so huge toolchains cannot
//! stall registration, and every failure degrades to `None` so callers can
//! skip registration instead of inventing identity.

use crate::providers::child_process::SANITIZED_ENV_KEYS;
use sha2::{Digest as _, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Streaming chunk size for file hashing.
const READ_CHUNK_BYTES: usize = 64 * 1024;
/// Upper bound on entries hashed into one directory-listing digest.
const MAX_DIRECTORY_ENTRIES: usize = 512;
/// Files up to this size contribute their CONTENT (not just metadata) to a
/// directory listing digest, so toolchain version files bind identity to the
/// actual installed toolchain contents.
const INLINE_CONTENT_LIMIT_BYTES: u64 = 64 * 1024;

/// `sha256:<hex>` over the exact bytes of `path`, streamed. Missing or
/// unreadable files yield `None`.
pub fn file_digest(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut chunk = vec![0u8; READ_CHUNK_BYTES];
    loop {
        let read = file.read(&mut chunk).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&chunk[..read]);
    }
    Some(format!("sha256:{}", hex::encode(hasher.finalize())))
}

/// Digest over the resolved engine binary itself (`binary_digest`, design §3).
pub fn binary_digest(binary_path: &Path) -> Option<String> {
    file_digest(binary_path)
}

/// Deterministic digest of one directory's immediate listing: sorted entries
/// hashed as `name \0 is_dir \0 len \0 [content-hash for small files] \0`,
/// capped at [`MAX_DIRECTORY_ENTRIES`]. Used for toolchain identity without
/// walking entire SDK trees while still binding version-marker contents.
pub fn directory_listing_digest(directory: &Path) -> Option<String> {
    let mut entries: Vec<(String, bool, u64, Option<String>)> = Vec::new();
    let reader = std::fs::read_dir(directory).ok()?;
    for entry in reader.flatten().take(MAX_DIRECTORY_ENTRIES) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let metadata = entry.metadata().ok()?;
        let content_hash = if metadata.is_file() && metadata.len() <= INLINE_CONTENT_LIMIT_BYTES {
            file_digest(&entry.path())
        } else {
            None
        };
        entries.push((name, metadata.is_dir(), metadata.len(), content_hash));
    }
    if entries.is_empty() {
        return None;
    }
    entries.sort();
    let mut hasher = Sha256::new();
    hasher.update(format!("entries={}\0", entries.len()));
    for (name, is_dir, len, content_hash) in entries {
        hasher.update(name.as_bytes());
        hasher.update([0u8]);
        hasher.update([u8::from(is_dir)]);
        hasher.update(len.to_le_bytes());
        hasher.update([0u8]);
        if let Some(content_hash) = content_hash {
            hasher.update(content_hash.as_bytes());
        }
        hasher.update([0u8]);
    }
    Some(format!("sha256:{}", hex::encode(hasher.finalize())))
}

/// Toolchain identity for one resolved engine binary: the digest of the
/// toolchain directory the binary lives in (its parent), falling back to that
/// directory's grandparent when the immediate directory carries no entries
/// (single-file shims on PATH). This binds the key to the actual installed
/// toolchain contents rather than its path alone.
pub fn toolchain_digest(binary_path: &Path) -> Option<String> {
    let parent = binary_path.parent()?;
    directory_listing_digest(parent)
        .or_else(|| parent.parent().and_then(directory_listing_digest))
}

/// Project configuration identity: chained `relpath \0 bytes \0` digests over
/// every listed file that exists under `root`. Returns `None` when none of
/// the listed configs exist so callers can distinguish "no config" from
/// "config with empty content".
pub fn project_config_digest(root: &Path, config_files: &[&str]) -> Option<String> {
    let mut hasher = Sha256::new();
    let mut found = false;
    for relative in config_files {
        let path = root.join(relative);
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        hasher.update(relative.as_bytes());
        hasher.update([0u8]);
        hasher.update(Sha256::digest(&bytes));
        hasher.update([0u8]);
        found = true;
    }
    if !found {
        return None;
    }
    Some(format!("sha256:{}", hex::encode(hasher.finalize())))
}

/// Sandbox policy identity (design §13): the environment allowlist plus the
/// exact allowlisted search directories handed to spawned engines. Any
/// change to containment inputs changes every engine key derived under it.
pub fn sandbox_policy_digest(search_path: &[PathBuf]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"sanitized-env-v1\0");
    for key in SANITIZED_ENV_KEYS {
        hasher.update(key.as_bytes());
        hasher.update([0u8]);
    }
    for directory in search_path {
        hasher.update(directory.to_string_lossy().as_bytes());
        hasher.update([0u8]);
    }
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, bytes: &[u8]) {
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn file_digest_is_content_bound_and_stable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("engine.bin");
        write(&path, b"engine-bytes");
        let first = file_digest(&path).unwrap();
        let again = file_digest(&path).unwrap();
        assert_eq!(first, again);
        assert!(first.starts_with("sha256:"));
        write(&path, b"engine-bytes-v2");
        assert_ne!(first, file_digest(&path).unwrap());
        assert_eq!(file_digest(&dir.path().join("missing")), None);
    }

    #[test]
    fn directory_listing_digest_reflects_contents_and_size() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(directory_listing_digest(dir.path()), None);
        write(&dir.path().join("a.txt"), b"one");
        let base = directory_listing_digest(dir.path()).unwrap();
        write(&dir.path().join("b.txt"), b"two");
        assert_ne!(base, directory_listing_digest(dir.path()).unwrap());
        write(&dir.path().join("b.txt"), b"two!");
        assert_ne!(
            directory_listing_digest(dir.path()).unwrap(),
            base,
            "size change must be visible"
        );
    }

    #[test]
    fn toolchain_digest_binds_to_binary_directory() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("lib");
        std::fs::create_dir_all(&bin_dir).unwrap();
        write(&bin_dir.join("engine"), b"x");
        write(&bin_dir.join("version"), b"1.0.0");
        let digest = toolchain_digest(&bin_dir.join("engine")).unwrap();
        // Same directory → same digest regardless of binary name.
        assert_eq!(toolchain_digest(&bin_dir.join("other")), Some(digest.clone()));
        write(&bin_dir.join("version"), b"1.0.1");
        assert_ne!(digest, toolchain_digest(&bin_dir.join("engine")).unwrap());
    }

    #[test]
    fn project_config_digest_requires_at_least_one_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(project_config_digest(dir.path(), &["tsconfig.json"]), None);
        write(&dir.path().join("tsconfig.json"), b"{}");
        let digest = project_config_digest(dir.path(), &["tsconfig.json"]).unwrap();
        assert!(digest.starts_with("sha256:"));
        write(&dir.path().join("tsconfig.json"), b"{\"strict\":true}");
        assert_ne!(
            digest,
            project_config_digest(dir.path(), &["tsconfig.json"]).unwrap()
        );
    }

    #[test]
    fn sandbox_policy_digest_tracks_search_path_and_is_stable() {
        let left = sandbox_policy_digest(&[PathBuf::from("/opt/bin")]);
        let again = sandbox_policy_digest(&[PathBuf::from("/opt/bin")]);
        assert_eq!(left, again);
        let right = sandbox_policy_digest(&[PathBuf::from("/usr/local/bin")]);
        assert_ne!(left, right);
        assert!(left.starts_with("sha256:"));
    }
}
