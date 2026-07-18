//! UI surface for PaddleBoard's sandbox backend setup.
//!
//! Two pieces live here:
//! * `SandboxStatusItem` — a status-bar entry showing a shield icon colored by
//!   the *active* sandbox tier (derived from the same gate the tools use, so
//!   the shield and gate can never disagree). Click dispatches
//!   `OpenSandboxPrereqs`.
//! * `SandboxPrereqsModal` — a backend picker. The user chooses "Native"
//!   (Apple `container` / libkrun microVM) or "Podman" (Podman + gVisor); each
//!   option is labeled honestly for this host, carries a Terminal-handoff
//!   installer, and persists the choice to `paddleboard_sandbox.preferred_backend`.
//!
//! The cached probe status lives in `paddleboard_sandbox_prereqs_state` so
//! non-UI consumers can read it without taking a `workspace` dependency.

use fs::Fs;
use gpui::{
    Action, App, ClipboardItem, DismissEvent, EventEmitter, FocusHandle, Focusable,
    MouseDownEvent, Render, ScrollHandle, SharedString,
};
use paddleboard_sandbox_prereqs::{
    BackendAvailability, BackendOption, CommandKind, InstallStep, Os, PreferredBackend,
    backend_options,
};
use paddleboard_sandbox_prereqs_state::{OpenSandboxPrereqs, SandboxPrereqs};
use paddleboard_sandbox_settings::{ActiveTier, ActiveTierKind, SandboxSettings, active_tier};
use settings::{
    PaddleboardPreferredBackendContent, Settings, SettingsStore, update_settings_file,
};
use ui::{
    Callout, CommonAnimationExt, Modal, ModalFooter, ModalHeader, Section, Tooltip, prelude::*,
};
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

/// Appearance of the status-bar shield, derived from the active tier. Color
/// signals severity; the tooltip names the chosen backend and — critically —
/// states when only one-shot commands are covered (phase-1 Native tiers), so
/// the shield can't read "green = everything sandboxed" when it isn't
/// (review finding #6).
fn shield_appearance(tier: ActiveTier) -> (Color, SharedString) {
    match tier.kind {
        ActiveTierKind::Unknown => (Color::Muted, "Sandbox: checking…".into()),
        ActiveTierKind::Podman => (Color::Success, "Sandbox: ready — Podman + gVisor".into()),
        ActiveTierKind::AppleContainer => (
            Color::Success,
            "Sandbox: Apple container — one-shot commands only (services & MCP need Podman)".into(),
        ),
        ActiveTierKind::BuiltInKrun => (
            Color::Success,
            "Sandbox: built-in microVM — one-shot commands only (services & MCP need Podman)".into(),
        ),
        ActiveTierKind::Host => (
            Color::Warning,
            "Sandbox: off — commands run on the host (no isolation)".into(),
        ),
        ActiveTierKind::Unavailable => (
            Color::Error,
            "Sandbox: unavailable — open to pick a backend".into(),
        ),
    }
}

fn current_tier(cx: &App) -> ActiveTier {
    active_tier(SandboxPrereqs::status(cx), SandboxSettings::get_global(cx))
}

pub struct SandboxStatusItem {
    workspace: gpui::WeakEntity<Workspace>,
}

impl SandboxStatusItem {
    pub fn new(workspace: &Workspace, cx: &mut Context<Self>) -> Self {
        cx.observe_global::<SandboxPrereqs>(|_, cx| cx.notify())
            .detach();
        // The shield reflects the chosen backend, so a settings change (a new
        // preferred_backend) must repaint it too.
        cx.observe_global::<SettingsStore>(|_, cx| cx.notify())
            .detach();
        Self {
            workspace: workspace.weak_handle(),
        }
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
        Some(HideStatusItem::new(|settings| {
            settings.paddleboard_ui.get_or_insert_default().sandbox_status = Some(false);
        }))
    }
}

impl Render for SandboxStatusItem {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // PaddleBoard Glowup: hideable, and only shown while a project is open —
        // the sandbox tier is meaningless without one.
        let project_open = self.workspace.upgrade().is_some_and(|workspace| {
            workspace
                .read(cx)
                .project()
                .read(cx)
                .visible_worktrees(cx)
                .next()
                .is_some()
        });
        if !paddleboard_ui::PaddleboardUiSettings::get(cx).sandbox_status || !project_open {
            return gpui::Empty.into_any_element();
        }

        let (color, tooltip_text) = shield_appearance(current_tier(cx));

        IconButton::new("sandbox-prereqs-status", IconName::Box)
            .icon_size(IconSize::Small)
            .icon_color(color)
            .tooltip(move |_window, cx| {
                Tooltip::for_action(tooltip_text.clone(), &OpenSandboxPrereqs, cx)
            })
            .on_click(|_, window, cx| {
                window.dispatch_action(OpenSandboxPrereqs.boxed_clone(), cx);
            })
            .into_any_element()
    }
}

pub struct SandboxPrereqsModal {
    focus_handle: FocusHandle,
    scroll_handle: ScrollHandle,
}

impl SandboxPrereqsModal {
    pub fn toggle(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
        workspace.toggle_modal(window, cx, |_window, cx| {
            cx.observe_global::<SandboxPrereqs>(|_, cx| cx.notify())
                .detach();
            // Re-render when the persisted backend choice changes so the
            // selected card updates as soon as the write lands.
            cx.observe_global::<SettingsStore>(|_, cx| cx.notify())
                .detach();
            SandboxPrereqsModal {
                focus_handle: cx.focus_handle(),
                scroll_handle: ScrollHandle::new(),
            }
        });
    }

    fn cancel(&mut self, _: &menu::Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }

    /// Persist the user's backend choice. The write is async; the modal's
    /// `SettingsStore` observer repaints once it lands.
    fn select_backend(backend: PreferredBackend, cx: &mut App) {
        let fs = <dyn Fs>::global(cx);
        let value = match backend {
            PreferredBackend::Native => PaddleboardPreferredBackendContent::Native,
            PreferredBackend::Podman => PaddleboardPreferredBackendContent::Podman,
        };
        update_settings_file(fs, cx, move |settings, _| {
            settings
                .paddleboard_sandbox
                .get_or_insert_default()
                .preferred_backend = Some(value);
        });
    }

    /// Render a single install/setup step: numbered description, plus the
    /// command (if any) with Copy and — on macOS/Linux for runnable shell
    /// commands — a "Run in a new Terminal window" button.
    fn render_step(
        index: usize,
        key: &str,
        step: InstallStep,
        bg_color: gpui::Hsla,
        host_os: Os,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        v_flex()
            .w_full()
            .gap_1()
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_start()
                    .child(
                        Label::new(format!("{}.", index + 1))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Label::new(step.description).size(LabelSize::Small)),
                    ),
            )
            .when_some(step.command, |this, command| {
                let command_for_copy = command.clone();
                let command_for_run = command.clone();
                let is_runnable = step.command_kind == CommandKind::Shell;
                let can_open_terminal = is_runnable && matches!(host_os, Os::MacOs | Os::Linux);
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
                                    Label::new(SharedString::from(command)).size(LabelSize::Small),
                                ),
                        )
                        .child(
                            IconButton::new(
                                SharedString::from(format!("sandbox-copy-{key}-{index}")),
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
                                    SharedString::from(format!("sandbox-run-{key}-{index}")),
                                    IconName::Terminal,
                                )
                                .icon_size(IconSize::Small)
                                .tooltip(Tooltip::text("Run in a new Terminal window"))
                                .on_click(cx.listener(move |_, _, _window, cx| {
                                    if let Err(err) = open_in_terminal(&command_for_run) {
                                        log::warn!(
                                            "Failed to open Terminal for sandbox step: {err}"
                                        );
                                    }
                                    SandboxPrereqs::refresh(cx);
                                })),
                            )
                        }),
                )
            })
            .into_any_element()
    }

    fn render_backend_card(
        option: BackendOption,
        selected: bool,
        bg_color: gpui::Hsla,
        host_os: Os,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let backend = option.backend;
        let key = match backend {
            PreferredBackend::Native => "native",
            PreferredBackend::Podman => "podman",
        };
        let (badge_color, badge_text) = availability_badge(&option.availability);
        let is_unsupported = matches!(option.availability, BackendAvailability::Unsupported { .. });
        let is_ready = matches!(option.availability, BackendAvailability::Ready);

        let border_color = if selected {
            cx.theme().colors().border_selected
        } else {
            cx.theme().colors().border
        };

        // Header: selection indicator + title + concrete runtime + availability.
        let header = h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(
                Icon::new(if selected {
                    IconName::Check
                } else {
                    IconName::Circle
                })
                .size(IconSize::Small)
                .color(if selected { Color::Accent } else { Color::Muted }),
            )
            .child(Headline::new(option.title).size(HeadlineSize::XSmall))
            .child(Label::new(option.runtime_label).color(Color::Muted))
            .child(div().flex_1())
            .child(
                Label::new(badge_text)
                    .size(LabelSize::Small)
                    .color(badge_color),
            );

        let mut card = v_flex()
            .id(SharedString::from(format!("sandbox-backend-{key}")))
            .w_full()
            .p_3()
            .gap_2()
            .rounded_md()
            .border_1()
            .border_color(border_color)
            .child(header)
            .child(Label::new(option.summary).size(LabelSize::Small).color(Color::Muted));

        if let Some(note) = option.coverage_note {
            card = card.child(
                Callout::new()
                    .severity(Severity::Info)
                    .icon(IconName::Info)
                    .title(note),
            );
        }

        if let BackendAvailability::Unsupported { reason } = &option.availability {
            card = card.child(
                Callout::new()
                    .severity(Severity::Warning)
                    .icon(IconName::Warning)
                    .title(reason.clone()),
            );
        }

        // "Use this backend" affordance — hidden when the backend can't run here.
        if !is_unsupported {
            card = card.child(
                Button::new(
                    SharedString::from(format!("sandbox-use-{key}")),
                    if selected { "Selected" } else { "Use this backend" },
                )
                .style(ButtonStyle::Filled)
                .label_size(LabelSize::Small)
                .disabled(selected)
                .on_click(cx.listener(move |_, _, _window, cx| {
                    Self::select_backend(backend, cx);
                    cx.notify();
                })),
            );
        }

        // Setup steps: only when there's something to do. A Ready backend needs
        // none; an Unsupported one shows its reason above instead.
        if !is_ready && !is_unsupported {
            let setup = option.setup;
            card = card.child(ui::Divider::horizontal());
            card = card.child(
                v_flex()
                    .gap_2()
                    .child(Label::new(setup.title).size(LabelSize::Small))
                    .children(setup.steps.into_iter().enumerate().map(|(i, step)| {
                        Self::render_step(i, key, step, bg_color, host_os, cx)
                    }))
                    .when_some(setup.doc_url, |this, url| {
                        this.child(
                            Label::new(format!("More: {url}"))
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        )
                    }),
            );
        }

        card.into_any_element()
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
        let preferred = SandboxSettings::get_global(cx).preferred_backend;
        let tier = current_tier(cx);
        let bg_color = cx.theme().colors().editor_background;

        // Active-tier banner: what will actually run right now, mirroring the
        // shield so the two never disagree — severity is derived from the same
        // shield color.
        let (banner_color, banner_text) = shield_appearance(tier);
        let banner_severity = match banner_color {
            Color::Success => Severity::Success,
            Color::Warning => Severity::Warning,
            Color::Error => Severity::Error,
            _ => Severity::Info,
        };
        let banner = Callout::new()
            .severity(banner_severity)
            .icon(IconName::Box)
            .title(banner_text);

        // Build the picker cards. Until the first probe lands we show a
        // placeholder rather than guessing availability.
        let cards: Vec<gpui::AnyElement> = match status.as_ref() {
            Some(status) => backend_options(status, os)
                .into_iter()
                .map(|option| {
                    let selected = option.backend == preferred;
                    Self::render_backend_card(option, selected, bg_color, os, cx)
                })
                .collect(),
            None => vec![
                h_flex()
                    .gap_1()
                    .child(
                        Icon::new(IconName::ArrowCircle)
                            .size(IconSize::Small)
                            .color(Color::Muted)
                            .with_rotate_animation(2),
                    )
                    .child(
                        Label::new("Probing this machine for available backends…")
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .into_any_element(),
            ],
        };

        let refresh_button = Button::new(
            "sandbox-prereqs-refresh",
            if refreshing { "Checking…" } else { "Refresh" },
        )
        .style(ButtonStyle::Outlined)
        .label_size(LabelSize::Small)
        .disabled(refreshing)
        .on_click(|_, _window, cx| SandboxPrereqs::refresh(cx));

        v_flex()
            .id("sandbox-prereqs-modal")
            .key_context("SandboxPrereqsModal")
            .w(rems(paddleboard_ui::modal_width::MEDIUM))
            .max_h(rems(36.))
            .elevation_3(cx)
            .track_focus(&self.focus_handle(cx))
            .on_action(cx.listener(Self::cancel))
            .on_any_mouse_down(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                this.focus_handle.focus(window, cx);
            }))
            .child(
                Modal::new("sandbox-prereqs", Some(self.scroll_handle.clone()))
                    .header(
                        ModalHeader::new()
                            .headline("Sandbox Backend")
                            .description(
                                "Choose how PaddleBoard sandboxes the tools that run untrusted \
                                 code. Your choice is honored exactly — a machine set to Native \
                                 uses the native tier even when Podman is installed, and vice \
                                 versa.",
                            )
                            .show_dismiss_button(true),
                    )
                    .section(Section::new().padded(false).child(banner))
                    .section(Section::new().child(v_flex().gap_3().children(cards)))
                    .footer(ModalFooter::new().end_slot(refresh_button)),
            )
    }
}

fn availability_badge(availability: &BackendAvailability) -> (Color, SharedString) {
    match availability {
        BackendAvailability::Ready => (Color::Success, "Ready".into()),
        BackendAvailability::NeedsSetup => (Color::Warning, "Needs setup".into()),
        BackendAvailability::Unsupported { .. } => (Color::Muted, "Unavailable here".into()),
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
         echo '=== PaddleBoard sandbox setup ==='\n\
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
