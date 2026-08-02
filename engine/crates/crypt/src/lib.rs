//! Crypt compatibility facade.
//!
//! Durable primitives live in Crypt crates; federation, freshness, and planning live in
//! `membrane-runtime`. Existing consumers retain the `crypt` crate path during migration.

pub use membrane_runtime::*;
