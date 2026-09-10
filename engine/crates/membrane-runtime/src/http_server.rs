//! Preserve the canonical wire header profile at HTTP serialization.
use std::future::Future;
use axum::Router;
use hyper_util::{rt::{TokioExecutor, TokioIo}, server::conn::auto::Builder, service::TowerToHyperService};
use tokio::{net::TcpListener, task::JoinSet};
use tokio_util::sync::CancellationToken;
use tower::Service;

pub(crate) async fn serve(
    listener: TcpListener,
    app: Router,
    shutdown: impl Future<Output = ()>,
) -> Result<(), String> {
    let mut make_service = app.into_make_service_with_connect_info::<std::net::SocketAddr>();
    let cancellation = CancellationToken::new();
    let mut connections = JoinSet::new();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            accepted = listener.accept() => {
                let (stream, remote) = accepted.map_err(|error| error.to_string())?;
                let service = make_service.call(remote).await.unwrap();
                let cancellation = cancellation.clone();
                connections.spawn(async move {
                    let mut builder = Builder::new(TokioExecutor::new());
                    // Hyper adds Date after middleware, so disable it here.
                    builder.http1().auto_date_header(false);
                    builder.http2().auto_date_header(false);
                    let connection = builder.serve_connection_with_upgrades(
                        TokioIo::new(stream), TowerToHyperService::new(service),
                    );
                    tokio::pin!(connection);
                    let result = tokio::select! {
                        result = &mut connection => result,
                        _ = cancellation.cancelled() => {
                            connection.as_mut().graceful_shutdown();
                            connection.await
                        }
                    };
                    if let Err(error) = result {
                        eprintln!("{}", serde_json::json!({"event":"http_connection_failed", "reason":error.to_string()}));
                    }
                });
            }
            result = connections.join_next(), if !connections.is_empty() => {
                if let Some(Err(error)) = result {
                    return Err(format!("HTTP connection task failed: {error}"));
                }
            }
        }
    }
    cancellation.cancel();
    // The runtime owner retains its existing five-second drain bound.
    while let Some(result) = connections.join_next().await {
        result.map_err(|error| format!("HTTP connection drain failed: {error}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, extract::ConnectInfo, http::{HeaderMap, Response}, routing::get};
    use membrane_client::{LoopbackAuthSigner, LoopbackIdentityFields};
    use std::{net::SocketAddr, time::{Duration, SystemTime, UNIX_EPOCH}};

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn signed_health_survives_real_http_serialization_and_drains() {
        for status in [200u16, 503] {
            let identity = LoopbackIdentityFields {
                installation_id: "installation-test".into(), cortex_store_id: "store-test".into(),
                release_generation: "release-test".into(), startup_generation: 1,
                stable_install_root: r"C:\Membrane\current".into(),
            };
            let token = "12".repeat(32);
            let handler_identity = identity.clone();
            let handler_token = token.clone();
            let app = Router::new().route("/health", get(move |ConnectInfo(remote): ConnectInfo<SocketAddr>, headers: HeaderMap| {
                let identity = handler_identity.clone();
                let token = handler_token.clone();
                async move {
                    assert!(remote.ip().is_loopback());
                    let signer = LoopbackAuthSigner::from_hex_token(&token).unwrap();
                    let headers = headers.iter().map(|(name, value)| (name.to_string(), value.to_str().unwrap().to_string())).collect::<Vec<_>>();
                    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                    let request = membrane_client::verify_loopback_request_headers(
                        &signer, &headers, "GET", "/health", "127.0.0.1", "", &[], &identity, now,
                    ).unwrap();
                    let body = br#"{"serviceId":"membrane-hub"}"#;
                    let headers = membrane_client::build_loopback_response_headers(
                        &signer, &identity, request.nonce, status, body, request.expiry_unix_secs,
                    ).unwrap();
                    let mut response = Response::builder().status(status);
                    for (name, value) in headers { response = response.header(name, value); }
                    response.body(Body::from(body.as_slice())).unwrap()
                }
            }));
            let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await.unwrap();
            let address = listener.local_addr().unwrap();
            let cancellation = CancellationToken::new();
            let shutdown = cancellation.clone();
            let server = tokio::spawn(serve(listener, app, shutdown.cancelled_owned()));
            let response = tokio::task::spawn_blocking(move || {
                crate::installed_health::probe_with_identity(address, &token, Duration::from_secs(2), identity)
            }).await.unwrap();
            cancellation.cancel();
            tokio::time::timeout(Duration::from_secs(2), server).await.unwrap().unwrap().unwrap();
            assert_eq!(response.unwrap().status, status);
        }
    }
}
