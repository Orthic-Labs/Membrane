//! Membrane — single signed binary dispatcher.
//!
//! Real work is delegated to `membrane_runtime`. This binary's only job is to parse the mode
//! subcommand and return the right exit code so launchers and the supervisor can act on the
//! outcome without parsing stderr.
//!
//! MBR-102: create one membrane executable with mode subcommands.

#![cfg_attr(windows, windows_subsystem = "windows")]

use membrane::dispatch::{parse_mode, ParsedInvocation};
use membrane::modes::{dispatch, DispatchOutcome};

fn main() {
    let invocation = match parse_mode(std::env::args_os()) {
        Ok(invocation) => invocation,
        Err(error) => {
            // clap writes the formatted help to stderr and returns the error string. We add a
            // short prefix so scripts that grep for `membrane:` can route on it.
            eprintln!("membrane: {error}");
            std::process::exit(2);
        }
    };
    let outcome = dispatch(&invocation);
    match outcome {
        DispatchOutcome::Ok => std::process::exit(0),
        DispatchOutcome::UserError(error) => {
            eprintln!("membrane: {error}");
            std::process::exit(2);
        }
        DispatchOutcome::InternalError(error) => {
            eprintln!("membrane: internal: {error}");
            std::process::exit(1);
        }
    }
}

/// Re-exported so integration tests can exercise the dispatcher without re-implementing it.
pub fn dispatch_parsed(invocation: &ParsedInvocation) -> DispatchOutcome {
    dispatch(invocation)
}
