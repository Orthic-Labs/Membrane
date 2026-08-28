//! Bounded, authenticated loopback snapshot polling for tray evidence.
//!
//! The tray never invents aggregate values. If resident snapshot admission
//! data is missing, malformed, unauthenticated, or unavailable, every metric
//! remains explicitly `Unknown` with its typed reason.

use std::{
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use membrane_protocol::{HubSnapshotV1, HUB_ADMISSION_SCHEMA_VERSION, HUB_SCHEMA_VERSION};

const REQUEST_TIMEOUT: Duration = Duration::from_millis(500);
const POLL_INTERVAL: Duration = Duration::from_secs(2);
const MAX_RESPONSE_BYTES: usize = 1024 * 1024 + 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotValues {
    pub admitted: String,
    pub withheld: String,
    pub budget: String,
    pub observed: String,
}

impl SnapshotValues {
    pub fn unknown(reason: &str) -> Self {
        let value = format!("Unknown · {reason}");
        Self {
            admitted: value.clone(),
            withheld: value.clone(),
            budget: value,
            observed: format!("Unknown · {reason}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotUpdate {
    pub generation: u64,
    pub values: SnapshotValues,
}

/// Start one bounded poller per daemon generation. Token stays in this
/// in-memory worker and is only written to loopback Authorization headers.
pub fn start_polling(
    endpoint: String,
    bearer_token: String,
    generation: u64,
) -> (Receiver<SnapshotUpdate>, Arc<AtomicBool>) {
    let (sender, receiver) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    thread::Builder::new()
        .name("membrane-tray-snapshot".into())
        .spawn(move || poll_loop(endpoint, bearer_token, generation, worker_stop, sender))
        .expect("snapshot worker thread must start");
    (receiver, stop)
}

fn poll_loop(
    endpoint: String,
    bearer_token: String,
    generation: u64,
    stop: Arc<AtomicBool>,
    sender: Sender<SnapshotUpdate>,
) {
    loop {
        if stop.load(Ordering::Acquire) {
            return;
        }
        let values = fetch_snapshot(&endpoint, &bearer_token)
            .unwrap_or_else(|reason| SnapshotValues::unknown(reason));
        if sender.send(SnapshotUpdate { generation, values }).is_err() {
            return;
        }
        let mut elapsed = Duration::ZERO;
        while elapsed < POLL_INTERVAL {
            if stop.load(Ordering::Acquire) {
                return;
            }
            let step = Duration::from_millis(100);
            thread::sleep(step);
            elapsed += step;
        }
    }
}

fn fetch_snapshot(endpoint: &str, bearer_token: &str) -> Result<SnapshotValues, &'static str> {
    let address = parse_loopback_endpoint(endpoint)?;
    if bearer_token.len() != 64
        || !bearer_token
            .as_bytes()
            .iter()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err("snapshot_auth_invalid");
    }
    let deadline = Instant::now()
        .checked_add(REQUEST_TIMEOUT)
        .unwrap_or_else(Instant::now);
    let connect_timeout = remaining_timeout(deadline)?;
    let mut stream =
        TcpStream::connect_timeout(&address, connect_timeout).map_err(snapshot_io_reason)?;
    let request = format!(
        "GET /hub/snapshot HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {bearer_token}\r\nConnection: close\r\n\r\n"
    );
    let request = request.as_bytes();
    let mut written = 0;
    while written < request.len() {
        let write_timeout = remaining_timeout(deadline)?;
        stream
            .set_write_timeout(Some(write_timeout))
            .map_err(|_| "snapshot_unavailable")?;
        match stream.write(&request[written..]) {
            Ok(0) => return Err("snapshot_unavailable"),
            Ok(count) => written += count,
            Err(error) => return Err(snapshot_io_reason(error)),
        }
    }
    let mut response = Vec::new();
    let mut bytes = [0_u8; 8 * 1024];
    let mut expected_response_bytes = None;
    loop {
        let read_timeout = remaining_timeout(deadline)?;
        stream
            .set_read_timeout(Some(read_timeout))
            .map_err(|_| "snapshot_unavailable")?;
        match stream.read(&mut bytes) {
            Ok(0) => break,
            Ok(read) => {
                response.extend_from_slice(&bytes[..read]);
                if response.len() > MAX_RESPONSE_BYTES {
                    return Err("snapshot_too_large");
                }
                if expected_response_bytes.is_none() {
                    if let Some(split) =
                        response.windows(4).position(|window| window == b"\r\n\r\n")
                    {
                        let head = std::str::from_utf8(&response[..split])
                            .map_err(|_| "snapshot_invalid_http")?;
                        let content_length = content_length(head)?;
                        let total = split
                            .checked_add(4)
                            .and_then(|value| value.checked_add(content_length))
                            .ok_or("snapshot_too_large")?;
                        if total > MAX_RESPONSE_BYTES {
                            return Err("snapshot_too_large");
                        }
                        expected_response_bytes = Some(total);
                    }
                }
                if expected_response_bytes.is_some_and(|expected| response.len() >= expected) {
                    break;
                }
            }
            Err(error) => return Err(snapshot_io_reason(error)),
        }
    }
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or("snapshot_invalid_http")?;
    let head = std::str::from_utf8(&response[..split]).map_err(|_| "snapshot_invalid_http")?;
    if !head.starts_with("HTTP/1.1 200 ") && !head.starts_with("HTTP/1.0 200 ") {
        return Err("snapshot_unavailable");
    }
    let body_length = content_length(head)?;
    let body_end = split
        .checked_add(4)
        .and_then(|value| value.checked_add(body_length))
        .ok_or("snapshot_too_large")?;
    let body = response
        .get(split + 4..body_end)
        .ok_or("snapshot_invalid_http")?;
    let snapshot: HubSnapshotV1 = serde_json::from_slice(body).map_err(|_| "snapshot_invalid")?;
    if snapshot.schema_version != HUB_SCHEMA_VERSION {
        return Err("snapshot_schema_unsupported");
    }
    let admission = snapshot.admission.ok_or("snapshot_admission_unavailable")?;
    if admission.schema_version != HUB_ADMISSION_SCHEMA_VERSION {
        return Err("snapshot_admission_schema_unsupported");
    }
    let admitted = admission
        .decisions_total
        .checked_sub(admission.omissions_total)
        .ok_or("snapshot_admission_invalid")?;
    Ok(SnapshotValues {
        admitted: admitted.to_string(),
        withheld: admission.omissions_total.to_string(),
        budget: admission.budget_pressure_total.to_string(),
        observed: format_observed(admission.window_hours, snapshot.observed_at_unix_ms),
    })
}

fn content_length(head: &str) -> Result<usize, &'static str> {
    let mut value = None;
    for line in head.lines().skip(1) {
        let Some((name, raw_value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            let parsed = raw_value
                .trim()
                .parse::<usize>()
                .map_err(|_| "snapshot_invalid_http")?;
            if value.replace(parsed).is_some() {
                return Err("snapshot_invalid_http");
            }
        }
    }
    value.ok_or("snapshot_invalid_http")
}

fn remaining_timeout(deadline: Instant) -> Result<Duration, &'static str> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        Err("snapshot_timeout")
    } else {
        Ok(remaining)
    }
}

fn snapshot_io_reason(error: io::Error) -> &'static str {
    match error.kind() {
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => "snapshot_timeout",
        _ => "snapshot_unavailable",
    }
}

fn parse_loopback_endpoint(endpoint: &str) -> Result<SocketAddr, &'static str> {
    let authority = endpoint
        .strip_prefix("http://")
        .ok_or("snapshot_endpoint_invalid")?
        .split('/')
        .next()
        .ok_or("snapshot_endpoint_invalid")?;
    let address: SocketAddr = authority.parse().map_err(|_| "snapshot_endpoint_invalid")?;
    if !address.ip().is_loopback() {
        return Err("snapshot_endpoint_not_loopback");
    }
    Ok(address)
}

fn format_observed(window_hours: u32, observed_at_ms: u64) -> String {
    if observed_at_ms == 0 {
        return format!("Unknown · window {window_hours}h");
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let age = now.saturating_sub(observed_at_ms);
    let age_label = if age < 1_000 {
        "now".to_owned()
    } else if age < 60_000 {
        format!("{}s ago", age / 1_000)
    } else {
        format!("{}m ago", age / 60_000)
    };
    format!("window {window_hours}h · observed {age_label}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_loopback_endpoint() {
        assert_eq!(
            parse_loopback_endpoint("http://192.0.2.1:4317").unwrap_err(),
            "snapshot_endpoint_not_loopback"
        );
    }

    #[test]
    fn rejects_non_hex_or_wrong_length_token() {
        assert_eq!(
            fetch_snapshot("http://127.0.0.1:1", "short").unwrap_err(),
            "snapshot_auth_invalid"
        );
        assert_eq!(
            fetch_snapshot("http://127.0.0.1:1", &"A".repeat(64)).unwrap_err(),
            "snapshot_auth_invalid"
        );
    }

    #[test]
    fn unknown_values_keep_typed_reason() {
        let values = SnapshotValues::unknown("snapshot_timeout");
        assert!(values.admitted.starts_with("Unknown · snapshot_timeout"));
        assert!(values.withheld.starts_with("Unknown · snapshot_timeout"));
        assert!(values.budget.starts_with("Unknown · snapshot_timeout"));
    }

    #[test]
    fn content_length_frames_keep_alive_response() {
        assert_eq!(
            content_length(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\ncontent-length: 42"
            ),
            Ok(42)
        );
        assert_eq!(
            content_length("HTTP/1.1 200 OK\r\ncontent-type: application/json"),
            Err("snapshot_invalid_http")
        );
        assert_eq!(
            content_length("HTTP/1.1 200 OK\r\ncontent-length: 1\r\nContent-Length: 1"),
            Err("snapshot_invalid_http")
        );
    }
}
