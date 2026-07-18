use std::sync::Arc;

use collections::HashSet;
use fs::Fs;
use gpui::{AnyElement, ClickEvent};
use project::project_settings::ProjectSettings;
use settings::{
    ContextServerCommand, ContextServerSettingsContent, Settings as _, update_settings_file,
};
use ui::CommonAnimationExt as _;
use ui::prelude::*;

use crate::ai_dock::AiDock;
use crate::ai_dock::build_mcp_modal::BuildMcpModal;
use crate::catalog::McpEntry;

pub(super) fn render(
    modal: &mut AiDock,
    window: &mut Window,
    cx: &mut Context<AiDock>,
) -> AnyElement {
    // Make sure the absorbed McpServersView exists — we host it below the
    // catalog section, so it has to be ready before we lay things out.
    modal.ensure_mcp_view(window, cx);

    let catalog = modal.catalog.clone();
    let installed_ids: HashSet<String> = ProjectSettings::get_global(cx)
        .context_servers
        .keys()
        .map(|k| k.to_string())
        .collect();

    // PaddleBoard: "Build an MCP" — generate a server for a service that has none.
    let header = render_tab_header(modal, cx);
    let catalog_section = render_catalog_section(&catalog.mcp_servers, &installed_ids, cx);
    let installed_view: AnyElement = match modal.mcp_view.as_ref() {
        Some(view) => div()
            .id("ai-dock-mcp-installed")
            .flex_1()
            .min_h_0()
            .child(view.clone())
            .into_any_element(),
        None => v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .child(
                h_flex()
                    .gap_1p5()
                    .child(
                        Icon::new(IconName::ArrowCircle)
                            .size(IconSize::Small)
                            .color(Color::Muted)
                            .with_rotate_animation(2),
                    )
                    .child(
                        Label::new("Loading MCP servers…")
                            .color(Color::Muted)
                            .size(LabelSize::Small),
                    ),
            )
            .into_any_element(),
    };

    v_flex()
        .size_full()
        .child(header)
        .child(catalog_section)
        .child(installed_view)
        .into_any_element()
}

// PaddleBoard: header row with the "Build an MCP" action, mirroring the Skills
// tab's "Create Skill" button.
fn render_tab_header(modal: &AiDock, cx: &mut Context<AiDock>) -> AnyElement {
    let workspace = modal.workspace.clone();
    h_flex()
        .w_full()
        .justify_between()
        .p_4()
        .border_b_1()
        .border_color(cx.theme().colors().border_variant)
        .child(
            Label::new("Generate a server for any service")
                .size(LabelSize::Small)
                .color(Color::Muted),
        )
        .child(
            Button::new("build-mcp-btn", "Build an MCP")
                .style(ButtonStyle::Filled)
                .label_size(LabelSize::Small)
                // PaddleBoard: plain on_click (NOT cx.listener) so the AiDock entity
                // isn't leased during the click. The AI Dock is itself a modal, and
                // toggle_modal dismisses it (AiDock::on_before_dismiss re-enters
                // AiDock.update) — leasing AiDock here would double-lease and panic.
                .on_click(move |_: &ClickEvent, window, cx| {
                    if let Some(workspace) = workspace.upgrade() {
                        let weak = workspace.read(cx).weak_handle();
                        workspace.update(cx, |workspace, cx| {
                            workspace.toggle_modal(window, cx, |window, cx| {
                                BuildMcpModal::new(weak, window, cx)
                            });
                        });
                    }
                }),
        )
        .into_any_element()
}

fn render_catalog_section(
    entries: &[McpEntry],
    installed_ids: &HashSet<String>,
    cx: &mut Context<AiDock>,
) -> AnyElement {
    if entries.is_empty() {
        return div().into_any_element();
    }

    v_flex()
        .p_4()
        .gap_1p5()
        .border_b_1()
        .border_color(cx.theme().colors().border_variant)
        .child(
            Label::new("Available")
                .size(LabelSize::Small)
                .color(Color::Muted),
        )
        .child(
            v_flex().gap_1().children(
                entries
                    .iter()
                    .map(|entry| render_catalog_row(entry, installed_ids.contains(&entry.id), cx)),
            ),
        )
        .into_any_element()
}

fn render_catalog_row(
    entry: &McpEntry,
    is_installed: bool,
    cx: &mut Context<AiDock>,
) -> AnyElement {
    let action: AnyElement = if is_installed {
        Button::new(
            SharedString::from(format!("ai-dock-mcp-installed-{}", entry.id)),
            "Installed",
        )
        .style(ButtonStyle::Outlined)
        .label_size(LabelSize::Small)
        .disabled(true)
        .into_any_element()
    } else {
        let entry_for_click = entry.clone();
        Button::new(
            SharedString::from(format!("ai-dock-mcp-install-{}", entry.id)),
            "Install",
        )
        .style(ButtonStyle::Filled)
        .label_size(LabelSize::Small)
        .on_click(cx.listener(move |_, _: &ClickEvent, _window, cx| {
            install_mcp_server(&entry_for_click, cx);
        }))
        .into_any_element()
    };

    h_flex()
        .w_full()
        .py_1()
        .px_2()
        .gap_2p5()
        .items_center()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().colors().border_variant)
        .bg(cx.theme().colors().elevated_surface_background.opacity(0.5))
        .child(
            Icon::new(IconName::Server)
                .size(IconSize::Small)
                .color(Color::Muted),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(Label::new(SharedString::from(entry.name.clone())).size(LabelSize::Small))
                .child(
                    Label::new(SharedString::from(entry.description.clone()))
                        .size(LabelSize::XSmall)
                        .color(Color::Muted)
                        .truncate(),
                ),
        )
        .child(action)
        .into_any_element()
}

fn install_mcp_server(entry: &McpEntry, cx: &mut Context<AiDock>) {
    let id: Arc<str> = entry.id.as_str().into();
    let command_path = entry.command.clone();
    let args = entry.args.clone();
    let fs = <dyn Fs>::global(cx);

    update_settings_file(fs.clone(), cx, move |settings, _| {
        settings
            .project
            .context_servers
            .entry(id)
            .or_insert_with(|| ContextServerSettingsContent::Stdio {
                enabled: true,
                remote: false,
                command: ContextServerCommand {
                    path: command_path.into(),
                    args,
                    env: None,
                    timeout: None,
                },
            });
    });

    // Settings writes propagate asynchronously to `context_server_store`,
    // which the absorbed McpServersView observes. Notifying here re-reads
    // the installed-ids set so the catalog row flips to "Installed" right
    // away without waiting for the next external state change.
    cx.notify();
}
