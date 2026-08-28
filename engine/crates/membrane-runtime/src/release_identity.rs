//! Stable release identity and per-process boot identity.

use sha2::{Digest, Sha256};
use std::sync::OnceLock;

/// Stable across every process built from the same verified Membrane source tree.
pub fn release_generation() -> String {
    format!(
        "sha256:{}",
        option_env!("MEMBRANE_SOURCE_TREE_SHA256").unwrap_or("unknown")
    )
}

/// Native release target encoded without relying on Cargo's build-script-only `TARGET` value.
pub const fn target_triple() -> &'static str {
    if cfg!(all(
        target_arch = "x86_64",
        target_os = "windows",
        target_env = "msvc"
    )) {
        "x86_64-pc-windows-msvc"
    } else if cfg!(all(
        target_arch = "aarch64",
        target_os = "windows",
        target_env = "msvc"
    )) {
        "aarch64-pc-windows-msvc"
    } else if cfg!(all(target_arch = "x86_64", target_os = "macos")) {
        "x86_64-apple-darwin"
    } else if cfg!(all(target_arch = "aarch64", target_os = "macos")) {
        "aarch64-apple-darwin"
    } else {
        "unsupported"
    }
}

/// Random per process. This remains separate from release identity so restarts are observable.
pub fn service_generation() -> &'static str {
    static GENERATION: OnceLock<String> = OnceLock::new();
    GENERATION
        .get_or_init(|| {
            let mut random = [0u8; 32];
            if getrandom::fill(&mut random).is_ok() {
                format!("sha256:{}", hex::encode(Sha256::digest(random)))
            } else {
                format!(
                    "sha256:{}",
                    hex::encode(Sha256::digest(
                        format!("{}:{}", std::process::id(), crate::time::now_iso()).as_bytes()
                    ))
                )
            }
        })
        .as_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_generation_is_derived_from_compile_time_tree_identity() {
        assert_eq!(
            release_generation(),
            format!(
                "sha256:{}",
                option_env!("MEMBRANE_SOURCE_TREE_SHA256").unwrap_or("unknown")
            )
        );
    }

    #[test]
    fn service_generation_is_stable_within_one_process_and_not_the_release() {
        assert_eq!(service_generation(), service_generation());
        assert_ne!(service_generation(), release_generation());
    }

    #[test]
    fn target_triple_is_a_supported_native_release_target() {
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        assert!(matches!(
            target_triple(),
            "x86_64-pc-windows-msvc"
                | "aarch64-pc-windows-msvc"
                | "x86_64-apple-darwin"
                | "aarch64-apple-darwin"
        ));
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        assert_eq!(target_triple(), "unsupported");
    }
}
