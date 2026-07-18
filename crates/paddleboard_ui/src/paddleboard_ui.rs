//! PaddleBoard's shared UI kit: the chrome-visibility settings for
//! PaddleBoard-added dock buttons and status items, plus small components and
//! layout constants used across PaddleBoard surfaces so they read as one
//! product.

use gpui::{App, IntoElement as _};
use settings::{RegisterSetting, Settings};

/// Force-link the crate so the `RegisterSetting` inventory entry for
/// [`PaddleboardUiSettings`] is reachable.
pub fn init(_cx: &mut App) {}

/// Visibility of PaddleBoard-added chrome. Every toggle defaults to shown;
/// each panel button and status item also offers right-click → Hide, which
/// persists `false` here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, RegisterSetting)]
pub struct PaddleboardUiSettings {
    pub browser_button: bool,
    pub llm_picker_button: bool,
    pub orchestration_button: bool,
    pub manifest_button: bool,
    pub sandbox_status: bool,
    pub mcp_status: bool,
    pub usage_status: bool,
    pub set_sail_status: bool,
}

impl Default for PaddleboardUiSettings {
    fn default() -> Self {
        Self {
            browser_button: true,
            llm_picker_button: true,
            orchestration_button: true,
            manifest_button: true,
            sandbox_status: true,
            mcp_status: true,
            usage_status: true,
            set_sail_status: true,
        }
    }
}

impl PaddleboardUiSettings {
    /// Convenience accessor so consumers don't need the `Settings` trait in
    /// scope (or a direct `settings` dependency) just to read a toggle.
    pub fn get(cx: &App) -> Self {
        *<Self as Settings>::get_global(cx)
    }
}

impl Settings for PaddleboardUiSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let defaults = Self::default();
        let Some(content) = content.paddleboard_ui.as_ref() else {
            return defaults;
        };
        Self {
            browser_button: content.browser_button.unwrap_or(defaults.browser_button),
            llm_picker_button: content
                .llm_picker_button
                .unwrap_or(defaults.llm_picker_button),
            orchestration_button: content
                .orchestration_button
                .unwrap_or(defaults.orchestration_button),
            manifest_button: content.manifest_button.unwrap_or(defaults.manifest_button),
            sandbox_status: content.sandbox_status.unwrap_or(defaults.sandbox_status),
            mcp_status: content.mcp_status.unwrap_or(defaults.mcp_status),
            usage_status: content.usage_status.unwrap_or(defaults.usage_status),
            set_sail_status: content.set_sail_status.unwrap_or(defaults.set_sail_status),
        }
    }
}

/// Modal width scale for PaddleBoard modals: S/M/L instead of per-modal
/// magic numbers.
pub mod modal_width {
    /// Small: focused single-purpose dialogs (pickers, short forms).
    pub const SMALL: f32 = 28.0;
    /// Medium: the default for configuration modals.
    pub const MEDIUM: f32 = 34.0;
    /// Large: browsing/catalog surfaces.
    pub const LARGE: f32 = 48.0;
}

/// The shared selectable option row PaddleBoard modals use for radio-style
/// choices (deploy target, persona, template, …): a leading Check/Circle
/// indicator, selected/hover backgrounds, one padding token.
///
/// Extracted from three hand-rolled copies (Set Sail custom target, Scion
/// persona + template selectors) so option rows look identical everywhere.
#[derive(gpui::IntoElement)]
pub struct SelectableRow {
    id: gpui::ElementId,
    selected: bool,
    content: gpui::AnyElement,
    on_click: Option<Box<dyn Fn(&gpui::ClickEvent, &mut gpui::Window, &mut App) + 'static>>,
}

impl SelectableRow {
    pub fn new(id: impl Into<gpui::ElementId>, selected: bool) -> Self {
        Self {
            id: id.into(),
            selected,
            content: gpui::Empty.into_any_element(),
            on_click: None,
        }
    }

    pub fn child(mut self, content: impl gpui::IntoElement) -> Self {
        self.content = content.into_any_element();
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl ui::RenderOnce for SelectableRow {
    fn render(self, _window: &mut gpui::Window, cx: &mut App) -> impl gpui::IntoElement {
        use gpui::prelude::*;
        use ui::prelude::*;

        let colors = cx.theme().colors();
        h_flex()
            .id(self.id)
            .px_2()
            .py_1()
            .gap_2()
            .rounded_sm()
            .cursor_pointer()
            .when(self.selected, |this| this.bg(colors.element_selected))
            .when(!self.selected, |this| {
                this.hover(|this| this.bg(colors.element_hover))
            })
            .when_some(self.on_click, |this, on_click| this.on_click(on_click))
            .child(
                Icon::new(if self.selected {
                    IconName::Check
                } else {
                    IconName::Circle
                })
                .size(IconSize::Small)
                .color(if self.selected {
                    Color::Accent
                } else {
                    Color::Muted
                }),
            )
            .child(self.content)
    }
}
