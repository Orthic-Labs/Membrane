//! Lightweight installed-engine forwarding client.
//!
//! This executable owns no Membrane runtime, state, planner, or fallback path.
//! It only frames bounded HTTP requests to the installer-owned singleton.

use serde_json::{json, Value};
use membrane::dispatch::parse_mode;
use membrane::modes::{dispatch, DispatchOutcome};
use std::io::{self, BufRead, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

const DEFAULT_PORT: u16 = 47_851;
const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(30);

fn main() {
    let args = std::env::args_os().collect::<Vec<_>>();
    if args.len() == 2 && args[1] == "--version" {
        println!("membrane-client {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    // Transport modes never enter Membrane's command dispatcher: they only
    // forward to the authenticated installed engine. Activation remains here
    // as installer-owned control-plane work, not a resident runtime path.
    if args.get(1).and_then(|arg| arg.to_str()).is_some_and(|mode| {
        matches!(mode, "stdio-mcp" | "hook" | "cli")
    }) {
        if let Err(error) = run() {
            eprintln!("membrane-client: {error}");
            std::process::exit(1);
        }
        return;
    }
    let invocation = match parse_mode(args) {
        Ok(invocation) => invocation,
        Err(error) => {
            eprintln!("membrane-client: {error}");
            std::process::exit(2);
        }
    };
    match dispatch(&invocation) {
        DispatchOutcome::Ok => {}
        DispatchOutcome::UserError(error) => {
            eprintln!("membrane-client: {error}");
            std::process::exit(2);
        }
        DispatchOutcome::InternalError(error) => {
            eprintln!("membrane-client: internal: {error}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_else(|| "stdio-mcp".into());
    match mode.as_str() {
        "--help" | "-h" => {
            println!("membrane-client [stdio-mcp|hook|cli] [args…]");
            Ok(())
        }
        "--version" | "-V" => {
            println!("membrane-client {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "stdio-mcp" => run_stdio_mcp(),
        "hook" => forward_stdin("/hook"),
        "cli" => {
            let tail = args.collect::<Vec<_>>();
            let request = json!({ "args": tail });
            match request_engine("/cli", request.to_string().as_bytes()) {
                Ok((_, body)) => write_stdout(&body),
                Err(_) => unavailable_cli(),
            }
        }
        other => {
            let mut tail = vec![other.to_owned()];
            tail.extend(args);
            let request = json!({ "args": tail });
            match request_engine("/cli", request.to_string().as_bytes()) {
                Ok((_, body)) => write_stdout(&body),
                Err(_) => unavailable_cli(),
            }
        }
    }
}

fn unavailable_cli() -> Result<(), String> {
    println!(
        "{}",
        json!({ "kind": "membrane_unavailable", "reason": "hub_inactive", "retryable": true })
    );
    Err("hub inactive".into())
}

fn run_stdio_mcp() -> Result<(), String> {
    let stdin = io::stdin();
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    for line in stdin.lock().lines() {
        let line = line.map_err(|error| format!("read stdio request: {error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        if line.len() > MAX_BODY_BYTES {
            return Err("stdio request exceeds transport limit".into());
        }
        let (_, body) = request_engine("/mcp", line.as_bytes())?;
        if !body.is_empty() {
            stdout
                .write_all(&body)
                .and_then(|_| stdout.write_all(b"\n"))
                .map_err(|error| format!("write stdio response: {error}"))?;
            stdout
                .flush()
                .map_err(|error| format!("flush stdio response: {error}"))?;
        }
    }
    Ok(())
}

fn forward_stdin(path: &str) -> Result<(), String> {
    let mut body = Vec::new();
    io::stdin()
        .take((MAX_BODY_BYTES + 1) as u64)
        .read_to_end(&mut body)
        .map_err(|error| format!("read hook payload: {error}"))?;
    if body.len() > MAX_BODY_BYTES {
        return Err("hook payload exceeds transport limit".into());
    }
    let (_, response) = request_engine(path, &body)?;
    write_stdout(&response)
}

fn write_stdout(body: &[u8]) -> Result<(), String> {
    io::stdout()
        .write_all(body)
        .and_then(|_| io::stdout().flush())
        .map_err(|error| format!("write response: {error}"))
}

fn request_engine(path: &str, body: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let (host, port) = endpoint();
    let address = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|error| format!("resolve engine endpoint: {error}"))?
        .next()
        .ok_or_else(|| "engine endpoint has no address".to_owned())?;
    let mut stream = TcpStream::connect_timeout(&address, IO_TIMEOUT)
        .map_err(|error| format!("connect to singleton engine: {error}"))?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .and_then(|_| stream.set_write_timeout(Some(IO_TIMEOUT)))
        .map_err(|error| format!("configure engine transport: {error}"))?;
    let token = bearer_token();
    let mut request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}:{port}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    if let Some(token) = token.as_deref() {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    request.push_str("\r\n");
    stream
        .write_all(request.as_bytes())
        .and_then(|_| stream.write_all(body))
        .map_err(|error| format!("send engine request: {error}"))?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|error| format!("read engine response: {error}"))?;
    parse_response(response)
}

fn endpoint() -> (String, u16) {
    if let Some(value) = std::env::var_os("MEMBRANE_ENDPOINT") {
        if let Some((host, port)) = value.to_string_lossy().rsplit_once(':') {
            if let Ok(port) = port.trim_end_matches('/').parse() {
                return (host.trim_start_matches("http://").to_owned(), port);
            }
        }
    }
    if let Ok(port) = std::env::var("MEMBRANE_PORT") {
        if let Ok(port) = port.parse() {
            return ("127.0.0.1".into(), port);
        }
    }
    ("127.0.0.1".into(), DEFAULT_PORT)
}

fn bearer_token() -> Option<String> {
    for name in ["MEMBRANE_BEARER_TOKEN", "MEMBRANE_API_TOKEN"] {
        if let Ok(value) = std::env::var(name) {
            if !value.trim().is_empty() {
                return Some(value);
            }
        }
    }
    std::env::var_os("MEMBRANE_API_TOKEN_FILE")
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn parse_response(response: Vec<u8>) -> Result<(u16, Vec<u8>), String> {
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| "engine response has no HTTP header boundary".to_owned())?;
    let header = std::str::from_utf8(&response[..split])
        .map_err(|_| "engine response headers are not UTF-8".to_owned())?;
    let status = header
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| "engine response status is invalid".to_owned())?;
    let body = response[split + 4..].to_vec();
    if body.len() > MAX_BODY_BYTES {
        return Err("engine response exceeds transport limit".into());
    }
    if !(200..300).contains(&status) {
        let detail = serde_json::from_slice::<Value>(&body)
            .ok()
            .and_then(|value| {
                value
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| format!("HTTP {status}"));
        return Err(format!("engine request failed: {detail}"));
    }
    Ok((status, body))
}
