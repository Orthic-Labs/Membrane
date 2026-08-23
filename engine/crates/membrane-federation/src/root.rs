//! Canonical repository-root binding for federation ingress.
//!
//! Root resolution is deliberately behind [`RootPathSource`].  Federation
//! validation must make its path and filesystem policy explicit, while tests
//! and resident composition can inject the owner of that policy.  No caller
//! may use the uncanonicalized request root for authorization or source I/O.

use std::fs;
use std::path::{Component, Path, PathBuf};

/// The canonical identity returned by a root owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalRepositoryRoot {
    /// Existing, absolute, canonical directory used for source access.
    pub path: PathBuf,
    /// Stable scope identity.  This is intentionally not a machine path.
    pub repository_id: String,
    /// Canonical worktree/repository identity containing `path`.
    pub worktree_root: PathBuf,
}

impl CanonicalRepositoryRoot {
    pub fn new(
        path: impl Into<PathBuf>,
        repository_id: impl Into<String>,
        worktree_root: impl Into<PathBuf>,
    ) -> Result<Self, RootError> {
        let path = path.into();
        let repository_id = repository_id.into();
        let worktree_root = worktree_root.into();
        if !path.is_absolute() || !worktree_root.is_absolute() {
            return Err(RootError::NotAbsolute);
        }
        if repository_id.trim().is_empty()
            || repository_id
                .chars()
                .any(|character| character == '/' || character == '\\' || character == ':')
        {
            return Err(RootError::MissingIdentity);
        }
        if !path.starts_with(&worktree_root) {
            return Err(RootError::OutsideWorktree);
        }
        // Keep construction side-effect free.  The injected owner has already
        // established directory, symlink/junction, and repository invariants.
        Ok(Self {
            path,
            repository_id,
            worktree_root,
        })
    }
}

/// Content-free root failures.  Details never include an unauthorized path.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RootError {
    #[error("repository root must be absolute")]
    NotAbsolute,
    #[error("repository root is unavailable")]
    Unavailable,
    #[error("repository root is not a directory")]
    NotDirectory,
    #[error("repository root is a symlink or junction")]
    Aliased,
    #[error("repository root is outside its declared worktree")]
    OutsideWorktree,
    #[error("repository identity is missing")]
    MissingIdentity,
    #[error("repository identity is invalid")]
    InvalidIdentity,
}

/// Owner-provided path/filesystem boundary used by request validation.
///
/// Implementations must canonicalize before returning, reject aliases and
/// traversal, ensure `path` is a directory, and bind it to `worktree_root`.
/// They must not read repository content.
pub trait RootPathSource {
    fn resolve_root(&self, requested: &Path) -> Result<CanonicalRepositoryRoot, RootError>;
}

/// Default local implementation for callers that do not need a resident-owned
/// path policy.  Repository identity is derived from canonical root only; it
/// never exposes that machine path as an identity value.
#[derive(Clone, Copy, Debug, Default)]
pub struct FilesystemRootSource;

impl RootPathSource for FilesystemRootSource {
    fn resolve_root(&self, requested: &Path) -> Result<CanonicalRepositoryRoot, RootError> {
        if !requested.is_absolute() {
            return Err(RootError::NotAbsolute);
        }
        reject_alias_components(requested)?;
        let metadata = fs::symlink_metadata(requested).map_err(|_| RootError::Unavailable)?;
        if !metadata.is_dir() {
            return Err(RootError::NotDirectory);
        }
        let canonical = requested.canonicalize().map_err(|_| RootError::Unavailable)?;
        let canonical_metadata = fs::metadata(&canonical).map_err(|_| RootError::Unavailable)?;
        if !canonical_metadata.is_dir() {
            return Err(RootError::NotDirectory);
        }
        let repository_id = canonical_repository_id(&canonical);
        CanonicalRepositoryRoot::new(canonical.clone(), repository_id, canonical)
    }
}

/// Inspect every existing path component without following it.  Checking only
/// the final component is insufficient: `/repo/link/subdir` would otherwise
/// canonicalize through `link` before authorization.
fn reject_alias_components(path: &Path) -> Result<(), RootError> {
    for component in path.ancestors() {
        let Ok(metadata) = fs::symlink_metadata(component) else {
            continue;
        };
        if metadata.file_type().is_symlink() || is_windows_reparse_point(&metadata) {
            return Err(RootError::Aliased);
        }
    }
    Ok(())
}

#[cfg(windows)]
fn is_windows_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_windows_reparse_point(_: &fs::Metadata) -> bool {
    false
}

/// Resolve an absolute request root through an injected source.
pub fn resolve_canonical_root<S: RootPathSource>(
    source: &S,
    requested: &str,
) -> Result<CanonicalRepositoryRoot, RootError> {
    if !is_absolute_path(requested) {
        return Err(RootError::NotAbsolute);
    }
    source.resolve_root(Path::new(requested))
}

/// Stable scope slug shared with the existing Cortex scope contract.
pub fn canonical_repository_id(path: &Path) -> String {
    let mut value = path.to_string_lossy().replace('\\', "/");
    if let Some(rest) = value.strip_prefix("//?/") {
        value = rest.to_owned();
    }
    value = value.replace(':', "-").replace('/', "-");
    let value = value.trim_matches('-');
    if value.is_empty() {
        return "global".to_owned();
    }
    let bytes = value.as_bytes();
    if bytes.len() > 1 && bytes[0].is_ascii_lowercase() && bytes[1] == b'-' {
        let mut normalized = value.to_owned();
        normalized.replace_range(..1, &value[..1].to_ascii_uppercase());
        normalized
    } else {
        value.to_owned()
    }
}

/// Portable absolute-path test covering Unix, drive, and UNC spellings.
pub fn is_absolute_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    value.starts_with('/')
        || value.starts_with('\\')
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes[2] == b'/' || bytes[2] == b'\\'))
}

/// Lexically normalize a path without consulting filesystem contents.
/// `..` may not escape the root; this is used for anchors after root binding.
pub fn normalize_relative_path(value: &str) -> Result<String, RootError> {
    if value.is_empty() || value.bytes().any(|byte| byte == 0 || byte < 0x20) {
        return Err(RootError::InvalidIdentity);
    }
    let mut parts = Vec::new();
    let normalized = value.replace('\\', "/");
    for component in normalized.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(RootError::OutsideWorktree);
                }
            }
            part => parts.push(part),
        }
    }
    if parts.is_empty() {
        return Err(RootError::InvalidIdentity);
    }
    Ok(parts.join("/"))
}

/// Convert an absolute anchor to a relative path only when it is lexically
/// contained by the canonical root.  No content or symlink resolution occurs.
pub fn normalize_anchor_path(root: &Path, anchor: &str) -> Result<String, RootError> {
    if is_absolute_path(anchor) {
        let root_text = root.to_string_lossy().replace('\\', "/");
        let requested_text = anchor.replace('\\', "/");
        let normalized_root = normalize_absolute_text(&root_text)?;
        let normalized_requested = normalize_absolute_text(&requested_text)?;
        let prefix = format!("{normalized_root}/");
        if normalized_requested == normalized_root {
            return Err(RootError::InvalidIdentity);
        }
        if !normalized_requested.starts_with(&prefix) {
            return Err(RootError::OutsideWorktree);
        }
        return normalize_relative_path(&normalized_requested[prefix.len()..]);
    }
    normalize_relative_path(anchor)
}

fn normalize_absolute_text(value: &str) -> Result<String, RootError> {
    if !is_absolute_path(value) {
        return Err(RootError::NotAbsolute);
    }
    let prefix = if value.starts_with("//") || value.starts_with("\\\\") {
        "//"
    } else if value.starts_with('/') || value.starts_with('\\') {
        "/"
    } else {
        &value[..3]
    };
    let rest = value[prefix.len()..].replace('\\', "/");
    let mut parts = Vec::new();
    for component in rest.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(RootError::OutsideWorktree);
                }
            }
            part => parts.push(part),
        }
    }
    let mut out = prefix.to_owned();
    out.push_str(&parts.join("/"));
    Ok(out.trim_end_matches('/').to_owned())
}

#[allow(dead_code)]
fn _path_has_only_normal_components(path: &Path) -> bool {
    path.components().all(|component| {
        !matches!(component, Component::ParentDir | Component::RootDir)
    })
}
