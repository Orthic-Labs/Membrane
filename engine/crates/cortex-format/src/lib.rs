//! cortex-format — Open Knowledge Format (OKF) bundle support and deterministic prose compression.
//!
//! Extracted from the config crate during the R0.2 workspace migration.
//! The `InstructionLoader::load_okf_bundle` method and policy-dependent
//! sanitization were excluded; they remain in CodeRight's config crate.

pub mod okf;

pub use okf::{
    compress_prose, emit_bundle, parse_bundle, OkfBundle, OkfBundleStats, OkfConcept,
    OkfEmitOptions, OkfError, OkfLink, OKF_VERSION,
};
