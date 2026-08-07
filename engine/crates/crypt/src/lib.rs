//! Crypt compatibility facade — public surface.
//!
//! MBR-107 turns the legacy `crypt` crate into a thin compatibility facade so
//! existing scripts continue to work without vocabulary drift. Every public
//! function exposed here (including the binary entry points in `main.rs` and
//! `crypt_service_main.rs`) emits a measured migration notice on first use.
//!
//! The notice is sourced from [`membrane_runtime::vocabulary`], which is the
//! single source of truth for the product-surface vocabulary. The legacy
//! `crypt` binary and the new `membrane` binary both read from that module, so
//! the wording cannot drift.
//!
//! Existing public items are preserved unchanged: the wildcard re-export keeps
//! every signature identical to the underlying `membrane_runtime` items, so
//! downstream code that imported `crypt::foo` keeps compiling and running.

// Re-export the existing public surface unchanged. The wildcard is intentional:
// the facade is a thin compatibility layer, and rewriting the re-exports would
// risk subtle drift. The facade helpers below add the migration notice on top.
pub use membrane_runtime::*;

// Compatibility facade helpers. These are the new public items introduced by
// MBR-107. They are additive; nothing existing was renamed or removed.
pub mod facade;
