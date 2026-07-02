// PaddleBoard: typed wrapper + registration for the persona system setting.
// The deserializable schema lives in `settings_content::PaddleboardPersonasContent`.

use gpui::App;
use settings::{RegisterSetting, Settings};

/// Force-link the crate so the `RegisterSetting` inventory entry for
/// [`PersonasSettings`] is reachable.
pub fn init(_cx: &mut App) {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, RegisterSetting)]
pub struct PersonasSettings {
    /// Whether the persona system is active. Defaults to `true`; the feature is
    /// inert until a persona file exists, so on-by-default costs nothing.
    pub enabled: bool,
}

impl Default for PersonasSettings {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl Settings for PersonasSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let Some(content) = content.paddleboard_personas.as_ref() else {
            return Self::default();
        };
        Self {
            enabled: content.enabled.unwrap_or(true),
        }
    }
}
