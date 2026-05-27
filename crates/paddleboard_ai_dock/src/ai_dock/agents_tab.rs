use client::Client;
use collections::HashMap;
use fs::Fs;
use gpui::ClickEvent;
use project::AgentRegistryStore;
use project::agent_server_store::AllAgentServersSettings;
use settings::{CustomAgentServerSettings, SettingsStore, update_settings_file};
use ui::prelude::*;

use crate::catalog::AgentEntry;
use crate::ai_dock::AiDock;
use crate::ai_dock::add_agent_modal::AddAgentModal;

pub(super) fn render(modal: &AiDock, cx: &mut Context<AiDock>) -> impl IntoElement {
    let catalog = modal.catalog.clone();
    let registry_agents = AgentRegistryStore::try_global(cx)
        .map(|store| store.read(cx).agents().to_vec())
        .unwrap_or_default();
    let installed_agents = cx
        .global::<SettingsStore>()
        .get::<AllAgentServersSettings>(None)
        .clone();

    v_flex()
        .id("ai-dock-agents-list")
        .size_full()
        .p_4()
        .gap_2()
        .overflow_y_scroll()
        .child(render_tab_header(modal, cx))
        .children(
            catalog
                .agents
                .iter()
                .map(|entry| render_agent_row(entry, &registry_agents, &installed_agents, cx)),
        )
}

fn render_tab_header(modal: &AiDock, cx: &mut Context<AiDock>) -> impl IntoElement {
    let workspace = modal.workspace.clone();
    h_flex()
        .w_full()
        .justify_between()
        .pb_1()
        .child(
            Label::new("Available Agents")
                .size(LabelSize::Small)
                .color(Color::Muted),
        )
        .child(
            Button::new("add-agent-btn", "Add Agent")
                .style(ButtonStyle::Filled)
                .label_size(LabelSize::Small)
                .on_click(cx.listener(move |_this, _: &ClickEvent, window, cx| {
                    if let Some(workspace) = workspace.upgrade() {
                        workspace.update(cx, |workspace, cx| {
                            workspace.toggle_modal(window, cx, |window, cx| {
                                AddAgentModal::new(window, cx)
                            });
                        });
                    }
                })),
        )
}

fn render_agent_row(
    entry: &AgentEntry,
    registry_agents: &[project::RegistryAgent],
    installed_agents: &AllAgentServersSettings,
    cx: &mut Context<AiDock>,
) -> AnyElement {
    let installed = entry.builtin_zed || installed_agents.contains_key(&entry.id);
    let registry_agent = registry_agents.iter().find(|a| a.id().as_ref() == entry.id);

    let icon = if entry.builtin_zed {
        Icon::new(IconName::ZedAgent)
    } else if let Some(reg) = registry_agent.as_ref() {
        match reg.icon_path() {
            Some(path) => Icon::from_external_svg(path.clone()),
            None => Icon::new(IconName::Sparkle),
        }
    } else {
        Icon::new(IconName::Sparkle)
    }
    .size(IconSize::Small)
    .color(Color::Muted);

    let action_button: AnyElement = if entry.builtin_zed {
        zed_agent_button(cx)
    } else if installed {
        Button::new(SharedString::from(format!("ai-dock-open-{}", entry.id)), "Configure")
            .style(ButtonStyle::Outlined)
            .label_size(LabelSize::Small)
            .into_any_element()
    } else {
        let agent_id = entry.id.clone();
        let fs = <dyn Fs>::global(cx);
        Button::new(SharedString::from(format!("ai-dock-install-{}", entry.id)), "Install")
            .style(ButtonStyle::Filled)
            .label_size(LabelSize::Small)
            .on_click(move |_: &ClickEvent, _window, cx| {
                let agent_id = agent_id.clone();
                update_settings_file(fs.clone(), cx, move |settings, _| {
                    let agent_servers = settings.agent_servers.get_or_insert_default();
                    agent_servers.entry(agent_id).or_insert_with(|| {
                        CustomAgentServerSettings::Registry {
                            env: Default::default(),
                            default_mode: None,
                            default_model: None,
                            favorite_models: Vec::new(),
                            default_config_options: HashMap::default(),
                            favorite_config_option_values: HashMap::default(),
                        }
                    });
                });
            })
            .into_any_element()
    };

    let homepage_link: Option<AnyElement> = entry.homepage.as_ref().map(|url| {
        let url = url.clone();
        IconButton::new(
            SharedString::from(format!("ai-dock-homepage-{}", entry.id)),
            IconName::ArrowUpRight,
        )
        .icon_size(IconSize::Small)
        .tooltip(ui::Tooltip::text("Open homepage"))
        .on_click(move |_: &ClickEvent, _window, cx| {
            cx.open_url(&url);
        })
        .into_any_element()
    });

    h_flex()
        .w_full()
        .p_3()
        .gap_3()
        .items_start()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().colors().border_variant)
        .bg(cx.theme().colors().elevated_surface_background.opacity(0.5))
        .child(div().pt_0p5().child(icon))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    h_flex()
                        .gap_2()
                        .child(Label::new(SharedString::from(entry.name.clone())))
                        .when(installed, |this| {
                            this.child(
                                Label::new("Installed")
                                    .size(LabelSize::XSmall)
                                    .color(Color::Success),
                            )
                        }),
                )
                .child(
                    Label::new(SharedString::from(entry.description.clone()))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                ),
        )
        .child(
            h_flex()
                .gap_1()
                .children(homepage_link)
                .child(action_button),
        )
        .into_any_element()
}

fn zed_agent_button(cx: &mut Context<AiDock>) -> AnyElement {
    let client = Client::global(cx);
    let status = *client.status().borrow();
    let is_signed_out = status.is_signed_out()
        || matches!(
            status,
            client::Status::AuthenticationError | client::Status::ConnectionError
        );

    if is_signed_out {
        Button::new("ai-dock-zed-signin", "Sign In")
            .style(ButtonStyle::Filled)
            .label_size(LabelSize::Small)
            .on_click(move |_: &ClickEvent, _window, cx| {
                let client = Client::global(cx);
                cx.spawn(async move |cx| client.sign_in_with_optional_connect(true, cx).await)
                    .detach_and_log_err(cx);
            })
            .into_any_element()
    } else {
        Button::new("ai-dock-zed-configured", "Signed In")
            .style(ButtonStyle::Outlined)
            .label_size(LabelSize::Small)
            .disabled(true)
            .into_any_element()
    }
}
