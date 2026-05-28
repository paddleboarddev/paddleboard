// PaddleBoard: settings schema for OpenTelemetry trace export. Lives in
// settings_content so the field can be deserialized like any other Zed
// setting; the typed wrapper + init logic lives in
// `paddleboard_otel_settings` to keep this file's drift surface small.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings_macros::{MergeFrom, with_fallible_options};

#[with_fallible_options]
#[derive(Debug, Default, PartialEq, Clone, Serialize, Deserialize, JsonSchema, MergeFrom)]
pub struct PaddleboardOtelContent {
    /// Whether OpenTelemetry tracing is enabled. When `true`, PaddleBoard
    /// installs a tracing subscriber that exports spans via OTLP.
    /// Can also be enabled via `PADDLEBOARD_OTEL_ENABLED=1`.
    ///
    /// Default: false
    pub enabled: Option<bool>,

    /// OTLP endpoint to export traces to. Overridden by the standard
    /// `OTEL_EXPORTER_OTLP_ENDPOINT` environment variable when set.
    ///
    /// Default: "http://localhost:4317"
    pub endpoint: Option<String>,

    /// The OTLP protocol to use for exporting traces.
    ///
    /// - `"grpc"` (default): gRPC transport (port 4317).
    /// - `"http"`: HTTP/protobuf transport (port 4318).
    pub protocol: Option<PaddleboardOtelProtocolContent>,

    /// Service name reported in traces.
    ///
    /// Default: "paddleboard"
    pub service_name: Option<String>,
}

#[derive(
    Debug, Copy, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, MergeFrom,
)]
#[serde(rename_all = "snake_case")]
pub enum PaddleboardOtelProtocolContent {
    #[default]
    Grpc,
    Http,
}
