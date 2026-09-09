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

/// LC-04 + NCL-05: explicit CLI operations must succeed with Hub off and no
/// CodeRight holder, and must execute through the native binary only — never
/// falling back to an interpreter shim. Stripping `PATH` to a directory that
/// contains no `python`/`node`/`sh` and removing every Hub/runtime-origin
/// discovery variable proves both the no-holder path and the native-only
/// contract in one run; a shim fallback would fail to spawn at all once the
/// interpreter is unreachable.
#[test]
fn explicit_cli_status_succeeds_with_no_holder_and_no_interpreter_on_path() {
    use std::process::{Command, Stdio};

    // With `WORKSPACE_ROOT` and `CORTEX_DB` both stripped (as this contract requires), db
    // resolution lands on `cli.rs::dev_workspace_db_path`, which only fires when the canonical
    // `tools/.cache/memory/` directory already exists in this checkout — it then creates and
    // migrates the db itself, with no Hub, holder, or interpreter involved. That directory is
    // gitignored (`.gitignore:17` `.cache/`), so it is present on a developer machine and
    // absent on a fresh CI checkout; creating it here makes the test depend on the product's
    // own fallback rather than on untracked local state. It does not weaken the negative
    // control: `PATH` stays stripped of every interpreter and the holder variables stay removed.
    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .and_then(std::path::Path::parent)
        .expect("engine/crates/membrane resolves to the repository root");
    std::fs::create_dir_all(workspace_root.join("tools/.cache/memory"))
        .expect("canonical dev workspace cache directory");

    let output = Command::new(env!("CARGO_BIN_EXE_membrane"))
        .args(["cli", "doctor"])
        .env("PATH", "/__membrane_no_interpreters__")
        .env_remove("PYTHONPATH")
        .env_remove("MEMBRANE_PORT")
        .env_remove("MEMBRANE_API_TOKEN")
        .env_remove("MEMBRANE_API_TOKEN_FILE")
        .env_remove("WORKSPACE_ROOT")
        .env_remove("CORTEX_DB")
        .stdin(Stdio::null())
        .output()
        .expect("native membrane binary starts without any interpreter on PATH");
    assert!(
        output.status.success(),
        "explicit `cli doctor` must succeed with Hub off and no interpreter reachable: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Packaging's runtime gate (`apps/membrane-hub/scripts/package-portable-windows.mjs`) spawns
/// `membrane hook --help` and requires exit status 0 as proof the shipped binary carries native
/// hook authority. clap reports `--help`/`--version` as an `Err` internally; `parse_mode` must
/// route those to stdout + exit 0 like any well-behaved CLI, not surface them as a parse
/// failure. This pins that contract at every level clap can produce a help/version display, plus
/// the real parse-error path staying on stderr with a non-zero, non-zero-but-not-0 exit code.
#[test]
fn help_and_version_exit_zero_on_stdout_at_every_level() {
    use std::process::{Command, Stdio};

    for args in [
        ["--help"].as_slice(),
        ["--version"].as_slice(),
        ["hook", "--help"].as_slice(),
        ["cli", "--help"].as_slice(),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_membrane"))
            .args(args)
            .stdin(Stdio::null())
            .output()
            .unwrap_or_else(|error| panic!("spawn `membrane {}`: {error}", args.join(" ")));
        assert_eq!(
            output.status.code(),
            Some(0),
            "`membrane {}` must exit 0: stdout={} stderr={}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !output.stdout.is_empty(),
            "`membrane {}` must print help/version text to stdout",
            args.join(" ")
        );
    }
}

/// The other half of the same contract: a genuine parse error (not `--help`/`--version`) must
/// still land on stderr with the non-zero user-error exit code, unaffected by routing help and
/// version to a successful exit.
#[test]
fn genuine_parse_errors_still_fail_with_nonzero_exit_on_stderr() {
    use std::process::{Command, Stdio};

    let output = Command::new(env!("CARGO_BIN_EXE_membrane"))
        .args(["wat"])
        .stdin(Stdio::null())
        .output()
        .expect("spawn `membrane wat`");
    assert_eq!(
        output.status.code(),
        Some(2),
        "`membrane wat` must fail as an unrecognized subcommand: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty(), "no help text belongs on stdout for a real parse error");
    assert!(!output.stderr.is_empty(), "the parse error must be reported on stderr");
}

#[test]
fn shipped_supervisor_config_is_schema_v2_without_watcher_policy() {
    let config = include_str!("../../../../dist/install/config.example.json");
    assert!(config.contains("\"schemaVersion\": 2"));
    assert!(!config.contains("watcherPolicy"));
    assert!(!config.contains("watchman.pid"));
}
