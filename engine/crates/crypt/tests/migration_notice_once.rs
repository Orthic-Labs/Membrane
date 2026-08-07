//! MBR-107: instrumentation test for the migration-notice-once guard.
//!
//! The crypt compatibility facade and the membrane binary both call
//! `membrane_runtime::vocabulary::emit_facade_notice_once(surface)`. The guard
//! must emit the structured notice exactly once per process per surface, and
//! every subsequent call must be silent. This test exercises the guard
//! directly so the contract is pinned in code rather than only in prose.
//!
//! The test uses the in-process observation handle `crypt_notice_emitted` /
//! `membrane_notice_emitted` to verify state. The Book 1 gate also runs a
//! shell-level check that captures stderr and counts the lines; this Rust test
//! complements that by checking the in-process state transitions.

#![allow(unused_imports)]

use membrane_runtime::vocabulary::{
    crypt_notice_emitted, emit_facade_notice_once, membrane_notice_emitted, ProductSurface,
};

/// First call for the crypt surface must perform the emission; subsequent
/// calls must be no-ops. We use the runtime observation handle to check the
/// state because stderr capture inside a single test binary is racy.
#[test]
fn crypt_notice_emits_on_first_call_and_is_silent_thereafter() {
    // We can't assert the *global* emission state because earlier tests in
    // this binary may have already emitted; instead, assert the contract that
    // the second call in a row returns false.
    //
    // To make that contract observable regardless of prior tests, we ask the
    // guard directly for a fresh observation of the crypt surface's behavior
    // pattern: consecutive calls return true exactly once, then always false.
    let observed_first = emit_facade_notice_once(ProductSurface::Crypt);
    // After this call, every subsequent call for the same surface returns
    // false within this process.
    let observed_second = emit_facade_notice_once(ProductSurface::Crypt);
    let observed_third = emit_facade_notice_once(ProductSurface::Crypt);
    if observed_first {
        // This was the global first call. The next two must be no-ops.
        assert!(!observed_second);
        assert!(!observed_third);
        assert!(crypt_notice_emitted());
    } else {
        // Some earlier test already emitted. Subsequent calls must still be
        // no-ops. The state flag must already be set.
        assert!(!observed_second);
        assert!(!observed_third);
        assert!(crypt_notice_emitted());
    }
}

/// Membrane surface has its own independent guard. The crypt surface's
/// emission state must not silence the membrane surface, and vice versa.
#[test]
fn crypt_and_membrane_guards_are_independent() {
    // Force both surfaces to settle: each surface's first call may emit, and
    // any later call must be silent. We don't assert which surface "won" the
    // first global emission; we only assert the independence property.
    let _ = emit_facade_notice_once(ProductSurface::Crypt);
    let _ = emit_facade_notice_once(ProductSurface::Membrane);
    // After both surfaces have been touched, both observation flags are set
    // (possibly by these calls, possibly by earlier tests).
    assert!(crypt_notice_emitted());
    assert!(membrane_notice_emitted());
}

/// The facade helper `crypt::facade::ensure_migration_notice` must share the
/// same guard as the runtime vocabulary's `emit_facade_notice_once(Crypt)`.
#[test]
fn crypt_facade_helper_uses_the_same_guard() {
    let _ = crypt::facade::ensure_migration_notice();
    // After the helper runs, the runtime's observation handle must show the
    // crypt surface as already emitted.
    assert!(crypt_notice_emitted());
}

/// The crypt facade and the membrane binary must converge on the same
/// wording for their respective notices. The runtime's vocabulary module is
/// the single source of truth.
#[test]
fn facade_and_membrane_use_the_same_vocabulary() {
    let crypt_text = crypt::facade::crypt_migration_notice_text();
    let membrane_text = crypt::facade::membrane_product_surface_notice_text();
    let runtime_crypt = membrane_runtime::vocabulary::CRYPT_FACADE_MIGRATION_NOTICE;
    let runtime_membrane = membrane_runtime::vocabulary::MEMBRANE_PRODUCT_SURFACE_NOTICE;
    assert_eq!(crypt_text, runtime_crypt);
    assert_eq!(membrane_text, runtime_membrane);
}
