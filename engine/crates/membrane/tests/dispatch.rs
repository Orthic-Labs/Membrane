//! Integration tests for the membrane binary dispatcher.
//!
//! These exercise `parse_mode` and `dispatch` without spawning a process, so the full test runs
//! from `cargo test` inside the worktree. The deferred book-mode command list does not include
//! these — they run with the rest of the suite when the Book 1 gate executes.
//!
//! MBR-102: create one membrane executable with mode subcommands.

use membrane::dispatch::{parse_mode, MembraneMode, ParsedInvocation};
use membrane::modes::{dispatch, DispatchOutcome};

#[test]
fn every_mode_round_trips_through_parse_mode() {
    let cases = [
        (
            vec!["membrane", "cli", "doctor"],
            MembraneMode::Cli,
            vec!["doctor".to_string()],
        ),
        (
            vec!["membrane", "stdio-mcp"],
            MembraneMode::StdioMcp,
            Vec::<String>::new(),
        ),
        (
            vec!["membrane", "loopback-api", "--port", "47900"],
            MembraneMode::LoopbackApi,
            Vec::<String>::new(),
        ),
        (
            vec!["membrane", "supervisor-child", "--lease", "/tmp/lease.json"],
            MembraneMode::SupervisorChild,
            Vec::<String>::new(),
        ),
    ];
    for (argv, expected_mode, expected_tail) in cases {
        let inv: ParsedInvocation = parse_mode(argv.iter().map(|s| s.to_string()))
            .expect("parse_mode should accept every documented invocation");
        assert_eq!(inv.mode, expected_mode, "argv={:?}", argv);
        assert_eq!(inv.cli_tail, expected_tail, "argv={:?}", argv);
    }
}

#[test]
fn exit_code_table_is_stable_across_modes() {
    use membrane::{EXIT_INTERNAL_ERROR, EXIT_OK, EXIT_USER_ERROR};
    assert_eq!(DispatchOutcome::Ok.exit_code(), EXIT_OK);
    assert_eq!(
        DispatchOutcome::UserError("x".into()).exit_code(),
        EXIT_USER_ERROR
    );
    assert_eq!(
        DispatchOutcome::InternalError("y".into()).exit_code(),
        EXIT_INTERNAL_ERROR
    );
    // The constants must be the values scripts and the supervisor read.
    assert_eq!(EXIT_OK, 0);
    assert_eq!(EXIT_USER_ERROR, 2);
    assert_eq!(EXIT_INTERNAL_ERROR, 1);
}

#[test]
fn dispatch_never_panics_on_legitimate_invocation() {
    // We don't have a real runtime in tests, so dispatch returns an Err-shaped outcome.
    // What matters here is the boundary: it must never panic.
    let inv = parse_mode(["membrane", "loopback-api"].iter().copied()).unwrap();
    let _ = dispatch(&inv);
}

#[test]
fn privileged_port_is_rejected_at_parse_time() {
    let err = parse_mode(["membrane", "loopback-api", "--port", "22"].iter().copied()).unwrap_err();
    assert!(err.contains("port 22 is privileged"));
}

#[test]
fn shipped_supervisor_config_is_schema_v2_without_watcher_policy() {
    let config = include_str!("../../../../dist/install/config.example.json");
    assert!(config.contains("\"schemaVersion\": 2"));
    assert!(!config.contains("watcherPolicy"));
    assert!(!config.contains("watchman.pid"));
}
