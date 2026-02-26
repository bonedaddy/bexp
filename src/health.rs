use std::sync::Arc;
use std::time::Instant;

use metrics_exporter_prometheus::PrometheusHandle;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::watch;

use crate::indexer::IndexerService;

pub async fn run_health_server(
    port: u16,
    indexer: Arc<IndexerService>,
    prometheus_handle: PrometheusHandle,
    mut shutdown: watch::Receiver<bool>,
) {
    let start_time = Instant::now();

    let listener = match TcpListener::bind(format!("127.0.0.1:{port}")).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(port, error = %e, "Failed to bind health server");
            return;
        }
    };
    tracing::info!(port, "Health/metrics server listening");

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (mut stream, _) = match result {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::debug!(error = %e, "Failed to accept connection");
                        continue;
                    }
                };

                let indexer = indexer.clone();
                let handle = prometheus_handle.clone();
                let uptime = start_time.elapsed().as_secs();

                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    let n = match tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await {
                        Ok(n) => n,
                        Err(_) => return,
                    };
                    let request = String::from_utf8_lossy(&buf[..n]);

                    let (status, content_type, body) = if request.starts_with("GET /health") {
                        let json = format!(
                            r#"{{"status":"ok","index_ready":{},"uptime_secs":{},"version":"{}"}}"#,
                            indexer.index_ready(),
                            uptime,
                            env!("CARGO_PKG_VERSION"),
                        );
                        ("200 OK", "application/json", json)
                    } else if request.starts_with("GET /metrics") {
                        let body = handle.render();
                        ("200 OK", "text/plain; version=0.0.4; charset=utf-8", body)
                    } else {
                        ("404 Not Found", "text/plain", "Not Found".to_string())
                    };

                    let response = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                });
            }
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    tracing::info!("Health server shutting down");
                    break;
                }
            }
        }
    }
}
