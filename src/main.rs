mod api;
mod chat;
mod donors;
mod encoding;
mod engine;
mod error;
mod status;
mod telemetry;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderValue, header};
use axum::routing::{get, post};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

/// 100 MiB cap per upload. Files stream to disk; this is the hard ceiling.
pub const MAX_UPLOAD_BYTES: usize = 100 * 1024 * 1024;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let prometheus_handle = telemetry::init_telemetry();
    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "static".into());
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "data".into());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3000);

    let llm_url = std::env::var("LLM_URL").unwrap_or_default();
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let state = api::AppState {
        metrics: prometheus_handle,
        donors: Arc::new(donors::DonorQueue::new(
            PathBuf::from(&data_dir).join("donors.json"),
        )),
        cpu: Arc::new(status::CpuSampler::new()),
        llm_url,
        http,
    };

    let app = Router::new()
        .route("/health", get(api::health_check))
        .route("/metrics", get(api::metrics_handler))
        .route("/api/v1/status", get(api::status_handler))
        .route("/api/v1/chat", post(api::chat_handler))
        .route("/api/v1/my-ip", get(api::my_ip_handler))
        .route(
            "/api/v1/donors",
            get(api::donors_list).post(api::donors_add),
        )
        .route("/api/v1/csv-to-parquet", post(api::csv_to_parquet_handler))
        .route("/api/v1/parquet-to-csv", post(api::parquet_to_csv_handler))
        .route("/api/v1/compare", post(api::compare_handler))
        .route(
            "/api/v1/fix-encoding/preview",
            post(api::encoding_preview_handler),
        )
        .route("/api/v1/fix-encoding", post(api::encoding_fix_handler))
        .fallback_service(ServeDir::new(PathBuf::from(&static_dir)))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES + 1024 * 1024))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("DataCooking.dev listening on http://{addr}");
    tracing::info!("health:   http://{addr}/health");
    tracing::info!("metrics:  http://{addr}/metrics");
    tracing::info!("static:   {static_dir}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
}
