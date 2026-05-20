//! UI surface for PaddleBoard's sandbox prerequisites (Podman + gVisor `runsc`).
//!
//! Two pieces live here:
//! * `SandboxStatusItem` — a status-bar entry showing a shield icon colored
//!   by severity. Click dispatches `OpenSandboxPrereqs`.
//! * `SandboxPrereqsModal` — the full status + install-steps view with a
//!   per-step copy-to-clipboard button and a Refresh control.
//!
//! The cached probe status lives in `paddleboard_sandbox_prereqs_state` so
//! non-UI consumers can read it without taking a `workspace` dependency.

use gpui::{
    Action, App, ClickEvent, ClipboardItem, DismissEvent, EventEmitter, FocusHandle, Focusable,
    MouseDownEvent, Render, SharedString,
};
use paddleboard_sandbox_prereqs::{CommandKind, GvisorStatus, Os, PodmanStatus, SandboxStatus};
use paddleboard_sandbox_prereqs_state::{OpenSandboxPrereqs, SandboxPrereqs};
use ui::{Tooltip, prelude::*};
use workspace::{HideStatusItem, ModalView, StatusItemView, Workspace};

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

    fn hide_setting(&self, _cx: &App) -> Option<HideStatusItem> {
        None
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
        let host_os = os;
        let instructions_block = instructions.map(|inst| {
            let title = inst.title.clone();
            let doc_url = inst.doc_url;
            v_flex()
                .gap_3()
                .child(Headline::new(title).size(HeadlineSize::XSmall))
                .children(inst.steps.into_iter().enumerate().map(|(i, step)| {
                    v_flex()
                        .w_full()
                        .gap_1()
                        .child(
                            h_flex()
                                .w_full()
                                .gap_2()
                                .items_start()
                                .child(
                                    Label::new(format!("{}.", i + 1))
                                        .size(LabelSize::Small)
                                        .color(Color::Muted),
                                )
                                .child(
                                    div().flex_1().min_w_0().child(
                                        Label::new(step.description).size(LabelSize::Small),
                                    ),
                                ),
                        )
                        .when_some(step.command, |this, command| {
                            let command_for_copy = command.clone();
                            let command_for_run = command.clone();
                            let is_runnable = step.command_kind == CommandKind::Shell;
                            let can_open_terminal =
                                is_runnable && matches!(host_os, Os::MacOs | Os::Linux);
                            this.child(
                                h_flex()
                                    .w_full()
                                    .pl_4()
                                    .gap_1()
                                    .items_start()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
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
                                        .tooltip(Tooltip::text("Copy to clipboard"))
                                        .on_click(cx.listener(move |_, _, _window, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                command_for_copy.clone(),
                                            ));
                                        })),
                                    )
                                    .when(can_open_terminal, |this| {
                                        this.child(
                                            IconButton::new(
                                                SharedString::from(format!("sandbox-run-{i}")),
                                                IconName::Terminal,
                                            )
                                            .icon_size(IconSize::Small)
                                            .tooltip(Tooltip::text(
                                                "Run in a new Terminal window",
                                            ))
                                            .on_click(cx.listener(
                                                move |_, _, _window, cx| {
                                                    if let Err(err) = open_in_terminal(
                                                        &command_for_run,
                                                    ) {
                                                        log::warn!(
                                                            "Failed to open Terminal for sandbox step: {err}"
                                                        );
                                                    }
                                                    SandboxPrereqs::refresh(cx);
                                                },
                                            )),
                                        )
                                    }),
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

/// Open the user's Terminal with `command` queued to run. Writes a small
/// wrapper script that runs the command, prints its exit code, waits on
/// Enter, and then removes itself; the OS-specific arm launches a terminal
/// emulator pointed at the script. Currently supports macOS and Linux —
/// callers must gate on `Os::MacOs` or `Os::Linux`.
fn open_in_terminal(command: &str) -> std::io::Result<()> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let path = write_wrapper_script(command)?;
        #[cfg(target_os = "macos")]
        {
            util::command::new_command("open")
                .arg("-a")
                .arg("Terminal")
                .arg(&path)
                .spawn()?;
            Ok(())
        }
        #[cfg(target_os = "linux")]
        {
            spawn_linux_terminal(&path)
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = command;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Run in Terminal is currently only supported on macOS and Linux",
        ))
    }
}

/// Write a self-deleting wrapper script that runs `command`, prints the exit
/// code, waits on Enter, then removes itself. Used by both the macOS and
/// Linux arms of `open_in_terminal`.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn write_wrapper_script(command: &str) -> std::io::Result<std::path::PathBuf> {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let path = std::env::temp_dir().join(format!("paddleboard-sandbox-{pid}-{nanos}.sh"));

    // Heredoc-quoted preamble keeps the user's command intact. The trailing
    // `rm -- "$0"` cleans the script up after the user dismisses the window.
    let script = format!(
        "#!/usr/bin/env bash\n\
         echo '=== PaddleBoard sandbox step ==='\n\
         echo\n\
         {command}\n\
         status=$?\n\
         echo\n\
         if [ $status -eq 0 ]; then\n\
           echo '=== Done (exit 0). Press Enter to close. ==='\n\
         else\n\
           echo \"=== Failed (exit $status). Press Enter to close. ===\"\n\
         fi\n\
         read -r _\n\
         rm -- \"$0\" 2>/dev/null\n"
    );

    let mut file = std::fs::File::create(&path)?;
    file.write_all(script.as_bytes())?;
    file.sync_all()?;
    let mut perms = std::fs::metadata(&path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms)?;

    Ok(path)
}

/// Launch the user's terminal emulator pointed at `script_path`. Linux has
/// no canonical "Terminal" — we walk a priority list of well-known emulators
/// and use the first one found on `$PATH`, falling back to `xterm`. Each
/// candidate carries its own argv shape because invocation conventions vary
/// (`gnome-terminal --`, `kitty <script>`, `konsole -e <script>`, …).
#[cfg(target_os = "linux")]
fn spawn_linux_terminal(script_path: &std::path::Path) -> std::io::Result<()> {
    // (program, args-before-script). `xdg-terminal-exec` is the XDG spec
    // helper that defers to the user's preferred terminal; preferring it
    // lets users override the rest of the list via desktop config.
    // `x-terminal-emulator` is Debian/Ubuntu's update-alternatives wrapper.
    const CANDIDATES: &[(&str, &[&str])] = &[
        ("xdg-terminal-exec", &[]),
        ("x-terminal-emulator", &["-e"]),
        ("gnome-terminal", &["--"]),
        ("konsole", &["-e"]),
        ("xfce4-terminal", &["-e"]),
        ("tilix", &["-e"]),
        ("terminator", &["-e"]),
        ("alacritty", &["-e"]),
        ("kitty", &[]),
        ("foot", &[]),
        ("wezterm", &["start", "--"]),
        ("ghostty", &["-e"]),
        ("xterm", &["-e"]),
    ];

    for (program, prefix_args) in CANDIDATES {
        if which::which(program).is_ok() {
            let mut cmd = std::process::Command::new(program);
            cmd.args(*prefix_args);
            cmd.arg(script_path);
            cmd.spawn()?;
            return Ok(());
        }
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "No supported terminal emulator found on PATH \
         (tried gnome-terminal, konsole, xfce4-terminal, alacritty, kitty, foot, wezterm, xterm, …). \
         Copy the command instead.",
    ))
}
