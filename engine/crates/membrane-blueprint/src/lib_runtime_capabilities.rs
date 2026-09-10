//! Native Rust port of `blueprint/src/lib/runtime-capabilities.mjs`.
//!
//! Lane LIB4 (r5 closure): this module has no prior native equivalent.
//!
//! The legacy module exists to detect an under-floor **Node.js** runtime
//! (e.g. missing `node:sqlite`) before the legacy JS toolchain does real
//! work. A native Rust binary has no Node runtime and no `process.version`,
//! so that top-level "am I running on a good enough Node" check does not
//! apply here. What IS ported, faithfully, is the portable logic: the
//! minimal `>=X.Y.Z` semver-floor parser/comparator (`parseMajorMinor` /
//! `supportsCurrent`), generalized to a reusable `RuntimeVersion` +
//! `meets_floor` pair so any future native capability gate (not just Node)
//! can reuse the exact same floor-comparison semantics.

/// A parsed `major.minor.patch` version triple. Mirrors the JS
/// `parseMajorMinor` return shape (`{ major, minor, patch }`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

/// Parse a version string like `"v22.22.3"`, `"22.22"`, or `"22"` into a
/// [`RuntimeVersion`]. Mirrors `parseMajorMinor`: an optional leading `v`,
/// required major.minor, optional patch (defaults to 0). Returns `None` for
/// unparseable input, matching the JS `null` return.
pub fn parse_major_minor(version: &str) -> Option<RuntimeVersion> {
    let s = version.trim();
    let s = s.strip_prefix('v').unwrap_or(s);

    let chars = s.char_indices();
    // major: one or more digits
    let major_end = chars
        .clone()
        .find(|(_, c)| !c.is_ascii_digit())
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    if major_end == 0 {
        return None;
    }
    let major: u64 = s[..major_end].parse().ok()?;

    let rest = &s[major_end..];
    let rest = rest.strip_prefix('.')?;

    let minor_end = rest
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit())
        .map(|(i, _)| i)
        .unwrap_or(rest.len());
    if minor_end == 0 {
        return None;
    }
    let minor: u64 = rest[..minor_end].parse().ok()?;

    let after_minor = &rest[minor_end..];
    let patch: u64 = if let Some(patch_str) = after_minor.strip_prefix('.') {
        let patch_end = patch_str
            .char_indices()
            .find(|(_, c)| !c.is_ascii_digit())
            .map(|(i, _)| i)
            .unwrap_or(patch_str.len());
        if patch_end == 0 {
            0
        } else {
            patch_str[..patch_end].parse().unwrap_or(0)
        }
    } else {
        0
    };

    Some(RuntimeVersion { major, minor, patch })
}

/// Minimal semver floor parsed from a range string like `">=22.22.3"`,
/// `">=22.22"`, or `">=22"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemverFloor {
    pub min_major: u64,
    pub min_minor: u64,
    pub min_patch: u64,
}

/// Parse a `>=X[.Y[.Z]]` floor range. Returns `None` when the range does
/// not contain a `>=` clause, matching the JS `false` fallback path in
/// `supportsCurrent` (callers should treat `None` the same way: unsupported).
pub fn parse_floor(range: &str) -> Option<SemverFloor> {
    let idx = range.find(">=")?;
    let rest = range[idx + 2..].trim_start();
    let digits_only = |s: &str| -> (u64, usize) {
        let end = s
            .char_indices()
            .find(|(_, c)| !c.is_ascii_digit())
            .map(|(i, _)| i)
            .unwrap_or(s.len());
        (s[..end].parse().unwrap_or(0), end)
    };
    let (min_major, consumed) = digits_only(rest);
    if consumed == 0 {
        return None;
    }
    let mut cursor = &rest[consumed..];
    let min_minor = if let Some(after_dot) = cursor.strip_prefix('.') {
        let (v, c) = digits_only(after_dot);
        cursor = &after_dot[c..];
        v
    } else {
        0
    };
    let min_patch = if let Some(after_dot) = cursor.strip_prefix('.') {
        let (v, _) = digits_only(after_dot);
        v
    } else {
        0
    };
    Some(SemverFloor {
        min_major,
        min_minor,
        min_patch,
    })
}

/// Minimal semver floor check: supports `>=X.Y.Z`, `>=X.Y`, `>=X` and
/// ignores pre-release tags. Mirrors `supportsCurrent`.
pub fn supports_current(version: &str, range: &str) -> bool {
    let parsed = match parse_major_minor(version) {
        Some(p) => p,
        None => return false,
    };
    let floor = match parse_floor(range) {
        Some(f) => f,
        None => return false,
    };
    if parsed.major != floor.min_major {
        return parsed.major > floor.min_major;
    }
    if parsed.minor != floor.min_minor {
        return parsed.minor > floor.min_minor;
    }
    parsed.patch >= floor.min_patch
}
