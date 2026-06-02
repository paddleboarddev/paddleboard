// PaddleBoard: typed wrapper + registration for the Scion integration setting.
// The deserializable schema lives in `settings_content::PaddleboardScionContent`.

use gpui::App;
use settings::{RegisterSetting, Settings};

/// Force-link the crate so the `RegisterSetting` inventory entry for
/// [`ScionSettings`] is reachable.
pub fn init(_cx: &mut App) {}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, RegisterSetting)]
pub struct ScionSettings {
    /// Whether the Scion integration is active. Defaults to `false` — installing
    /// the `scion` CLI alone no longer auto-activates the integration.
    pub enabled: bool,
}

impl Settings for ScionSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let Some(content) = content.paddleboard_scion.as_ref() else {
            return Self::default();
        };
        Self {
            enabled: content.enabled.unwrap_or(false),
        }
    }
}
