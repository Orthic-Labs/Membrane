// Test-only driver for engine/crates/membrane-runtime/tests/hook_leaf_containment.rs.
// It reads one HookHost JSON payload from stdin, runs it through the same
// in-process native dispatcher the resident `/hook` route uses, and prints
// the single JSON response. It exists so containment tests can set a hostile
// process environment (fake `git.exe` on PATH, stub resident port) without
// mutating the test process's own environment. It must never ship.
use std::io::Read;

fn main() {
    let mut body = Vec::new();
    if std::io::stdin().take(2 * 1024 * 1024 + 1).read_to_end(&mut body).is_err() {
        eprintln!("hook-test-driver: read payload");
        std::process::exit(1);
    }
    let payload: serde_json::Value = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
    let response = membrane_runtime::hook::run_hook_payload(payload);
    match serde_json::to_string(&response) {
        Ok(encoded) => println!("{encoded}"),
        Err(error) => {
            eprintln!("hook-test-driver: serialize response: {error}");
            std::process::exit(1);
        }
    }
}
