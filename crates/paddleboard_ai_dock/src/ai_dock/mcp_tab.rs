use std::sync::Arc;

use collections::HashSet;
use fs::Fs;
use gpui::{AnyElement, ClickEvent};
use project::project_settings::ProjectSettings;
use settings::{
    ContextServerCommand, ContextServerSettingsContent, Settings as _, update_settings_file,
};
use ui::prelude::*;

use crate::ai_dock::AiDock;
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
                Label::new("Loading MCP servers…")
                    .color(Color::Muted)
                    .size(LabelSize::Small),
            )
            .into_any_element(),
    };

    v_flex()
        .size_full()
        .child(catalog_section)
        .child(installed_view)
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
        .p_3()
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
