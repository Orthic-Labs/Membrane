//! Integration tests for the membrane binary dispatcher.
//!
//! These exercise `parse_mode` and `dispatch` without spawning a process, so the full test runs
//! from `cargo test` inside the worktree. The deferred book-mode command list does not include
//! these — they run with the rest of the suite when the Book 1 gate executes.
//!
//! MBR-102: create one membrane executable with mode subcommands.

use membrane::dispatch::{parse_mode, MembraneMode, ParsedInvocation};
use membrane::modes::DispatchOutcome;

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
    ];
    for (argv, expected_mode, expected_tail) in cases {
        let inv: ParsedInvocation = parse_mode(argv.iter().map(|s| s.to_string()))
            .expect("parse_mode should accept every documented invocation");
        assert_eq!(inv.mode, expected_mode, "argv={:?}", argv);
        assert_eq!(inv.cli_tail, expected_tail, "argv={:?}", argv);
    }
}

#[test]
fn activation_defaults_to_inspection_safe_client_projection() {
    let inv = parse_mode(["membrane", "activate", "--dry-run"].iter().copied())
        .expect("activate dry-run parses");
    assert_eq!(inv.mode, MembraneMode::Activate);
    let activation = inv.activation.expect("activation payload");
    assert!(activation.clients.is_empty(), "empty means default clients");
    assert!(activation.dry_run);
    assert_eq!(activation.timeout_ms, 35_000);
}

#[test]
fn status_uses_activation_receipt_without_client_mutation() {
    let inv = parse_mode(["membrane", "status", "--dry-run"].iter().copied())
        .expect("status dry-run parses");
    assert_eq!(inv.mode, MembraneMode::Activate);
    let activation = inv.activation.expect("status payload");
    assert!(activation.status_only);
    assert!(activation.dry_run);
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
fn retired_resident_modes_are_rejected() {
    assert!(parse_mode(["membrane", "serve"].iter().copied()).is_err());
    assert!(parse_mode(["membrane", "loopback-api"].iter().copied()).is_err());
    assert!(parse_mode(["membrane", "supervisor-child"].iter().copied()).is_err());
}

#[test]
fn installed_cli_cannot_start_a_resident() {
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    for args in [["serve"].as_slice(), ["cli", "serve"].as_slice()] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_membrane"))
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn installed membrane binary");
        let deadline = Instant::now() + Duration::from_secs(2);
        let status = loop {
            if let Some(status) = child.try_wait().expect("poll membrane CLI") {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill().expect("stop unexpected resident process");
                let _ = child.wait();
                panic!(
                    "`membrane {}` started or blocked in resident mode",
                    args.join(" ")
                );
            }
            thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(
            status.code(),
            Some(2),
            "`membrane {}` must fail as a rejected public command",
            args.join(" ")
        );
    }
}

#[test]
fn shipped_supervisor_config_is_schema_v2_without_watcher_policy() {
    let config = include_str!("../../../../dist/install/config.example.json");
    assert!(config.contains("\"schemaVersion\": 2"));
    assert!(!config.contains("watcherPolicy"));
    assert!(!config.contains("watchman.pid"));
}
