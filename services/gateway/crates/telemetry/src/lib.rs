//! Tracing and OpenTelemetry setup, plus the per-request timing breakdown that every
//! response carries back to the UI.

mod timing;

pub use timing::ServerTiming;

use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig as _;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::{EnvFilter, Layer as _, fmt};

#[derive(Debug, thiserror::Error)]
pub enum TelemetryError {
    #[error("failed to build the OTLP exporter: {0}")]
    Exporter(#[from] opentelemetry_otlp::ExporterBuildError),
    #[error("a global tracing subscriber is already installed: {0}")]
    AlreadyInstalled(#[from] tracing_subscriber::util::TryInitError),
}

#[derive(Debug, Clone)]
pub struct TelemetryConfig {
    pub service_name: String,
    /// Emit one JSON object per log line (containers) instead of human-readable text.
    pub json: bool,
    /// When set, spans are also exported over OTLP/gRPC to this endpoint.
    pub otlp_endpoint: Option<String>,
}

/// Flushes buffered spans when dropped; keep it alive for the life of the process.
pub struct TelemetryGuard {
    provider: Option<SdkTracerProvider>,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        if let Some(provider) = self.provider.take()
            && let Err(err) = provider.shutdown()
        {
            eprintln!("failed to flush OpenTelemetry spans: {err}");
        }
    }
}

/// Installs the global tracing subscriber. `RUST_LOG` controls filtering (default `info`).
pub fn init(config: &TelemetryConfig) -> Result<TelemetryGuard, TelemetryError> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let fmt_layer = if config.json {
        fmt::layer()
            .json()
            .flatten_event(true)
            .with_current_span(true)
            .boxed()
    } else {
        fmt::layer().compact().boxed()
    };

    let (otel_layer, provider) = match config.otlp_endpoint.as_deref() {
        Some(endpoint) if !endpoint.is_empty() => {
            let exporter = opentelemetry_otlp::SpanExporter::builder()
                .with_tonic()
                .with_endpoint(endpoint)
                .build()?;
            let provider = SdkTracerProvider::builder()
                .with_batch_exporter(exporter)
                .with_resource(
                    Resource::builder()
                        .with_service_name(config.service_name.clone())
                        .build(),
                )
                .build();
            let tracer = provider.tracer(config.service_name.clone());
            (
                Some(tracing_opentelemetry::layer().with_tracer(tracer)),
                Some(provider),
            )
        }
        _ => (None, None),
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .with(otel_layer)
        .try_init()?;

    Ok(TelemetryGuard { provider })
}
