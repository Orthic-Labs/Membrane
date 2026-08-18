use membrane::{check_parity, dispatch::cli_parser_snapshot, generate_cli_subcommands};
use membrane_protocol::operations::{OperationsIndex, OPERATIONS};
use std::{collections::BTreeSet, fs, path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("cli-parity: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode, String> {
    if std::env::args().nth(1).as_deref() == Some("--generate") {
        print!("{}", generate_cli_subcommands(OPERATIONS));
        return Ok(ExitCode::SUCCESS);
    }

    let index_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../../schemas/registry/operations/operations-index.v1.golden.json");
    let index: OperationsIndex = serde_json::from_str(
        &fs::read_to_string(&index_path)
            .map_err(|error| format!("read {}: {error}", index_path.display()))?,
    )
    .map_err(|error| format!("parse {}: {error}", index_path.display()))?;

    let index_names: BTreeSet<&str> = index.names().collect();
    let spec_names: BTreeSet<&str> = OPERATIONS.iter().map(|spec| spec.id).collect();
    let mut report = check_parity(OPERATIONS, cli_parser_snapshot);

    for name in index_names.difference(&spec_names) {
        report.extra.push(format!("registry:{name}"));
    }
    for name in spec_names.difference(&index_names) {
        report.missing.push(format!("registry:{name}"));
    }
    report.extra.sort();
    report.extra.dedup();
    report.missing.sort();
    report.missing.dedup();

    println!(
        "{}",
        serde_json::to_string_pretty(&report)
            .map_err(|error| format!("serialize parity report: {error}"))?
    );
    Ok(if report.is_drift_free() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
