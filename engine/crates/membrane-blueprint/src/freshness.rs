//! Source-state observation, freshness gates, and bounded stable reads.
//!
//! This module deliberately has no process, shell, or runtime dependency. A
//! caller may provide a VCS observation, while filesystem snapshots remain the
//! authoritative reconciliation mechanism in [`crate::watch`].

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;
use std::time::SystemTime;

use thiserror::Error;
use serde::{Deserialize, Serialize};
use xxhash_rust::xxh3::xxh3_128;

pub const MAX_SOURCE_FILE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessState {
    Fresh,
    ChangedSinceGeneration,
    Unknown,
    Unavailable,
}

impl FreshnessState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::ChangedSinceGeneration => "changed_since_generation",
            Self::Unknown => "unknown",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct GenerationFreshnessBasis {
    pub indexed_revision: Option<String>,
    pub indexed_worktree_fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CurrentSourceState {
    pub available: bool,
    pub vcs_revision: Option<String>,
    pub dirty: Option<bool>,
    pub worktree_fingerprint: Option<String>,
}

pub fn evaluate_freshness(
    generation: &GenerationFreshnessBasis,
    current: &CurrentSourceState,
) -> FreshnessState {
    if !current.available {
        return FreshnessState::Unavailable;
    }
    let (Some(indexed_revision), Some(indexed_fingerprint)) = (
        generation.indexed_revision.as_deref(),
        generation.indexed_worktree_fingerprint.as_deref(),
    ) else {
        return FreshnessState::Unknown;
    };
    if current.vcs_revision.as_deref() == Some(indexed_revision)
        && current.worktree_fingerprint.as_deref() == Some(indexed_fingerprint)
    {
        FreshnessState::Fresh
    } else {
        FreshnessState::ChangedSinceGeneration
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("generation mismatch: pinned {pinned:?}, served {served:?}")]
pub struct GenerationMismatch {
    pub pinned: Option<String>,
    pub served: Option<String>,
}

pub fn assert_generation_coherence(
    pinned_generation_id: Option<&str>,
    served_generation_id: Option<&str>,
) -> Result<(), GenerationMismatch> {
    match pinned_generation_id {
        None => Ok(()),
        Some(pinned) if Some(pinned) == served_generation_id => Ok(()),
        Some(pinned) => Err(GenerationMismatch {
            pinned: Some(pinned.to_owned()),
            served: served_generation_id.map(str::to_owned),
        }),
    }
}

#[derive(Debug, Error)]
pub enum StableReadError {
    #[error("source file is too large: {size} bytes (limit {limit})")]
    TooLarge { size: u64, limit: u64 },
    #[error("source read failed: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileObservation {
    pub size: u64,
    pub modified_ns: Option<u128>,
    pub identity: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StableRead {
    pub bytes: Vec<u8>,
    pub content_digest: String,
    pub before: FileObservation,
    pub after: FileObservation,
    pub unstable: bool,
}

fn modified_ns(metadata: &fs::Metadata) -> Option<u128> {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
}

#[cfg(unix)]
fn file_identity(metadata: &fs::Metadata) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    Some(format!("{}:{}", metadata.dev(), metadata.ino()))
}

// Windows' stable std metadata API does not expose a portable file-id pair.
// Size + mtime remain the bounded identity signals; callers must not depend on
// unstable platform-extension methods for freshness correctness.
#[cfg(windows)]
fn file_identity(_metadata: &fs::Metadata) -> Option<String> { None }

#[cfg(not(any(unix, windows)))]
fn file_identity(_metadata: &fs::Metadata) -> Option<String> { None }

fn observation(metadata: &fs::Metadata) -> FileObservation {
    FileObservation {
        size: metadata.len(),
        modified_ns: modified_ns(metadata),
        identity: file_identity(metadata),
    }
}

fn changed(before: &FileObservation, after: &FileObservation) -> bool {
    before.size != after.size
        || before.modified_ns != after.modified_ns
        || before.identity != after.identity
}

pub fn content_digest(bytes: &[u8]) -> String {
    format!("xxh128:{:032x}", xxh3_128(bytes))
}

/// Read a regular source file at most twice, never accepting an unstable
/// second observation as if it were stable.
pub fn stable_read(path: &Path) -> Result<StableRead, StableReadError> {
    stable_read_with_limit(path, MAX_SOURCE_FILE_BYTES)
}

pub fn stable_read_with_limit(path: &Path, limit: u64) -> Result<StableRead, StableReadError> {
    let mut before = observation(&fs::metadata(path)?);
    if before.size > limit {
        return Err(StableReadError::TooLarge { size: before.size, limit });
    }
    let mut bytes = read_bounded(path, before.size, limit)?;
    let mut after = observation(&fs::metadata(path)?);
    let mut unstable = changed(&before, &after);
    if unstable {
        before = observation(&fs::metadata(path)?);
        if before.size > limit {
            return Err(StableReadError::TooLarge { size: before.size, limit });
        }
        bytes = read_bounded(path, before.size, limit)?;
        after = observation(&fs::metadata(path)?);
        unstable = changed(&before, &after);
    }
    Ok(StableRead { content_digest: content_digest(&bytes), bytes, before, after, unstable })
}

fn read_bounded(path: &Path, expected_size: u64, limit: u64) -> Result<Vec<u8>, StableReadError> {
    if expected_size > limit {
        return Err(StableReadError::TooLarge { size: expected_size, limit });
    }
    let file = File::open(path)?;
    let capacity = usize::try_from(expected_size).unwrap_or(usize::MAX).min(limit as usize);
    let mut bytes = Vec::with_capacity(capacity);
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(StableReadError::TooLarge { size: bytes.len() as u64, limit });
    }
    Ok(bytes)
}
