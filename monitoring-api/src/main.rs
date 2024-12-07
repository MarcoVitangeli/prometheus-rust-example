use axum::{
    extract::{MatchedPath, Request},
    middleware::{self, Next},
    response::IntoResponse,
    routing::get,
    Router,
};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use std::{
    future::ready, time::{Duration, Instant}
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn metrics_app() -> Router {
    let collector = metrics_process::Collector::default();
    collector.describe();
    let recorder_handle = setup_metrics_recorder();
    Router::new().route("/metrics", get(move || { 
        collector.collect();
        ready(recorder_handle.render()) 
    }))
}

fn main_app() -> Router {
    Router::new()
        .route("/fast", get(|| async {}))
        .route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }),
        )
        .route_layer(middleware::from_fn(track_metrics))
}

async fn start_main_server() {
    let app = main_app();

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .unwrap();
    tracing::debug!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

async fn start_metrics_server() {
    let app = metrics_app();
    let mut system = sysinfo::System::new_all();
    let cpu_usage = metrics::gauge!("cpu_usage_percentage");
    let memory_usage = metrics::gauge!("memory_usage_percentage");

    tokio::task::spawn(async move {
        loop {
            // Refresh system data
            system.refresh_cpu_all();
            system.refresh_memory();

            // Get CPU usage
            let cpu_percent = system.global_cpu_usage();
            cpu_usage.set(cpu_percent as f64);

            // Get memory usage
            let total_memory = system.total_memory();
            let used_memory = system.used_memory();
            let memory_percent = (used_memory as f64 / total_memory as f64) * 100.0;
            memory_usage.set(memory_percent as f64);

            // Sleep for 10 seconds before updating again
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
    });

    // NOTE: expose metrics endpoint on a different port
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3001")
        .await
        .unwrap();
    tracing::debug!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                format!("{}=debug,tower_http=debug", env!("CARGO_CRATE_NAME")).into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // The `/metrics` endpoint should not be publicly available. If behind a reverse proxy, this
    // can be achieved by rejecting requests to `/metrics`. In this example, a second server is
    // started on another port to expose `/metrics`.
    let (_main_server, _metrics_server) = tokio::join!(start_main_server(), start_metrics_server());
}

fn setup_metrics_recorder() -> PrometheusHandle {
    const EXPONENTIAL_SECONDS: &[f64] = &[
        0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
    ];

    PrometheusBuilder::new()
        .set_buckets_for_metric(
            Matcher::Full("http_requests_duration_seconds".to_string()),
            EXPONENTIAL_SECONDS,
        )
        .unwrap()
        .install_recorder()
        .unwrap()
}

async fn track_metrics(req: Request, next: Next) -> impl IntoResponse {
    let start = Instant::now();
    let path = if let Some(matched_path) = req.extensions().get::<MatchedPath>() {
        matched_path.as_str().to_owned()
    } else {
        req.uri().path().to_owned()
    };
    let method = req.method().clone();

    let response = next.run(req).await;

    let latency = start.elapsed().as_secs_f64();
    let status = response.status().as_u16().to_string();

    tracing::debug!("Recording metrics: method={}, path={}, status={}, latency={}", method, path, status, latency);

    let labels = [
        ("method", method.to_string()),
        ("path", path),
        ("status", status),
    ];
    
    metrics::counter!("http_requests_total", &labels).increment(1);
    metrics::histogram!("http_requests_duration_seconds", &labels).record(latency);

    response
}
