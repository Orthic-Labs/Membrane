//! Cortex durable-memory CLI entry point.

fn print_help() {
    println!("Cortex durable-memory CLI\n\nCommands:");
    println!(
        "  {}",
        membrane_runtime::cli::CORTEX_DURABLE_COMMANDS.join(", ")
    );
    println!("\nPull, Push, Guide, Blueprint, Adapt, & service orchestration belong to Membrane.");
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let command = argv.get(1).map(String::as_str);
    if command.is_none() || matches!(command, Some("--help" | "-h")) {
        print_help();
        return;
    }
    if matches!(command, Some("--version" | "-V")) {
        println!("cortex {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let refs = argv.iter().map(String::as_str).collect::<Vec<_>>();
    if let Err(error) = membrane_runtime::cli::run_cortex_durable_cli_from(&refs) {
        eprintln!("cortex: {error}");
        std::process::exit(1);
    }
}
