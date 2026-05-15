//! UI surface for PaddleBoard's sandbox prerequisites (Podman + gVisor `runsc`).
//!
//! Three pieces hang together here:
//! * `SandboxPrereqs` — `gpui::Global` holding the latest `SandboxStatus`.
//!   Updated asynchronously via a Tokio-bridged probe; views observe it and
//!   re-render on change.
//! * `SandboxStatusItem` — a status-bar entry showing a shield icon colored
//!   by severity. Click dispatches `OpenSandboxPrereqs`.
//! * `SandboxPrereqsModal` — the full status + install-steps view with a
//!   per-step copy-to-clipboard button and a Refresh control.

use gpui::{
    Action, App, ClickEvent, ClipboardItem, DismissEvent, EventEmitter, FocusHandle, Focusable,
    MouseDownEvent, Render, SharedString, actions,
};
use gpui_tokio::Tokio;
use paddleboard_sandbox_prereqs::{GvisorStatus, Os, PodmanStatus, SandboxStatus};
use ui::{Tooltip, prelude::*};
use workspace::{ModalView, StatusItemView, Workspace};

actions!(
    paddleboard,
    [
        /// Opens the sandbox prerequisites status modal.
        OpenSandboxPrereqs
    ]
);

/// Initialize the UI surface. Registers the `SandboxPrereqs` global, kicks
/// off the first probe in the background, and wires the `OpenSandboxPrereqs`
/// action to every workspace as it opens.
pub fn init(cx: &mut App) {
    SandboxPrereqs::init(cx);

    cx.observe_new(|workspace: &mut Workspace, _window, _cx| {
        workspace.register_action(|workspace, _: &OpenSandboxPrereqs, window, cx| {
            SandboxPrereqsModal::toggle(workspace, window, cx);
        });
    })
    .detach();
}

#[derive(Default)]
pub struct SandboxPrereqs {
    status: Option<SandboxStatus>,
    refreshing: bool,
}

impl gpui::Global for SandboxPrereqs {}

impl SandboxPrereqs {
    fn init(cx: &mut App) {
        cx.set_global(SandboxPrereqs::default());
        Self::refresh(cx);
    }

    pub fn status(cx: &App) -> Option<&SandboxStatus> {
        cx.global::<SandboxPrereqs>().status.as_ref()
    }

    pub fn is_refreshing(cx: &App) -> bool {
        cx.global::<SandboxPrereqs>().refreshing
    }

    pub fn refresh(cx: &mut App) {
        cx.update_global::<SandboxPrereqs, _>(|prereqs, _| {
            prereqs.refreshing = true;
        });

        let task = Tokio::spawn(cx, async { paddleboard_sandbox_prereqs::check().await });

        cx.spawn(async move |cx| {
            let status = task.await.ok();
            cx.update(|cx| {
                cx.update_global::<SandboxPrereqs, _>(|prereqs, _| {
                    if let Some(status) = status {
                        prereqs.status = Some(status);
                    }
                    prereqs.refreshing = false;
                });
            });
        })
        .detach();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Severity {
    Unknown,
    Ok,
    Warning,
    Error,
}

impl Severity {
    fn from_status(status: Option<&SandboxStatus>) -> Severity {
        let Some(status) = status else {
            return Severity::Unknown;
        };
        match (&status.podman, &status.gvisor) {
            (PodmanStatus::Missing, _) | (PodmanStatus::InstalledNotRunning { .. }, _) => {
                Severity::Error
            }
            (PodmanStatus::Ready { .. }, GvisorStatus::Available)
            | (PodmanStatus::Ready { .. }, GvisorStatus::NotApplicable { .. }) => Severity::Ok,
            (PodmanStatus::Ready { .. }, _) => Severity::Warning,
        }
    }

    fn color(self) -> Color {
        match self {
            Severity::Unknown => Color::Muted,
            Severity::Ok => Color::Success,
            Severity::Warning => Color::Warning,
            Severity::Error => Color::Error,
        }
    }
}

pub struct SandboxStatusItem;

impl SandboxStatusItem {
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.observe_global::<SandboxPrereqs>(|_, cx| cx.notify())
            .detach();
        Self
    }
}

impl StatusItemView for SandboxStatusItem {
    fn set_active_pane_item(
        &mut self,
        _active_pane_item: Option<&dyn workspace::ItemHandle>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }
}

impl Render for SandboxStatusItem {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = SandboxPrereqs::status(cx);
        let severity = Severity::from_status(status);
        let tooltip_text = severity_tooltip(severity, status);

        IconButton::new("sandbox-prereqs-status", IconName::Box)
            .icon_size(IconSize::Small)
            .icon_color(severity.color())
            .tooltip(move |_window, cx| {
                Tooltip::for_action(tooltip_text.clone(), &OpenSandboxPrereqs, cx)
            })
            .on_click(|_, window, cx| {
                window.dispatch_action(OpenSandboxPrereqs.boxed_clone(), cx);
            })
    }
}

fn severity_tooltip(severity: Severity, status: Option<&SandboxStatus>) -> SharedString {
    match severity {
        Severity::Unknown => "Sandbox: checking…".into(),
        Severity::Ok => "Sandbox: ready".into(),
        Severity::Warning => match status.map(|s| &s.gvisor) {
            Some(GvisorStatus::NotConfigured) => "Sandbox: gVisor not configured".into(),
            _ => "Sandbox: degraded".into(),
        },
        Severity::Error => match status.map(|s| &s.podman) {
            Some(PodmanStatus::Missing) => "Sandbox: Podman not installed".into(),
            Some(PodmanStatus::InstalledNotRunning { .. }) => "Sandbox: Podman not running".into(),
            _ => "Sandbox: unavailable".into(),
        },
    }
}

pub struct SandboxPrereqsModal {
    focus_handle: FocusHandle,
}

impl SandboxPrereqsModal {
    pub fn toggle(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
        workspace.toggle_modal(window, cx, |_window, cx| {
            cx.observe_global::<SandboxPrereqs>(|_, cx| cx.notify())
                .detach();
            SandboxPrereqsModal {
                focus_handle: cx.focus_handle(),
            }
        });
    }

    fn cancel(&mut self, _: &menu::Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }
}

impl EventEmitter<DismissEvent> for SandboxPrereqsModal {}

impl Focusable for SandboxPrereqsModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ModalView for SandboxPrereqsModal {}

impl Render for SandboxPrereqsModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = SandboxPrereqs::status(cx).cloned();
        let refreshing = SandboxPrereqs::is_refreshing(cx);
        let os = Os::detect();
        let instructions = status
            .as_ref()
            .map(|s| paddleboard_sandbox_prereqs::install_instructions(s, os));

        // Header row with title + close button.
        let header = h_flex()
            .w_full()
            .justify_between()
            .child(Headline::new("Sandbox Prerequisites").size(HeadlineSize::Medium))
            .child(
                IconButton::new("sandbox-prereqs-close", IconName::Close).on_click(cx.listener(
                    |_, _: &ClickEvent, _window, cx| {
                        cx.emit(DismissEvent);
                    },
                )),
            );

        // Status rows for Podman + gVisor.
        let podman_row = {
            let (icon, color, label): (IconName, Color, SharedString) = match status
                .as_ref()
                .map(|s| &s.podman)
            {
                None => (IconName::Ellipsis, Color::Muted, "Podman: checking…".into()),
                Some(PodmanStatus::Missing) => (
                    IconName::XCircle,
                    Color::Error,
                    "Podman: not found on PATH".into(),
                ),
                Some(PodmanStatus::InstalledNotRunning { version }) => (
                    IconName::Warning,
                    Color::Warning,
                    format!("Podman: {version} — daemon unreachable").into(),
                ),
                Some(PodmanStatus::Ready { version }) => (
                    IconName::Check,
                    Color::Success,
                    format!("Podman: {version}").into(),
                ),
            };
            h_flex()
                .gap_2()
                .child(Icon::new(icon).color(color).size(IconSize::Small))
                .child(Label::new(label))
        };
        let gvisor_row = {
            let (icon, color, label): (IconName, Color, SharedString) = match status
                .as_ref()
                .map(|s| &s.gvisor)
            {
                None => (IconName::Ellipsis, Color::Muted, "gVisor: checking…".into()),
                Some(GvisorStatus::Available) => (
                    IconName::Check,
                    Color::Success,
                    "gVisor: runsc registered with Podman".into(),
                ),
                Some(GvisorStatus::NotConfigured) => (
                    IconName::XCircle,
                    Color::Warning,
                    "gVisor: runsc not registered with Podman".into(),
                ),
                Some(GvisorStatus::NotApplicable { reason }) => (
                    IconName::Info,
                    Color::Muted,
                    format!("gVisor: {reason}").into(),
                ),
                Some(GvisorStatus::Unknown) => (
                    IconName::Ellipsis,
                    Color::Muted,
                    "gVisor: status unknown (Podman unreachable)".into(),
                ),
            };
            h_flex()
                .gap_2()
                .child(Icon::new(icon).color(color).size(IconSize::Small))
                .child(Label::new(label))
        };
        let status_block = v_flex().gap_1().child(podman_row).child(gvisor_row);

        // Install steps block. Each step is a numbered description; if the
        // step carries a copy-pasteable command we render it inline as a
        // styled monospace block with a Copy button on the right.
        let bg_color = cx.theme().colors().editor_background;
        let instructions_block = instructions.map(|inst| {
            let title = inst.title.clone();
            let doc_url = inst.doc_url;
            v_flex()
                .gap_3()
                .child(Headline::new(title).size(HeadlineSize::XSmall))
                .children(inst.steps.into_iter().enumerate().map(|(i, step)| {
                    v_flex()
                        .gap_1()
                        .child(
                            h_flex()
                                .gap_2()
                                .items_start()
                                .child(
                                    Label::new(format!("{}.", i + 1))
                                        .size(LabelSize::Small)
                                        .color(Color::Muted),
                                )
                                .child(Label::new(step.description).size(LabelSize::Small)),
                        )
                        .when_some(step.command, |this, command| {
                            let command_for_copy = command.clone();
                            this.child(
                                h_flex()
                                    .pl_4()
                                    .gap_1()
                                    .items_start()
                                    .child(
                                        div()
                                            .flex_1()
                                            .px_2()
                                            .py_1()
                                            .rounded_sm()
                                            .bg(bg_color)
                                            .child(
                                                Label::new(SharedString::from(command))
                                                    .size(LabelSize::Small),
                                            ),
                                    )
                                    .child(
                                        IconButton::new(
                                            SharedString::from(format!("sandbox-copy-{i}")),
                                            IconName::Copy,
                                        )
                                        .icon_size(IconSize::Small)
                                        .on_click(cx.listener(move |_, _, _window, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                command_for_copy.clone(),
                                            ));
                                        })),
                                    ),
                            )
                        })
                }))
                .when_some(doc_url, |this, url| {
                    this.child(
                        Label::new(format!("More: {url}"))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                })
        });

        let refresh_button = Button::new(
            "sandbox-prereqs-refresh",
            if refreshing { "Checking…" } else { "Refresh" },
        )
        .disabled(refreshing)
        .on_click(|_, _window, cx| SandboxPrereqs::refresh(cx));

        v_flex()
            .id("sandbox-prereqs-modal")
            .key_context("SandboxPrereqsModal")
            .w(rems(36.))
            .elevation_3(cx)
            .track_focus(&self.focus_handle(cx))
            .on_action(cx.listener(Self::cancel))
            .on_any_mouse_down(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                this.focus_handle.focus(window, cx);
            }))
            .child(v_flex().p_4().gap_3().child(header).child(status_block))
            .child(ui::Divider::horizontal())
            .child(
                v_flex()
                    .id("sandbox-prereqs-steps")
                    .p_4()
                    .gap_3()
                    .max_h(rems(24.))
                    .overflow_y_scroll()
                    .children(instructions_block),
            )
            .child(ui::Divider::horizontal())
            .child(h_flex().p_3().justify_end().child(refresh_button))
    }
}
