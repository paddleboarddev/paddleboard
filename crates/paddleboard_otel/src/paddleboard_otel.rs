use std::sync::OnceLock;

use gpui::{App, Global};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig as _;
use opentelemetry_sdk::trace::SdkTracerProvider;
use paddleboard_otel_settings::{OtelProtocol, OtelSettings};
use settings::Settings;
use tracing_subscriber::prelude::*;

static OTEL_INITIALIZED: OnceLock<bool> = OnceLock::new();

struct OtelGuard {
    provider: SdkTracerProvider,
}

impl Global for OtelGuard {}

impl Drop for OtelGuard {
    fn drop(&mut self) {
        if let Err(err) = self.provider.shutdown() {
            log::error!("paddleboard_otel: shutdown error: {err}");
        }
    }
}

pub fn init(cx: &mut App) {
    if OTEL_INITIALIZED.set(true).is_err() {
        return;
    }

    if std::env::var_os("ZTRACING").is_some()
        || std::env::var_os("ZTRACING_WITH_MEMORY").is_some()
    {
        log::info!("paddleboard_otel: skipping — Tracy (ztracing) is active");
        return;
    }

    let env_enabled = std::env::var("PADDLEBOARD_OTEL_ENABLED")
        .ok()
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    let settings = OtelSettings::get_global(cx);
    if !settings.enabled && !env_enabled {
        log::debug!("paddleboard_otel: disabled by settings and env");
        return;
    }

    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .unwrap_or_else(|_| settings.endpoint.clone());

    let service_name = std::env::var("OTEL_SERVICE_NAME")
        .unwrap_or_else(|_| settings.service_name.clone());

    let protocol = settings.protocol;

    match try_init_pipeline(&endpoint, &service_name, protocol) {
        Ok(guard) => {
            cx.set_global(guard);
            log::info!("paddleboard_otel: pipeline initialized, exporting to {endpoint}");
        }
        Err(err) => {
            log::error!("paddleboard_otel: failed to initialize pipeline: {err:#}");
        }
    }
}

fn try_init_pipeline(
    endpoint: &str,
    service_name: &str,
    protocol: OtelProtocol,
) -> anyhow::Result<OtelGuard> {
    use opentelemetry::KeyValue;
    use opentelemetry_sdk::Resource;

    let resource = Resource::builder()
        .with_attributes([KeyValue::new("service.name", service_name.to_string())])
        .build();

    let exporter = match protocol {
        OtelProtocol::Grpc => opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint)
            .build()
            .map_err(|err| anyhow::anyhow!("OTLP gRPC exporter: {err}"))?,
        OtelProtocol::Http => {
            log::warn!(
                "paddleboard_otel: HTTP protocol requested but only gRPC is compiled in; \
                 falling back to gRPC"
            );
            opentelemetry_otlp::SpanExporter::builder()
                .with_tonic()
                .with_endpoint(endpoint)
                .build()
                .map_err(|err| anyhow::anyhow!("OTLP gRPC exporter (fallback): {err}"))?
        }
    };

    let provider = SdkTracerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(exporter)
        .build();

    let tracer = provider.tracer("paddleboard");
    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(otel_layer))
        .map_err(|err| anyhow::anyhow!("failed to set global subscriber: {err}"))?;

    Ok(OtelGuard { provider })
}
