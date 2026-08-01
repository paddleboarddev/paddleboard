// PaddleBoard: a status-bar item for in-app updates.
//
// This exists because PaddleBoard has no visible surface for update progress at
// all. Upstream's only one is a `"Updating..."` label in `title_bar.rs`, and it
// sits inside a `match` arm on `client::Status::UpgradeRequired` — a Zed Cloud
// collab connection state. PaddleBoard never connects to Zed Cloud, so that arm
// is unreachable here and the entire update ran silently: no indicator while
// downloading, nothing on completion, no prompt to restart. The only evidence an
// update had happened was in the log.
//
// Deliberately in its own crate rather than an edit to `title_bar.rs` or
// `activity_indicator.rs`: both are upstream-shaped and this is net-new
// behaviour, so an upstream merge cannot conflict with it.

use auto_update::{AutoUpdateStatus, AutoUpdater};
use gpui::{App, Entity, Window};
use settings::SettingsStore;
use ui::{Button, ButtonCommon, Clickable, Icon, IconName, IconSize, Label, LabelSize, prelude::*};
use ui::{Color, CommonAnimationExt, Tooltip};
use workspace::{HideStatusItem, StatusItemView};

pub struct UpdateStatusItem {
    updater: Option<Entity<AutoUpdater>>,
}

impl UpdateStatusItem {
    pub fn new(cx: &mut Context<Self>) -> Self {
        // The updater is a global created during `auto_update::init`, so it is
        // already present by the time the status bar is assembled.
        let updater = AutoUpdater::get(cx);

        if let Some(updater) = &updater {
            // Repaint on every status transition — Checking → Downloading →
            // Installing → Updated — including each progress tick, which is what
            // makes the percentage move rather than jump.
            cx.observe(updater, |_, _, cx| cx.notify()).detach();
        }

        // The hide toggle lives in settings, so a change there must repaint.
        cx.observe_global::<SettingsStore>(|_, cx| cx.notify())
            .detach();

        Self { updater }
    }

    fn status(&self, cx: &App) -> Option<AutoUpdateStatus> {
        Some(self.updater.as_ref()?.read(cx).status())
    }
}

impl StatusItemView for UpdateStatusItem {
    fn set_active_pane_item(
        &mut self,
        _active_pane_item: Option<&dyn workspace::ItemHandle>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    fn hide_setting(&self, _cx: &App) -> Option<HideStatusItem> {
        Some(HideStatusItem::new(|settings| {
            settings.paddleboard_ui.get_or_insert_default().update_status = Some(false);
        }))
    }
}

impl Render for UpdateStatusItem {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !paddleboard_ui::PaddleboardUiSettings::get(cx).update_status {
            return gpui::Empty.into_any_element();
        }

        // Idle is the overwhelmingly common state — an hourly poll that finds
        // nothing. Rendering nothing then is the whole point: this appears only
        // while an update is actually in flight, and stays afterwards only to
        // tell you a restart is pending. It is not another permanent status item.
        let Some(status) = self.status(cx) else {
            return gpui::Empty.into_any_element();
        };

        match status {
            AutoUpdateStatus::Idle | AutoUpdateStatus::Errored { .. } => {
                // Errors are deliberately silent here. An automatic check that
                // fails offline is not worth a status-bar item; `auto_update:
                // Check` surfaces the error for anyone who asked for one.
                gpui::Empty.into_any_element()
            }

            AutoUpdateStatus::Checking => spinner_label("Checking for updates…", cx),

            AutoUpdateStatus::Downloading { progress, .. } => {
                // `progress` is None until Content-Length is known, so the label
                // has to read sensibly without a percentage.
                let label = match progress {
                    Some(fraction) => {
                        format!("Downloading update… {}%", (fraction * 100.0) as u8)
                    }
                    None => "Downloading update…".to_string(),
                };
                spinner_label(label, cx)
            }

            AutoUpdateStatus::Installing { .. } => spinner_label("Installing update…", cx),

            AutoUpdateStatus::Updated { version } => {
                // The one state that needs an action: the new version is on disk
                // but the running process is still the old one. Without this the
                // update completes invisibly and the user keeps running the old
                // build indefinitely, which is exactly what happened on v0.2.2.
                let tooltip = format!("PaddleBoard {version} is installed and starts on restart");
                Button::new("update-status-restart", "Restart to update")
                    .label_size(LabelSize::Small)
                    .color(Color::Accent)
                    .tooltip(move |_window, cx| Tooltip::simple(tooltip.clone(), cx))
                    .on_click(|_, _window, cx| workspace::reload(cx))
                    .into_any_element()
            }
        }
    }
}

fn spinner_label(text: impl Into<SharedString>, _cx: &mut App) -> gpui::AnyElement {
    h_flex()
        .gap_1()
        .child(
            Icon::new(IconName::ArrowCircle)
                .size(IconSize::Small)
                .color(Color::Muted)
                .with_rotate_animation(2),
        )
        .child(Label::new(text).size(LabelSize::Small).color(Color::Muted))
        .into_any_element()
}
