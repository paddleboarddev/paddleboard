use gpui::App;
use serde::Deserialize;
use settings::{RegisterSetting, Settings};

/// Force-link the crate so the `RegisterSetting` inventory entry for
/// [`OtelSettings`] is reachable.
pub fn init(_cx: &mut App) {}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OtelProtocol {
    #[default]
    Grpc,
    Http,
}

#[derive(Debug, Clone, PartialEq, RegisterSetting)]
pub struct OtelSettings {
    pub enabled: bool,
    pub endpoint: String,
    pub protocol: OtelProtocol,
    pub service_name: String,
}

impl Default for OtelSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: "http://localhost:4317".to_string(),
            protocol: OtelProtocol::Grpc,
            service_name: "paddleboard".to_string(),
        }
    }
}

impl Settings for OtelSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let Some(content) = content.paddleboard_otel.as_ref() else {
            return Self::default();
        };
        Self {
            enabled: content.enabled.unwrap_or(false),
            endpoint: content
                .endpoint
                .clone()
                .unwrap_or_else(|| "http://localhost:4317".to_string()),
            protocol: content
                .protocol
                .map(|p| match p {
                    settings::PaddleboardOtelProtocolContent::Grpc => OtelProtocol::Grpc,
                    settings::PaddleboardOtelProtocolContent::Http => OtelProtocol::Http,
                })
                .unwrap_or_default(),
            service_name: content
                .service_name
                .clone()
                .unwrap_or_else(|| "paddleboard".to_string()),
        }
    }
}
