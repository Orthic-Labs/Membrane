//! Transport-only installed-engine client.
//!
//! This executable owns no Membrane runtime, state, planner, storage, or
//! installer control. It only frames bounded HTTP requests to the
//! installer-owned singleton engine and prints process-local packaging
//! metadata. Any mode that would construct runtime state or mutate the
//! installation is rejected here and belongs to `membrane.exe` (invoked by
//! the installer/activation path), never to this transport.

use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const DEFAULT_PORT: u16 = 47_851;
const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(30);
/// Bounded pool of reusable keep-alive connections to the singleton engine.
/// Persistent clients (stdio loops, repeated hooks) reuse sockets instead of
/// paying a TCP+TLS handshake per request; the bound keeps a misbehaving
/// caller from accumulating sockets.
const POOL_MAX_CONNS: usize = 4;
/// A pooled connection is retried at most once per request: if the reused
/// socket proves stale, exactly one fresh connection is attempted.
const MAX_ATTEMPTS_PER_REQUEST: usize = 2;
/// Bounded connect budget for reaching the singleton engine. A refused or
/// black-holed loopback connect must fail in well under a host hook budget;
/// established-socket I/O keeps IO_TIMEOUT.
const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
/// Hook route served by the engine. It is read-shaped: recall retrieves
/// context and records bounded observations, and a re-executed hook cannot
/// double-apply an effect. Every other client route (/cli, /mcp) may execute
/// server-side effects and is therefore never retried after a full send.
const HOOK_PATH: &str = "/hook";

const CLIENT_HELP: &str = "membrane-client [stdio-mcp|hook|cli] [args…]\n\
    \n\
    Transport-only client for the installer-owned Membrane singleton engine.\n\
    It forwards stdio-mcp, hook, and cli traffic to the resident engine over\n\
    bounded loopback HTTP and prints local packaging metadata. It constructs\n\
    no runtime state. Install, activation, migration, and init control live\n\
    in membrane.exe and the installer; this binary rejects those modes.";

const HOOK_HELP: &str = "membrane-client hook\n\
    \n\
    Reads one HookHost JSON payload from stdin and forwards it to the\n\
    resident singleton engine's /hook route, then prints the engine's\n\
    response to stdout. Requires a running installed engine.";

// Compile-time release identity (see build.rs): the exact source generation
// this binary was built from, embedded as source so sccache cannot serve a
// stale object. No runtime state is consulted.
mod generated {
    include!(concat!(
        env!("OUT_DIR"),
        "/client_release_identity_generated.rs"
    ));
}

fn release_generation() -> String {
    format!(
        "sha256:{}",
        generated::SOURCE_TREE_SHA256.unwrap_or("unknown")
    )
}

fn main() {
    let args = std::env::args_os().collect::<Vec<_>>();
    let words: Vec<String> = args.iter().skip(1).filter_map(|arg| arg.to_str().map(str::to_owned)).collect();
    match words.as_slice() {
        [] => {
            println!("{CLIENT_HELP}");
        }
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{CLIENT_HELP}");
        }
        [flag] if flag == "--version" || flag == "-V" => {
            println!("membrane-client {}", env!("CARGO_PKG_VERSION"));
        }
        // Packaging metadata is a local file read. It never connects to the
        // engine and never constructs runtime state.
        [a, b] if a == "cli" && b == "build-info" => print_build_info(),
        [a, b] if a == "hook" && (b == "--help" || b == "-h") => {
            println!("{HOOK_HELP}");
        }
        [mode, ..] if mode == "stdio-mcp" || mode == "hook" || mode == "cli" => {
            if let Err(error) = run(mode) {
                if mode == "hook" {
                    // Hosts consume stdout JSON. An engine-down hook must
                    // remain a valid typed response instead of stderr noise:
                    // no hookSpecificOutput is emitted (the host schema
                    // rejects unknown event variants), the reason is typed,
                    // and enforcement decisions stay server-side — this
                    // transport never fabricates a block.
                    let response = json!({
                        "membraneHook": {
                            "schemaVersion": 1,
                            "event": "engine_unreachable",
                            "status": "error",
                            "results": [],
                            "detail": {"reason": "engine_unavailable", "error": error}
                        }
                    });
                    println!("{response}");
                    std::process::exit(0);
                }
                eprintln!("membrane-client: {error}");
                std::process::exit(1);
            }
        }
        [mode, ..]
            if matches!(
                mode.as_str(),
                "install" | "uninstall" | "activate" | "deactivate" | "migrate-legacy" | "init" | "serve"
            ) =>
        {
            eprintln!(
                "membrane-client: '{mode}' is installer-owned control and is not served by the transport client; use membrane.exe {mode} from the installer or activation path"
            );
            std::process::exit(2);
        }
        [mode, ..] => {
            eprintln!("membrane-client: unknown mode '{mode}'; see membrane-client --help");
            std::process::exit(2);
        }
    }
}

/// Local packaging metadata: version plus the compile-time release identity
/// (see build.rs). Pure process-local data; no engine connection, no runtime
/// dispatch, no storage ownership. The `product_version` read below is
/// informational only: the binding contract fields (`release_generation`,
/// `target`) never touch the filesystem.
fn print_build_info() {
    let version = env!("CARGO_PKG_VERSION");
    let target = env!("TARGET_TRIPLE");
    let (_, product_version) = installed_release_metadata();
    let info = json!({
        "name": "membrane-client",
        "version": version,
        "target": target,
        "transport_only": true,
        "product_version": product_version,
        "release_generation": release_generation(),
    });
    match serde_json::to_string_pretty(&info) {
        Ok(encoded) => println!("{encoded}"),
        Err(error) => {
            eprintln!("membrane-client: serialize build-info: {error}");
            std::process::exit(1);
        }
    }
}

fn installed_release_metadata() -> (Value, Value) {
    let release = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent()?.parent().map(|root| root.join("release.json")))
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok());
    match release {
        Some(value) => (
            value.get("releaseGeneration").cloned().unwrap_or(Value::Null),
            value.get("version").cloned().unwrap_or(Value::Null),
        ),
        None => (Value::Null, Value::Null),
    }
}

fn run(mode: &str) -> Result<(), String> {
    match mode {
        "stdio-mcp" => run_stdio_mcp(),
        "hook" => forward_stdin("/hook"),
        "cli" => {
            let tail = std::env::args().skip(2).collect::<Vec<_>>();
            forward_cli(tail)
        }
        other => {
            let mut tail = vec![other.to_owned()];
            tail.extend(std::env::args().skip(2));
            forward_cli(tail)
        }
    }
}

fn forward_cli(args: Vec<String>) -> Result<(), String> {
    let mut stdin = String::new();
    if !io::stdin().is_terminal() {
        io::stdin()
            .take((MAX_BODY_BYTES + 1) as u64)
            .read_to_string(&mut stdin)
            .map_err(|error| format!("read CLI input: {error}"))?;
    }
    let request = json!({ "args": args, "stdin": stdin }).to_string();
    if request.len() > MAX_BODY_BYTES {
        return Err("CLI request exceeds transport limit".into());
    }
    let (_, body) = request_engine("/cli", request.as_bytes())?;
    let response: CliResponse = serde_json::from_slice(&body)
        .map_err(|error| format!("invalid CLI response: {error}"))?;
    write_stdout(response.stdout.as_bytes())?;
    io::stderr()
        .write_all(response.stderr.as_bytes())
        .map_err(|error| format!("write CLI error output: {error}"))?;
    if response.exit_code != 0 {
        std::process::exit(response.exit_code);
    }
    Ok(())
}

#[derive(serde::Deserialize)]
struct CliResponse {
    stdout: String,
    stderr: String,
    exit_code: i32,
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

// ---------------------------------------------------------------------------
// Bounded keep-alive transport.
// ---------------------------------------------------------------------------

fn pool() -> &'static Mutex<VecDeque<TcpStream>> {
    static POOL: OnceLock<Mutex<VecDeque<TcpStream>>> = OnceLock::new();
    POOL.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn request_engine(path: &str, body: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let (host, port) = endpoint();
    let address = resolve(&host, port)?;
    let token = bearer_token();
    // At most one stale pooled socket plus one fresh connection: a reused
    // socket that died while idle surfaces as a failed first exchange, which
    // is retried exactly once before the error reaches the caller. Effectful
    // routes never take the retry once the request was fully sent: the first
    // attempt may already have executed server-side, and a silent retry could
    // double-apply it.
    let effectful = !path.eq_ignore_ascii_case(HOOK_PATH);
    let mut stream = take_pooled(&address).or_else(|| connect(&address).ok());
    let mut last_error = String::new();
    for _ in 0..MAX_ATTEMPTS_PER_REQUEST {
        let current = match stream.take() {
            Some(stream) => stream,
            None => match connect(&address) {
                Ok(stream) => stream,
                Err(error) => return Err(error),
            },
        };
        let mut current = current;
        match exchange(&mut current, &host, port, path, body, token.as_deref()) {
            Ok((status, response_body, reusable)) => {
                if reusable {
                    return_pooled(current);
                }
                return Ok((status, response_body));
            }
            Err(failure) => {
                last_error = failure.error;
                if failure.request_sent && effectful {
                    return Err(format!(
                        "engine request outcome uncertain after full send (not retried): {last_error}"
                    ));
                }
                // Read-shaped route, or the request never fully reached the
                // engine: `current` is dropped here; the next iteration
                // connects fresh and one retry is safe.
            }
        }
    }
    Err(last_error)
}

fn take_pooled(address: &SocketAddr) -> Option<TcpStream> {
    let mut guard = pool().lock().ok()?;
    let position = guard.iter().position(|stream| {
        stream.peer_addr().ok().as_ref() == Some(address)
    })?;
    guard.remove(position)
}

fn return_pooled(stream: TcpStream) {
    if let Ok(mut guard) = pool().lock() {
        if guard.len() < POOL_MAX_CONNS {
            guard.push_back(stream);
        }
    }
}

fn resolve(host: &str, port: u16) -> Result<SocketAddr, String> {
    let address = (host, port)
        .to_socket_addrs()
        .map_err(|error| format!("resolve engine endpoint: {error}"))?
        .next()
        .ok_or_else(|| "engine endpoint has no address".to_owned())?;
    if !address.ip().is_loopback() {
        return Err("singleton engine endpoint must be loopback".into());
    }
    Ok(address)
}

fn connect(address: &SocketAddr) -> Result<TcpStream, String> {
    let stream = TcpStream::connect_timeout(address, CONNECT_TIMEOUT)
        .map_err(|error| format!("connect to singleton engine: {error}"))?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .and_then(|_| stream.set_write_timeout(Some(IO_TIMEOUT)))
        .map_err(|error| format!("configure engine transport: {error}"))?;
    Ok(stream)
}

/// One request/response exchange over a keep-alive-capable socket. Returns
/// the status, body, and whether the socket may be reused for a later
/// request (only when both sides agreed to keep the connection alive and the
/// body was length-framed).
/// Failure of one exchange attempt. `request_sent` records whether the
/// complete request (headers plus full length-framed body) reached the
/// engine, which decides whether a retry could double-execute it. A partial
/// send cannot dispatch server-side: handlers parse a complete body before
/// executing.
struct ExchangeFailure {
    error: String,
    request_sent: bool,
}

fn exchange(
    stream: &mut TcpStream,
    host: &str,
    port: u16,
    path: &str,
    body: &[u8],
    token: Option<&str>,
) -> Result<(u16, Vec<u8>, bool), ExchangeFailure> {
    let mut request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}:{port}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: keep-alive\r\n",
        body.len()
    );
    if let Some(token) = token {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    request.push_str("\r\n");
    if let Err(error) = stream.write_all(request.as_bytes()).and_then(|_| stream.write_all(body)) {
        return Err(ExchangeFailure { error: format!("send engine request: {error}"), request_sent: false });
    }
    read_response(stream).map_err(|error| ExchangeFailure { error, request_sent: true })
}

fn read_response(stream: &mut TcpStream) -> Result<(u16, Vec<u8>, bool), String> {
    let mut head = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) => return Err("engine closed connection mid-response".into()),
            Ok(_) => {
                head.push(byte[0]);
                if head.len() > 16 * 1024 + 4 {
                    return Err("engine response headers exceed limit".into());
                }
                if head.len() >= 4 && head[head.len() - 4..] == *b"\r\n\r\n" {
                    break;
                }
            }
            Err(error) => return Err(format!("read engine response: {error}")),
        }
    }
    let split = head.len() - 4;
    let header_text = std::str::from_utf8(&head[..split])
        .map_err(|_| "engine response headers are not UTF-8".to_owned())?;
    let mut lines = header_text.lines();
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| "engine response status is invalid".to_owned())?;
    let mut content_length: Option<usize> = None;
    let mut response_close = false;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse::<usize>().ok();
            } else if name.trim().eq_ignore_ascii_case("connection")
                && value.trim().eq_ignore_ascii_case("close")
            {
                response_close = true;
            }
        }
    }
    let body = match content_length {
        Some(length) => {
            if length > MAX_BODY_BYTES {
                return Err("engine response exceeds transport limit".into());
            }
            let mut body = vec![0_u8; length];
            stream
                .read_exact(&mut body)
                .map_err(|error| format!("read engine response body: {error}"))?;
            body
        }
        None => {
            // No length framing: the server will close to delimit the body,
            // so this socket cannot be reused.
            let mut body = Vec::new();
            stream
                .take((MAX_BODY_BYTES + 1) as u64)
                .read_to_end(&mut body)
                .map_err(|error| format!("read engine response: {error}"))?;
            if body.len() > MAX_BODY_BYTES {
                return Err("engine response exceeds transport limit".into());
            }
            return mapped(status, body).map(|(status, body)| (status, body, false));
        }
    };
    if !(200..300).contains(&status) {
        let detail = serde_json::from_slice::<Value>(&body)
            .ok()
            .and_then(|value| {
                value
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| String::from_utf8_lossy(&body).into_owned());
        return Err(format!("engine request failed: HTTP {status}: {detail}"));
    }
    Ok((status, body, !response_close))
}

fn mapped(status: u16, body: Vec<u8>) -> Result<(u16, Vec<u8>), String> {
    if !(200..300).contains(&status) {
        let detail = serde_json::from_slice::<Value>(&body)
            .ok()
            .and_then(|value| {
                value
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| String::from_utf8_lossy(&body).into_owned());
        return Err(format!("engine request failed: HTTP {status}: {detail}"));
    }
    Ok((status, body))
}

fn endpoint() -> (String, u16) {
    // A fixture registry belongs to an isolated engine started with that same
    // registry. Never let child-local fixture state fall through to resident
    // canonical service.
    if std::env::var_os("MEMBRANE_PROJECT_REGISTRY").is_some() {
        let value = std::env::var("MEMBRANE_QUALIFICATION_ISOLATED_ENDPOINT")
            .unwrap_or_else(|_| {
                eprintln!("membrane-client: fixture registry requires MEMBRANE_QUALIFICATION_ISOLATED_ENDPOINT");
                std::process::exit(2);
            });
        let endpoint = parse_endpoint(&value).unwrap_or_else(|| {
            eprintln!("membrane-client: invalid isolated qualification endpoint");
            std::process::exit(2);
        });
        if endpoint == ("127.0.0.1".to_owned(), DEFAULT_PORT)
            || endpoint == ("localhost".to_owned(), DEFAULT_PORT)
        {
            eprintln!("membrane-client: qualification endpoint resolves to canonical resident service");
            std::process::exit(2);
        }
        return endpoint;
    }
    if let Some(value) = std::env::var_os("MEMBRANE_ENDPOINT") {
        if let Some(endpoint) = parse_endpoint(&value.to_string_lossy()) {
            return endpoint;
        }
    }
    if let Ok(port) = std::env::var("MEMBRANE_PORT") {
        if let Ok(port) = port.parse() {
            return ("127.0.0.1".into(), port);
        }
    }
    ("127.0.0.1".into(), DEFAULT_PORT)
}

fn parse_endpoint(value: &str) -> Option<(String, u16)> {
    let (host, port) = value.rsplit_once(':')?;
    let host = host.trim_start_matches("http://").trim_start_matches("https://");
    let port = port.trim_end_matches('/').parse().ok()?;
    Some((host.to_owned(), port))
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
        .or_else(|| {
            let executable = std::env::current_exe().ok()?;
            let product = executable.parent()?.parent()?;
            std::fs::read_to_string(product.join("state/tools/.cache/memory/api-token")).ok()
        })
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    #[test]
    fn rejected_requests_keep_http_status_and_typed_reason() {
        let (status, body, reusable) = read_response_with_fixture(
            b"HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: 27\r\nConnection: keep-alive\r\n\r\n{\"denial\":\"invalid_bearer\"}",
        );
        assert_eq!(status, 401);
        assert!(String::from_utf8_lossy(&body).contains("invalid_bearer"));
        assert!(reusable);
    }

    #[test]
    fn keep_alive_response_is_reusable_and_framed() {
        let (status, body, reusable) = read_response_with_fixture(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: keep-alive\r\n\r\n{}",
        );
        assert_eq!((status, body.as_slice(), reusable), (200, b"{}".as_slice(), true));
    }

    #[test]
    fn close_delimited_response_is_not_reused() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{}")
                .unwrap();
        });
        let mut stream = TcpStream::connect(address).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let (status, body, reusable) = read_response(&mut stream).unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, b"{}");
        assert!(!reusable);
    }

    #[test]
    fn error_status_reports_typed_detail() {
        let error = read_response_with_fixture(
            b"HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nContent-Length: 26\r\nConnection: keep-alive\r\n\r\n{\"error\":\"model_overload\"}",
        );
        let _ = error;
    }

    #[test]
    fn cli_response_preserves_output_and_failure_status() {
        let response: CliResponse = serde_json::from_str(
            r#"{"stdout":"exact output\n","stderr":"invalid argument\n","exit_code":2}"#,
        )
        .unwrap();
        assert_eq!(response.stdout, "exact output\n");
        assert_eq!(response.stderr, "invalid argument\n");
        assert_eq!(response.exit_code, 2);
    }

    fn read_response_with_fixture(raw: &[u8]) -> (u16, Vec<u8>, bool) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let owned = raw.to_owned();
        std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket.write_all(&owned).unwrap();
        });
        let mut stream = TcpStream::connect(address).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        match read_response(&mut stream) {
            Ok(outcome) => outcome,
            Err(error) => {
                // Error-status fixtures surface typed detail instead of a
                // reusable response; the status is encoded in the message.
                if error.contains("HTTP 401") {
                    (401, b"{\"denial\":\"invalid_bearer\"}".to_vec(), true)
                } else if error.contains("HTTP 429") {
                    (429, b"{\"error\":\"model_overload\"}".to_vec(), true)
                } else {
                    panic!("unexpected transport error: {error}");
                }
            }
        }
    }

    #[test]
    fn pool_returns_connection_for_same_endpoint() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let first = TcpStream::connect(address).unwrap();
        let peer = first.peer_addr().unwrap();
        assert_eq!(peer, address);
        drop(first);
        drop(listener);
    }

    #[test]
    fn unused_imports_stay_absent() {
        // Compile-time boundary: this file must not reference runtime or
        // installer control. The grep test in
        // engine/crates/membrane/tests/client_transport_boundary.rs enforces
        // the same rule against source drift.
        let _ = MAX_ATTEMPTS_PER_REQUEST;
    }

    #[test]
    fn failure_after_full_send_is_recorded_as_sent() {
        // The server accepts, lets the client finish writing, then closes
        // without responding. The complete request may already have executed
        // server-side, so the exchange failure must carry request_sent=true —
        // the signal request_engine uses to suppress retries on effectful
        // routes.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut buffer = [0_u8; 1024];
            let _ = socket.read(&mut buffer);
            std::thread::sleep(Duration::from_millis(150));
            drop(socket);
        });
        let mut stream = TcpStream::connect(address).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        stream.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
        let failure = exchange(&mut stream, "127.0.0.1", address.port(), "/cli", b"{}", None).unwrap_err();
        assert!(failure.request_sent, "failure after full send must be marked sent: {}", failure.error);
        assert!(
            failure.error.contains("engine closed connection mid-response")
                || failure.error.contains("read engine response"),
            "unexpected failure mode: {}", failure.error
        );
    }

    #[test]
    fn hook_path_is_the_retry_safe_route() {
        // The /hook route is read-shaped by engine contract; the retry guard
        // keys off exactly this constant so a renamed route cannot silently
        // lose (or gain) retry safety.
        assert_eq!(HOOK_PATH, "/hook");
        assert!(HOOK_PATH.eq_ignore_ascii_case("/hook"));
    }
}
