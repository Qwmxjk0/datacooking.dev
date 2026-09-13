use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub fn init_telemetry() -> PrometheusHandle {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "datacooking=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    PrometheusBuilder::new()
        .install_recorder()
        .expect("failed to initialize Prometheus metrics recorder")
}
