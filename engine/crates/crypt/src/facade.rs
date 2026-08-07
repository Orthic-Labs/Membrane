//! Crypt compatibility facade — migration-notice helpers.
//!
//! MBR-107: every public function on the legacy `crypt` surface emits a measured
//! migration notice on first use. The notice wording is owned by
//! [`membrane_runtime::vocabulary`] so the legacy `crypt` binary and the new
//! `membrane` binary cannot drift.
//!
//! The notice is a single structured log line on stderr. See
//! [`membrane_runtime::vocabulary::CRYPT_FACADE_MIGRATION_NOTICE`] for the
//! exact wording — the facade intentionally does not embed the literal text
//! so the vocabulary module remains the single source of truth.
//!
//! It is emitted at most once per process. The guard is keyed on the crypt
//! surface so subsequent calls within the same process are silent.

use membrane_runtime::vocabulary::{
    emit_facade_notice_once, ProductSurface, CRYPT_FACADE_MIGRATION_NOTICE,
    MEMBRANE_PRODUCT_SURFACE_NOTICE,
};

/// Emit the crypt compatibility facade migration notice exactly once per
/// process. Returns `true` if this call performed the emission, `false` if it
/// was already emitted for this surface.
///
/// Public callers can invoke this from any entry point they own. The binary
/// entry points (`main.rs`, `crypt_service_main.rs`) call it automatically.
pub fn ensure_migration_notice() -> bool {
    emit_facade_notice_once(ProductSurface::Crypt)
}

/// Canonical text of the crypt facade migration notice (without log-format
/// decoration). Exposed so tests and operators can match the exact wording.
pub fn migration_notice_text() -> &'static str {
    CRYPT_FACADE_MIGRATION_NOTICE
}

/// Pass-through to the runtime's crypt notice text. Lets the facade own the
/// public surface for `crypt::*` callers without forcing them to reach into
/// `membrane_runtime::vocabulary` directly.
pub fn crypt_migration_notice_text() -> &'static str {
    CRYPT_FACADE_MIGRATION_NOTICE
}

/// Pass-through to the runtime's membrane product-surface notice text.
pub fn membrane_product_surface_notice_text() -> &'static str {
    MEMBRANE_PRODUCT_SURFACE_NOTICE
}

/// Convenience: re-export the product-surface enum so callers do not need to
/// import `membrane_runtime` directly to talk about surfaces.
pub use membrane_runtime::vocabulary::ProductSurface as Surface;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_notice_text_matches_canonical_vocabulary() {
        assert_eq!(migration_notice_text(), CRYPT_FACADE_MIGRATION_NOTICE,);
    }

    #[test]
    fn crypt_and_membrane_notice_pass_throughs_are_independent() {
        assert_ne!(
            crypt_migration_notice_text(),
            membrane_product_surface_notice_text()
        );
    }

    #[test]
    fn surface_is_a_passthrough_to_runtime_vocabulary() {
        assert_eq!(Surface::Crypt.as_str(), ProductSurface::Crypt.as_str());
        assert_eq!(
            Surface::Membrane.as_str(),
            ProductSurface::Membrane.as_str()
        );
    }
}
