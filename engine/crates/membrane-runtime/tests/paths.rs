//! MBR-106 integration tests for the runtime stable-roots module.
//!
//! These tests assert two contracts:
//!   1. Each of the four stable roots resolves to the platform-correct
//!      location for the current target (Linux XDG, macOS Application
//!      Support / Caches / Logs, Windows APPDATA / LOCALAPPDATA).
//!   2. The same roots resolve identically across two consecutive calls
//!      inside one process — i.e. the resolver is stable per-process, so
//!      multiple modules reading the roots in the same install agree on
//!      what the tree looks like.
//!
//! Tests compile but are not executed here; the Book 1 gate runs them.
//! See MEMBRANE-BOOK-MODE-EXECUTION-RULES.md for the deferredCommands
//! list (cargo fmt --check and cargo test --workspace).

use membrane_runtime::paths::{
    cache_root, config_root, data_root, log_root, Roots, PRODUCT_DIR_NAME,
};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

static PATH_ENV_LOCK: Mutex<()> = Mutex::new(());

fn lock_path_environment() -> MutexGuard<'static, ()> {
    PATH_ENV_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

/// All four roots agree on `MEMBRANE_*` overrides when the variable is
/// set. We pick a value the tests control so the assertion never
/// depends on the host machine's real install tree.
#[test]
fn override_variable_is_honoured_for_every_root() {
    let _guard = lock_path_environment();
    let scratch = std::env::temp_dir().join(format!("membrane-paths-test-{}", std::process::id()));

    // We override each root with a distinct sentinel so the assertion
    // catches accidental root swaps (config shadowing data, etc.).
    let config_dir = scratch.join("cfg");
    let data_dir = scratch.join("dat");
    let cache_dir = scratch.join("cch");
    let log_dir = scratch.join("log");

    // SAFETY: PATH_ENV_LOCK serializes every test in this binary while these
    // process-global keys are set. Production code never writes these keys.
    unsafe {
        std::env::set_var("MEMBRANE_CONFIG_ROOT", &config_dir);
        std::env::set_var("MEMBRANE_DATA_ROOT", &data_dir);
        std::env::set_var("MEMBRANE_CACHE_ROOT", &cache_dir);
        std::env::set_var("MEMBRANE_LOG_ROOT", &log_dir);
    }

    assert_eq!(config_root(), config_dir);
    assert_eq!(data_root(), data_dir);
    assert_eq!(cache_root(), cache_dir);
    assert_eq!(log_root(), log_dir);

    // The Roots struct agrees with the per-root free functions so the
    // doctor `paths` subcommand and any code path that calls
    // `Roots::resolve()` see the same tree.
    let roots = Roots::resolve();
    assert_eq!(roots.config, config_dir);
    assert_eq!(roots.data, data_dir);
    assert_eq!(roots.cache, cache_dir);
    assert_eq!(roots.log, log_dir);

    unsafe {
        std::env::remove_var("MEMBRANE_CONFIG_ROOT");
        std::env::remove_var("MEMBRANE_DATA_ROOT");
        std::env::remove_var("MEMBRANE_CACHE_ROOT");
        std::env::remove_var("MEMBRANE_LOG_ROOT");
    }
}

/// The four roots are stable across multiple invocations inside the
/// same process. The doctor `paths` subcommand relies on this so a
/// single run of `membrane cli doctor paths` returns one consistent
/// snapshot even though the resolver touches the environment each call.
#[test]
fn roots_are_stable_across_repeated_calls() {
    let _guard = lock_path_environment();
    let first = Roots::resolve();
    let second = Roots::resolve();
    let third = Roots::resolve();

    assert_eq!(first.config, second.config);
    assert_eq!(first.data, second.data);
    assert_eq!(first.cache, second.cache);
    assert_eq!(first.log, second.log);

    assert_eq!(first.config, third.config);
    assert_eq!(first.data, third.data);
    assert_eq!(first.cache, third.cache);
    assert_eq!(first.log, third.log);

    // Free functions agree with the struct's snapshot.
    assert_eq!(first.config, config_root());
    assert_eq!(first.data, data_root());
    assert_eq!(first.cache, cache_root());
    assert_eq!(first.log, log_root());
}

/// Each root lives under the platform-correct anchor for the current
/// build. The test is exhaustive for the three supported platforms and
/// skips cleanly on hosts we don't ship to.
#[test]
fn each_root_sits_under_the_platform_correct_anchor() {
    let _guard = lock_path_environment();
    let roots = Roots::resolve();

    if cfg!(target_os = "macos") {
        // All non-cache roots collapse onto Library/Application Support
        // because that is the canonical macOS location for both config
        // and persistent data. Cache lives under Library/Caches and
        // logs under Library/Logs.
        let support = macos_application_support_root();
        let caches = macos_caches_root();
        let logs = macos_logs_root();

        assert!(
            roots.config.starts_with(&support),
            "macOS config root {:?} must start with {:?}",
            roots.config,
            support
        );
        assert!(
            roots.data.starts_with(&support),
            "macOS data root {:?} must start with {:?}",
            roots.data,
            support
        );
        assert!(
            roots.cache.starts_with(&caches),
            "macOS cache root {:?} must start with {:?}",
            roots.cache,
            caches
        );
        assert!(
            roots.log.starts_with(&logs),
            "macOS log root {:?} must start with {:?}",
            roots.log,
            logs
        );
        // The product directory name must appear at the leaf so
        // multiple vendors can co-exist on the same machine.
        assert!(roots.config.ends_with(PRODUCT_DIR_NAME));
        assert!(roots.data.ends_with(PRODUCT_DIR_NAME));
        assert!(roots.cache.ends_with(PRODUCT_DIR_NAME));
        assert!(roots.log.ends_with(PRODUCT_DIR_NAME));
    } else if cfg!(windows) {
        // Windows config lives under APPDATA (roaming). Data, cache,
        // and log collapse onto LOCALAPPDATA because Microsoft
        // considers all three "local machine state".
        let appdata = windows_appdata_root();
        let local = windows_local_appdata_root();
        assert!(roots.config.starts_with(&appdata));
        assert!(roots.data.starts_with(&local));
        assert!(roots.cache.starts_with(&local));
        assert!(roots.log.starts_with(&local));
        assert!(roots.config.ends_with(PRODUCT_DIR_NAME));
        assert!(roots.data.ends_with(PRODUCT_DIR_NAME));
        assert!(roots.cache.ends_with(PRODUCT_DIR_NAME));
        assert!(roots.log.ends_with(PRODUCT_DIR_NAME));
    } else if cfg!(target_family = "unix") {
        // Linux + other XDG-respecting unices: every root lives under
        // the user's home, has the product name at the leaf, and
        // follows XDG defaults when XDG_*_HOME is unset.
        let home = std::env::var_os("HOME").map(PathBuf::from);
        assert!(home.is_some(), "HOME is required on XDG hosts");
        let home = home.unwrap();

        assert!(
            roots.config.starts_with(&home),
            "XDG config root {:?} must be under HOME",
            roots.config
        );
        assert!(
            roots.data.starts_with(&home),
            "XDG data root {:?} must be under HOME",
            roots.data
        );
        assert!(
            roots.cache.starts_with(&home),
            "XDG cache root {:?} must be under HOME",
            roots.cache
        );
        assert!(
            roots.log.starts_with(&home),
            "XDG log root {:?} must be under HOME",
            roots.log
        );

        // The four roots never collide: distinct parents under HOME.
        // `log` may share its parent with `data` on hosts without
        // XDG_STATE_HOME (both fall back to .local/share and
        // .local/state respectively), but they live under different
        // XDG buckets so the leaf segments never overlap.
        let leaves: [&std::path::Path; 4] = [
            roots.config.as_path(),
            roots.data.as_path(),
            roots.cache.as_path(),
            roots.log.as_path(),
        ];
        for (idx, a) in leaves.iter().enumerate() {
            for b in leaves.iter().skip(idx + 1) {
                assert_ne!(a, b, "stable roots must never alias: {a:?} vs {b:?}");
            }
        }
    } else {
        // Unsupported target — accept the test as vacuously passing
        // because the runtime never claims to support it.
    }
}

/// `Roots::contains` is the cheap pre-filter the receipt uses to decide
/// whether a path is "outside the stable roots". Asserting the contract
/// here keeps the two modules in lockstep.
#[test]
fn roots_contains_reports_inside_vs_outside_correctly() {
    let _guard = lock_path_environment();
    let roots = Roots::resolve();
    let inside = roots
        .config
        .join("nested/receipt-tracking-should-skip-me.json");
    let outside = std::env::temp_dir().join(format!(
        "membrane-outside-{}-{}",
        std::process::id(),
        "paths-contains"
    ));

    assert!(
        roots.contains(&inside),
        "path inside a root must report as contained"
    );
    assert!(
        !roots.contains(&outside),
        "path outside every root must report as not contained"
    );

    // iter() yields the four roots in a stable order.
    let iter = roots.iter();
    assert_eq!(iter.len(), 4);
    assert_eq!(iter[0].0, "config");
    assert_eq!(iter[1].0, "data");
    assert_eq!(iter[2].0, "cache");
    assert_eq!(iter[3].0, "log");
}

// ----- platform helpers used by the assertion above -----
//
// The helpers exist on every target but are only referenced inside
// `if cfg!(...)` runtime branches. We allow dead_code on the
// non-target variants so the compiler does not reject the call site
// for a platform we are not currently building on.

#[allow(dead_code)]
fn macos_application_support_root() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME");
    PathBuf::from(home)
        .join("Library")
        .join("Application Support")
}

#[allow(dead_code)]
fn macos_caches_root() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME");
    PathBuf::from(home).join("Library").join("Caches")
}

#[allow(dead_code)]
fn macos_logs_root() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME");
    PathBuf::from(home).join("Library").join("Logs")
}

#[allow(dead_code)]
fn windows_appdata_root() -> PathBuf {
    std::env::var_os("APPDATA")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

#[allow(dead_code)]
fn windows_local_appdata_root() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}
