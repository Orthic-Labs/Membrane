//! Canonical product vocabulary.
//!
//! This module is the single source of truth for the strings the product surface
//! emits to scripts, callers, and operators. The legacy `crypt` binary and the
//! new `membrane` binary both read from here, so the facade notice and the
//! product-surface notice cannot drift.
//!
//! MBR-107: keep the Crypt compatibility facade while renaming the product
//! surface. Existing scripts continue to work and emit a measured migration
//! notice without vocabulary drift.

use std::sync::atomic::{AtomicBool, Ordering};

/// The product surface a process is currently presenting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProductSurface {
    /// The legacy `crypt` binary / crate. Kept as a compatibility facade.
    Crypt,
    /// The native `membrane` binary / crate. The new product surface.
    Membrane,
}

impl ProductSurface {
    /// Lower-case identifier used in structured log lines and machine-readable notices.
    pub const fn as_str(self) -> &'static str {
        match self {
            ProductSurface::Crypt => "crypt",
            ProductSurface::Membrane => "membrane",
        }
    }
}

/// Canonical migration notice emitted by the `crypt` compatibility facade on first
/// use. Existing scripts keep working; operators see a structured log line telling
/// them to switch to `membrane` for new code.
pub const CRYPT_FACADE_MIGRATION_NOTICE: &str =
    "crypt compatibility facade active; use membrane in new code";

/// Canonical product-surface notice emitted by the `membrane` binary on first
/// invocation of the `cli` mode. Tells operators the legacy `crypt` binary is a
/// compatibility facade.
pub const MEMBRANE_PRODUCT_SURFACE_NOTICE: &str =
    "this binary is membrane; the legacy crypt binary is a compatibility facade";

/// Canonical structured log line emitted by the `crypt` facade on first use.
/// Format: `level=notice surface=<crypt|membrane> msg="<text>"`.
/// `level` and `surface` are fixed keys so scripts can grep deterministically.
pub const NOTICE_LOG_LEVEL: &str = "notice";
pub const NOTICE_LOG_SURFACE_KEY: &str = "surface";
pub const NOTICE_LOG_LEVEL_KEY: &str = "level";
pub const NOTICE_LOG_MSG_KEY: &str = "msg";

/// Format the canonical structured log line for a given surface and message.
/// The crypt facade and the membrane binary both call this so the log shape
/// is identical and cannot drift.
pub fn format_notice_log_line(surface: ProductSurface, message: &str) -> String {
    format!(
        "{key_level}={level} {key_surface}={surface} {key_msg}=\"{msg}\"",
        key_level = NOTICE_LOG_LEVEL_KEY,
        level = NOTICE_LOG_LEVEL,
        key_surface = NOTICE_LOG_SURFACE_KEY,
        surface = surface.as_str(),
        key_msg = NOTICE_LOG_MSG_KEY,
        msg = message,
    )
}

/// State of the process-wide "emit the facade notice once" guard.
/// Three distinct slots so the crypt facade and the membrane binary can never
/// suppress each other's notice.
#[derive(Debug)]
struct FacadeNoticeState {
    crypt: AtomicBool,
    membrane: AtomicBool,
}

impl FacadeNoticeState {
    const fn new() -> Self {
        Self {
            crypt: AtomicBool::new(false),
            membrane: AtomicBool::new(false),
        }
    }
}

static NOTICE_STATE: std::sync::OnceLock<FacadeNoticeState> = std::sync::OnceLock::new();

fn notice_state() -> &'static FacadeNoticeState {
    NOTICE_STATE.get_or_init(FacadeNoticeState::new)
}

/// Emit the migration notice for the requested surface at most once per process.
///
/// Returns `true` if this call performed the emission; `false` if the notice was
/// already emitted for that surface earlier in the same process. The returned
/// flag is observable for tests that assert "exactly once per process".
///
/// The emission goes to stderr as a single structured log line so it is visible
/// to operators without polluting stdout (which scripts parse).
pub fn emit_facade_notice_once(surface: ProductSurface) -> bool {
    let flag = match surface {
        ProductSurface::Crypt => &notice_state().crypt,
        ProductSurface::Membrane => &notice_state().membrane,
    };
    // compare_exchange on false -> true: only the first caller wins.
    if flag
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        let message = match surface {
            ProductSurface::Crypt => CRYPT_FACADE_MIGRATION_NOTICE,
            ProductSurface::Membrane => MEMBRANE_PRODUCT_SURFACE_NOTICE,
        };
        let line = format_notice_log_line(surface, message);
        eprintln!("{line}");
        true
    } else {
        false
    }
}

/// Has the crypt facade notice already been emitted in this process?
/// Test-only observation handle.
pub fn crypt_notice_emitted() -> bool {
    notice_state().crypt.load(Ordering::SeqCst)
}

/// Has the membrane product-surface notice already been emitted in this process?
/// Test-only observation handle.
pub fn membrane_notice_emitted() -> bool {
    notice_state().membrane.load(Ordering::SeqCst)
}

/// Canonical text for the crypt facade notice (without formatting).
pub fn crypt_migration_notice_text() -> &'static str {
    CRYPT_FACADE_MIGRATION_NOTICE
}

/// Canonical text for the membrane product-surface notice (without formatting).
pub fn membrane_product_surface_notice_text() -> &'static str {
    MEMBRANE_PRODUCT_SURFACE_NOTICE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_surface_identifiers_are_stable() {
        assert_eq!(ProductSurface::Crypt.as_str(), "crypt");
        assert_eq!(ProductSurface::Membrane.as_str(), "membrane");
    }

    #[test]
    fn notice_constants_are_the_canonical_strings() {
        assert_eq!(
            crypt_migration_notice_text(),
            "crypt compatibility facade active; use membrane in new code"
        );
        assert_eq!(
            membrane_product_surface_notice_text(),
            "this binary is membrane; the legacy crypt binary is a compatibility facade"
        );
    }

    #[test]
    fn format_includes_level_surface_and_msg_keys() {
        let line = format_notice_log_line(ProductSurface::Crypt, CRYPT_FACADE_MIGRATION_NOTICE);
        assert!(line.contains("level=notice"));
        assert!(line.contains("surface=crypt"));
        assert!(line.contains("msg=\""));
        assert!(line.contains(CRYPT_FACADE_MIGRATION_NOTICE));
    }

    #[test]
    fn crypt_and_membrane_notices_have_independent_guards() {
        // Each surface's first emission returns true; the second is false. The
        // test order is deterministic only within one binary; the guard is
        // process-wide by construction.
        let _ = emit_facade_notice_once(ProductSurface::Crypt);
        // The second call for the same surface must be a no-op:
        assert!(!emit_facade_notice_once(ProductSurface::Crypt));
        // Membrane's guard is independent and may emit independently:
        let _ = emit_facade_notice_once(ProductSurface::Membrane);
        assert!(!emit_facade_notice_once(ProductSurface::Membrane));
    }
}
