//! Membrane — single signed binary dispatcher.
//!
//! Real work is delegated to `membrane_runtime`. This binary's only job is to parse the mode
//! subcommand and return the right exit code so launchers and the supervisor can act on the
//! outcome without parsing stderr.
//!
//! MBR-102: create one membrane executable with mode subcommands.

use membrane::dispatch::{parse_mode, ParsedInvocation};
use membrane::modes::{dispatch, DispatchOutcome};

fn main() {
    let args = std::env::args_os().collect::<Vec<_>>();
    if args.len() == 2 && args[1] == "--version" {
        println!("membrane {}", installed_release_version());
        std::process::exit(0);
    }
    let invocation = match parse_mode(args) {
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

fn installed_release_version() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|root| root.join("release.json")))
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|value| value.get("version").and_then(serde_json::Value::as_str).map(str::to_owned))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}

/// Re-exported so integration tests can exercise the dispatcher without re-implementing it.
pub fn dispatch_parsed(invocation: &ParsedInvocation) -> DispatchOutcome {
    dispatch(invocation)
}
