//! Stable, platform-correct roots for the Membrane runtime.
//!
//! MBR-106: standardize stable config, data, cache, and log roots.
//!
//! These four roots are the single source of truth for every persistent
//! directory the runtime reads or writes during normal operation. Install,
//! update, rollback, and uninstall all anchor on this module so user data
//! survives an update and uninstall removes only what the runtime itself
//! wrote outside these roots (tracked in `receipt`).
//!
//! ## Platform mapping
//!
//! | Root   | Linux (XDG)                          | macOS                                      | Windows                  |
//! |--------|--------------------------------------|--------------------------------------------|--------------------------|
//! | config | `$XDG_CONFIG_HOME/membrane`          | `~/Library/Application Support/Membrane`   | `%APPDATA%\Membrane`     |
//! | data   | `$XDG_DATA_HOME/membrane`            | `~/Library/Application Support/Membrane`   | `%LOCALAPPDATA%\Membrane`|
//! | cache  | `$XDG_CACHE_HOME/membrane`           | `~/Library/Caches/Membrane`                | `%LOCALAPPDATA%\Membrane`|
//! | log    | `$XDG_STATE_HOME/membrane/log`       | `~/Library/Logs/Membrane`                  | `%LOCALAPPDATA%\Membrane`|
//!
//! ## Overrides
//!
//! Every root honors an explicit `MEMBRANE_<ROOT>` environment variable
//! so tests and operators can pin a writable scratch tree without
//! touching the user's real config. The CLI `doctor paths` subcommand
//! reports the resolved value, the override that won, and any
//! receipt-owned files recorded for the current process.

use std::path::{Path, PathBuf};

/// Name of the product as it appears under the user's data tree. Kept in
/// one place so the install docs and the runtime agree.
pub const PRODUCT_DIR_NAME: &str = "Membrane";

/// Returns the user's home directory (best effort). Prefers `HOME` on POSIX,
/// falls back to `USERPROFILE` on Windows. Returns `None` when no anchor is
/// resolvable so callers can decide how to fail (rather than silently
/// collapsing to `.`).
pub fn user_home() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    } else {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    }
}

/// Stable config root: settings, lease, supervisor config. Tiny, durable,
/// hand-edited, never thrown away.
pub fn config_root() -> PathBuf {
    if let Some(override_dir) = override_dir("MEMBRANE_CONFIG_ROOT") {
        return override_dir;
    }
    #[cfg(windows)]
    {
        return windows_appdata_root().join(PRODUCT_DIR_NAME);
    }
    #[cfg(target_os = "macos")]
    {
        return macos_application_support_root().join(PRODUCT_DIR_NAME);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // XDG_CONFIG_HOME or $HOME/.config/membrane
        return xdg_dir("XDG_CONFIG_HOME", ".config").join(PRODUCT_DIR_NAME.to_ascii_lowercase());
    }
    #[allow(unreachable_code)]
    PathBuf::from(PRODUCT_DIR_NAME)
}

/// Stable data root: persistent memory DB, checkpoints, supervisor state.
/// Survives `membrane update`; uninstall leaves it intact.
pub fn data_root() -> PathBuf {
    if let Some(override_dir) = override_dir("MEMBRANE_DATA_ROOT") {
        return override_dir;
    }
    #[cfg(windows)]
    {
        return windows_local_appdata_root().join(PRODUCT_DIR_NAME);
    }
    #[cfg(target_os = "macos")]
    {
        return macos_application_support_root().join(PRODUCT_DIR_NAME);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // XDG_DATA_HOME or $HOME/.local/share/membrane
        return xdg_dir("XDG_DATA_HOME", ".local/share")
            .join(PRODUCT_DIR_NAME.to_ascii_lowercase());
    }
    #[allow(unreachable_code)]
    PathBuf::from(PRODUCT_DIR_NAME)
}

/// Stable cache root: regenerable caches (fastembed artifacts, downloaded
/// models). Safe to wipe on uninstall.
pub fn cache_root() -> PathBuf {
    if let Some(override_dir) = override_dir("MEMBRANE_CACHE_ROOT") {
        return override_dir;
    }
    #[cfg(windows)]
    {
        return windows_local_appdata_root().join(PRODUCT_DIR_NAME);
    }
    #[cfg(target_os = "macos")]
    {
        return macos_caches_root().join(PRODUCT_DIR_NAME);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // XDG_CACHE_HOME or $HOME/.cache/membrane
        return xdg_dir("XDG_CACHE_HOME", ".cache").join(PRODUCT_DIR_NAME.to_ascii_lowercase());
    }
    #[allow(unreachable_code)]
    PathBuf::from(PRODUCT_DIR_NAME)
}

/// Stable log root: append-only log files written by the resident and
/// supervisor. Kept apart from `data_root` so the two retention policies
/// can diverge (data is forever; logs rotate).
pub fn log_root() -> PathBuf {
    if let Some(override_dir) = override_dir("MEMBRANE_LOG_ROOT") {
        return override_dir;
    }
    #[cfg(windows)]
    {
        return windows_local_appdata_root().join(PRODUCT_DIR_NAME);
    }
    #[cfg(target_os = "macos")]
    {
        return macos_logs_root().join(PRODUCT_DIR_NAME);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // XDG_STATE_HOME or $HOME/.local/state/membrane/log
        let base =
            xdg_dir("XDG_STATE_HOME", ".local/state").join(PRODUCT_DIR_NAME.to_ascii_lowercase());
        return base.join("log");
    }
    #[allow(unreachable_code)]
    PathBuf::from(PRODUCT_DIR_NAME)
}

/// All four roots in one structure so `doctor paths` and tests can
/// snapshot the entire policy in one call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roots {
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
    pub log: PathBuf,
}

impl Roots {
    /// Resolve every root under the current process's environment.
    pub fn resolve() -> Self {
        Self {
            config: config_root(),
            data: data_root(),
            cache: cache_root(),
            log: log_root(),
        }
    }

    /// List the four roots as `PathBuf` references in a fixed order.
    /// Used by the receipt audit and the doctor `paths` subcommand so
    /// they agree on iteration order.
    pub fn iter(&self) -> [(&'static str, &Path); 4] {
        [
            ("config", self.config.as_path()),
            ("data", self.data.as_path()),
            ("cache", self.cache.as_path()),
            ("log", self.log.as_path()),
        ]
    }

    /// Returns `true` when `path` lives inside any of the four roots.
    /// The receipt uses this as the cheap pre-filter before deciding
    /// whether a file is "outside the stable roots" and therefore owned.
    pub fn contains(&self, path: &Path) -> bool {
        self.iter().iter().any(|(_, root)| path.starts_with(root))
    }
}

#[allow(dead_code)] // helper is only used on non-Windows, non-macOS XDG targets
fn xdg_dir(env_var: &str, default_relative_to_home: &str) -> PathBuf {
    if let Some(raw) = std::env::var_os(env_var).filter(|value| !value.is_empty()) {
        return PathBuf::from(raw);
    }
    if let Some(home) = user_home() {
        return home.join(default_relative_to_home);
    }
    PathBuf::from(default_relative_to_home)
}

fn override_dir(env_var: &str) -> Option<PathBuf> {
    std::env::var_os(env_var)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(target_os = "macos")]
fn macos_application_support_root() -> PathBuf {
    user_home()
        .map(|home| home.join("Library").join("Application Support"))
        .unwrap_or_else(|| PathBuf::from("Library/Application Support"))
}

#[cfg(target_os = "macos")]
fn macos_caches_root() -> PathBuf {
    user_home()
        .map(|home| home.join("Library").join("Caches"))
        .unwrap_or_else(|| PathBuf::from("Library/Caches"))
}

#[cfg(target_os = "macos")]
fn macos_logs_root() -> PathBuf {
    user_home()
        .map(|home| home.join("Library").join("Logs"))
        .unwrap_or_else(|| PathBuf::from("Library/Logs"))
}

#[cfg(windows)]
fn windows_appdata_root() -> PathBuf {
    std::env::var_os("APPDATA")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| user_home())
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(windows)]
fn windows_local_appdata_root() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| user_home())
        .unwrap_or_else(|| PathBuf::from("."))
}
