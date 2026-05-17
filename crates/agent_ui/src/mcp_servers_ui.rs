use std::ops::Range;
use std::sync::Arc;

use agent::ContextServerRegistry;
use context_server::ContextServerId;
use editor::{Editor, EditorElement, EditorStyle};
use extension_host::ExtensionStore;
use fs::Fs;
use gpui::{
    Action, AnyElement, App, Context, Corner, Entity, EventEmitter, Focusable, KeyContext,
    ParentElement, Render, RenderOnce, SharedString, Styled, Task, TextStyle,
    UniformListScrollHandle, WeakEntity, Window, point, uniform_list,
};
use language::LanguageRegistry;
use project::context_server_store::{
    ContextServerConfiguration, ContextServerStatus, ContextServerStore,
};
use settings::{Settings as _, SettingsStore, update_settings_file};
use theme_settings::ThemeSettings;
use ui::{
    ButtonStyle, Chip, ContextMenu, DividerColor, PopoverMenu, ScrollableHandle, Switch,
    ToggleButtonGroup, ToggleButtonGroupSize, ToggleButtonGroupStyle, ToggleButtonSimple, Tooltip,
    WithScrollbar, prelude::*,
};
use util::ResultExt as _;
use workspace::{
    Workspace,
    item::{Item, ItemEvent},
};

use crate::agent_configuration::{
    ConfigureContextServerModal, ConfigureContextServerToolsModal,
    extension_only_provides_context_server, resolve_extension_for_context_server,
    show_unable_to_uninstall_extension_with_context_server,
};
use crate::agent_panel::AgentPanel;
use paddleboard_actions::ExtensionCategoryFilter;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum McpFilter {
    All,
    Running,
    Stopped,
    Error,
}

#[derive(IntoElement)]
struct McpServerCard {
    children: Vec<AnyElement>,
}

impl McpServerCard {
    fn new() -> Self {
        Self {
            children: Vec::new(),
        }
    }
}

impl ParentElement for McpServerCard {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements)
    }
}

impl RenderOnce for McpServerCard {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        div().w_full().child(
            v_flex()
                .p_3()
                .mt_4()
                .w_full()
                .gap_2()
                .bg(cx.theme().colors().elevated_surface_background.opacity(0.5))
                .border_1()
                .border_color(cx.theme().colors().border_variant)
                .rounded_md()
                .children(self.children),
        )
    }
}

pub struct McpServersPage {
    fs: Arc<dyn Fs>,
    language_registry: Arc<LanguageRegistry>,
    workspace: WeakEntity<Workspace>,
    context_server_store: Entity<ContextServerStore>,
    context_server_registry: Entity<ContextServerRegistry>,
    list: UniformListScrollHandle,
    server_ids: Vec<ContextServerId>,
    filtered_indices: Vec<usize>,
    query_editor: Entity<Editor>,
    filter: McpFilter,
    _subscriptions: Vec<gpui::Subscription>,
}

impl McpServersPage {
    pub fn new(
        workspace: &Workspace,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Entity<Self> {
        let fs = <dyn Fs>::global(cx);
        let language_registry = workspace.app_state().languages.clone();
        let weak_workspace = workspace.weak_handle();
        let project = workspace.project().clone();
        let context_server_store = project.read(cx).context_server_store();

        // Reuse the AgentPanel's registry if available so tool counts stay in sync.
        let context_server_registry = workspace
            .panel::<AgentPanel>(cx)
            .map(|panel| panel.read(cx).context_server_registry().clone())
            .unwrap_or_else(|| {
                cx.new(|cx| ContextServerRegistry::new(context_server_store.clone(), cx))
            });

        cx.new(|cx| {
            let query_editor = cx.new(|cx| {
                let mut input = Editor::single_line(window, cx);
                input.set_placeholder_text("Search MCP servers...", window, cx);
                input
            });
            cx.subscribe(&query_editor, Self::on_query_change).detach();

            let mut subscriptions = Vec::new();
            subscriptions.push(cx.subscribe(&context_server_store, |this, _, _, cx| {
                this.reload_servers(cx);
            }));
            subscriptions.push(cx.subscribe(&context_server_registry, |_, _, _, cx| {
                cx.notify();
            }));
            subscriptions.push(cx.observe_global::<SettingsStore>(|this, cx| {
                this.filter_servers(cx);
            }));

            let mut this = Self {
                fs,
                language_registry,
                workspace: weak_workspace,
                context_server_store,
                context_server_registry,
                list: UniformListScrollHandle::new(),
                server_ids: Vec::new(),
                filtered_indices: Vec::new(),
                query_editor,
                filter: McpFilter::All,
                _subscriptions: subscriptions,
            };

            this.reload_servers(cx);
            this
        })
    }

    fn reload_servers(&mut self, cx: &mut Context<Self>) {
        let store = self.context_server_store.read(cx);
        let mut ids = store.server_ids().to_vec();
        ids.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        self.server_ids = ids;
        self.filter_servers(cx);
    }

    fn search_query(&self, cx: &App) -> Option<String> {
        let search = self.query_editor.read(cx).text(cx);
        if search.trim().is_empty() {
            None
        } else {
            Some(search)
        }
    }

    fn status_for(&self, id: &ContextServerId, cx: &App) -> ContextServerStatus {
        self.context_server_store
            .read(cx)
            .status_for_server(id)
            .unwrap_or(ContextServerStatus::Stopped)
    }

    fn filter_servers(&mut self, cx: &mut Context<Self>) {
        let search = self.search_query(cx).map(|s| s.to_lowercase());
        let filter = self.filter;

        let indices = self
            .server_ids
            .iter()
            .enumerate()
            .filter(|(_, id)| {
                let matches_search = search
                    .as_ref()
                    .is_none_or(|query| id.0.to_lowercase().contains(query));

                let status = self.status_for(id, cx);
                let matches_filter = match filter {
                    McpFilter::All => true,
                    McpFilter::Running => matches!(status, ContextServerStatus::Running),
                    McpFilter::Stopped => matches!(status, ContextServerStatus::Stopped),
                    McpFilter::Error => matches!(status, ContextServerStatus::Error(_)),
                };

                matches_search && matches_filter
            })
            .map(|(index, _)| index)
            .collect();

        self.filtered_indices = indices;
        cx.notify();
    }

    fn scroll_to_top(&mut self, cx: &mut Context<Self>) {
        self.list.set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    fn on_query_change(
        &mut self,
        _: Entity<Editor>,
        event: &editor::EditorEvent,
        cx: &mut Context<Self>,
    ) {
        if let editor::EditorEvent::Edited { .. } = event {
            self.filter_servers(cx);
            self.scroll_to_top(cx);
        }
    }

    fn render_text_input(
        &self,
        editor: &Entity<Editor>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let settings = ThemeSettings::get_global(cx);
        let text_style = TextStyle {
            color: if editor.read(cx).read_only(cx) {
                cx.theme().colors().text_disabled
            } else {
                cx.theme().colors().text
            },
            font_family: settings.ui_font.family.clone(),
            font_features: settings.ui_font.features.clone(),
            font_fallbacks: settings.ui_font.fallbacks.clone(),
            font_size: rems(0.875).into(),
            font_weight: settings.ui_font.weight,
            line_height: relative(1.3),
            ..Default::default()
        };

        EditorElement::new(
            editor,
            EditorStyle {
                background: cx.theme().colors().editor_background,
                local_player: cx.theme().players().local(),
                text: text_style,
                ..Default::default()
            },
        )
    }

    fn render_search(&self, cx: &mut Context<Self>) -> Div {
        let mut key_context = KeyContext::new_with_defaults();
        key_context.add("BufferSearchBar");

        h_flex()
            .key_context(key_context)
            .h_8()
            .min_w(rems_from_px(384.))
            .flex_1()
            .pl_1p5()
            .pr_2()
            .gap_2()
            .border_1()
            .border_color(cx.theme().colors().border)
            .rounded_md()
            .child(Icon::new(IconName::MagnifyingGlass).color(Color::Muted))
            .child(self.render_text_input(&self.query_editor, cx))
    }

    fn render_add_server_popover(&self) -> impl IntoElement {
        PopoverMenu::new("mcp-add-server-popover")
            .trigger(
                Button::new("mcp-add-server", "Add Server")
                    .style(ButtonStyle::Outlined)
                    .start_icon(
                        Icon::new(IconName::Plus)
                            .size(IconSize::Small)
                            .color(Color::Muted),
                    )
                    .label_size(LabelSize::Small),
            )
            .menu(move |window, cx| {
                Some(ContextMenu::build(window, cx, |menu, _window, _cx| {
                    menu.entry("Add Custom Server", None, |window, cx| {
                        window.dispatch_action(crate::AddContextServer.boxed_clone(), cx)
                    })
                    .entry("Install from Extensions", None, |window, cx| {
                        window.dispatch_action(
                            paddleboard_actions::Extensions {
                                category_filter: Some(ExtensionCategoryFilter::ContextServers),
                                id: None,
                            }
                            .boxed_clone(),
                            cx,
                        )
                    })
                }))
            })
            .anchor(Corner::TopRight)
            .offset(gpui::Point {
                x: px(0.0),
                y: px(2.0),
            })
    }

    fn render_empty_state(&self, cx: &Context<Self>) -> impl IntoElement {
        let has_search = self.search_query(cx).is_some();
        let message = match self.filter {
            McpFilter::All => {
                if has_search {
                    "No MCP servers match your search."
                } else {
                    "No MCP servers installed yet. Add one to get started."
                }
            }
            McpFilter::Running => "No MCP servers are currently running.",
            McpFilter::Stopped => "No MCP servers are stopped.",
            McpFilter::Error => "No MCP servers are reporting errors.",
        };

        h_flex().py_4().gap_1p5().child(Label::new(message))
    }

    fn render_servers(
        &mut self,
        range: Range<usize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<McpServerCard> {
        range
            .map(|index| {
                let Some(server_index) = self.filtered_indices.get(index).copied() else {
                    return self.render_missing_card();
                };
                let Some(id) = self.server_ids.get(server_index).cloned() else {
                    return self.render_missing_card();
                };
                self.render_server_card(id, cx)
            })
            .collect()
    }

    fn render_missing_card(&self) -> McpServerCard {
        McpServerCard::new().child(
            Label::new("Missing server entry.")
                .size(LabelSize::Small)
                .color(Color::Muted),
        )
    }

    fn render_server_card(
        &self,
        context_server_id: ContextServerId,
        cx: &Context<Self>,
    ) -> McpServerCard {
        let status = self.status_for(&context_server_id, cx);
        let configuration = self
            .context_server_store
            .read(cx)
            .configuration_for_server(&context_server_id);
        let is_running = matches!(status, ContextServerStatus::Running);
        let id_string = SharedString::from(context_server_id.0.clone());

        let provided_by_extension = configuration.as_ref().is_none_or(|config| {
            matches!(config.as_ref(), ContextServerConfiguration::Extension { .. })
        });

        let display_name = if provided_by_extension {
            resolve_extension_for_context_server(&context_server_id, cx)
                .map(|(_, manifest)| {
                    let name = manifest.name.as_str();
                    let stripped = name
                        .strip_suffix(" MCP Server")
                        .or_else(|| name.strip_suffix(" MCP"))
                        .or_else(|| name.strip_suffix(" Context Server"))
                        .unwrap_or(name);
                    SharedString::from(stripped.to_string())
                })
                .unwrap_or_else(|| id_string.clone())
        } else {
            id_string
        };

        let source_label = match configuration.as_ref().map(|c| c.as_ref()) {
            Some(ContextServerConfiguration::Extension { .. }) => "Extension",
            Some(ContextServerConfiguration::Sandboxed { .. }) => "Sandboxed",
            Some(ContextServerConfiguration::Http { .. }) => "Remote",
            Some(ContextServerConfiguration::Custom { remote: true, .. }) => "Remote",
            Some(ContextServerConfiguration::Custom { .. }) => "Custom",
            None => "Extension",
        };

        let detail_text: Option<SharedString> = match configuration.as_ref().map(|c| c.as_ref()) {
            Some(ContextServerConfiguration::Http { url, .. }) => Some(url.to_string().into()),
            Some(config) => config.command().map(|cmd| {
                let path = cmd.path.to_string_lossy();
                if cmd.args.is_empty() {
                    SharedString::from(path.to_string())
                } else {
                    SharedString::from(format!("{} {}", path, cmd.args.join(" ")))
                }
            }),
            None => None,
        };

        let tool_count = self
            .context_server_registry
            .read(cx)
            .tools_for_server(&context_server_id)
            .count();

        let (status_label, status_color) = match &status {
            ContextServerStatus::Running => ("Running", Color::Success),
            ContextServerStatus::Starting => ("Starting", Color::Info),
            ContextServerStatus::Stopped => ("Stopped", Color::Muted),
            ContextServerStatus::Error(_) => ("Error", Color::Error),
            ContextServerStatus::AuthRequired => ("Auth Required", Color::Warning),
            ContextServerStatus::Authenticating => ("Authenticating", Color::Info),
        };

        let error_message = if let ContextServerStatus::Error(error) = &status {
            Some(error.clone())
        } else {
            None
        };

        let is_remote = configuration
            .as_ref()
            .map(|c| matches!(c.as_ref(), ContextServerConfiguration::Http { .. }))
            .unwrap_or(false);
        let should_show_logout_button = configuration.as_ref().is_some_and(|config| {
            matches!(config.as_ref(), ContextServerConfiguration::Http { .. })
                && !config.has_static_auth_header()
        });
        let auth_required = matches!(status, ContextServerStatus::AuthRequired);

        let config_menu = self.render_config_menu(
            context_server_id.clone(),
            tool_count,
            is_remote,
            should_show_logout_button,
            provided_by_extension,
        );

        let switch = Switch::new(
            SharedString::from(format!("mcp-switch-{}", context_server_id.0)),
            is_running.into(),
        )
        .on_click({
            let context_server_store = self.context_server_store.clone();
            let fs = self.fs.clone();
            let context_server_id = context_server_id.clone();
            move |state, _window, cx| {
                let is_enabled = match state {
                    ToggleState::Unselected | ToggleState::Indeterminate => {
                        context_server_store.update(cx, |this, cx| {
                            this.stop_server(&context_server_id, cx).log_err();
                        });
                        false
                    }
                    ToggleState::Selected => {
                        context_server_store.update(cx, |this, cx| {
                            if let Some(server) = this.get_server(&context_server_id) {
                                this.start_server(server, cx);
                            }
                        });
                        true
                    }
                };
                update_settings_file(fs.clone(), cx, {
                    let context_server_id = context_server_id.clone();
                    move |settings, _| {
                        settings
                            .project
                            .context_servers
                            .entry(context_server_id.0)
                            .or_insert_with(|| {
                                settings::ContextServerSettingsContent::Extension {
                                    enabled: is_enabled,
                                    remote: false,
                                    settings: serde_json::json!({}),
                                }
                            })
                            .set_enabled(is_enabled);
                    }
                });
            }
        });

        let header = h_flex()
            .justify_between()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .min_w_0()
                    .flex_1()
                    .child(
                        Icon::new(IconName::Server)
                            .size(IconSize::Medium)
                            .color(Color::Muted),
                    )
                    .child(Headline::new(display_name).size(HeadlineSize::Small))
                    .child(Chip::new(source_label).label_size(LabelSize::XSmall)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                Icon::new(IconName::Circle)
                                    .size(IconSize::XSmall)
                                    .color(status_color),
                            )
                            .child(
                                Label::new(status_label)
                                    .size(LabelSize::Small)
                                    .color(status_color),
                            ),
                    )
                    .child(switch)
                    .child(config_menu),
            );

        let tool_label = if is_running && tool_count > 0 {
            Some(if tool_count == 1 {
                SharedString::from("1 tool")
            } else {
                SharedString::from(format!("{tool_count} tools"))
            })
        } else {
            None
        };

        let mut card = McpServerCard::new().child(header);

        card = card.child(
            h_flex()
                .gap_2()
                .justify_between()
                .child(
                    Label::new(SharedString::from(format!("ID: {}", context_server_id.0)))
                        .size(LabelSize::Small)
                        .color(Color::Muted)
                        .truncate(),
                )
                .when_some(tool_label, |this, label| {
                    this.child(Label::new(label).size(LabelSize::Small).color(Color::Muted))
                }),
        );

        if let Some(detail) = detail_text {
            card = card.child(
                Label::new(detail)
                    .size(LabelSize::Small)
                    .color(Color::Muted)
                    .truncate(),
            );
        }

        if let Some(error) = error_message {
            card = card.child(
                h_flex()
                    .gap_1p5()
                    .child(
                        Icon::new(IconName::XCircle)
                            .size(IconSize::XSmall)
                            .color(Color::Error),
                    )
                    .child(Label::new(error).color(Color::Muted).size(LabelSize::Small)),
            );
        } else if auth_required {
            let context_server_store = self.context_server_store.clone();
            let id_for_auth = context_server_id.clone();
            card = card.child(
                h_flex()
                    .justify_between()
                    .gap_2()
                    .child(
                        Label::new("Authenticate to connect this server")
                            .color(Color::Muted)
                            .size(LabelSize::Small),
                    )
                    .child(
                        Button::new(
                            SharedString::from(format!("auth-{}", context_server_id.0)),
                            "Authenticate",
                        )
                        .style(ButtonStyle::Outlined)
                        .label_size(LabelSize::Small)
                        .on_click(move |_event, _window, cx| {
                            context_server_store.update(cx, |store, cx| {
                                store.authenticate_server(&id_for_auth, cx).log_err();
                            });
                        }),
                    ),
            );
        }

        card
    }

    fn render_config_menu(
        &self,
        context_server_id: ContextServerId,
        tool_count: usize,
        is_remote: bool,
        should_show_logout_button: bool,
        provided_by_extension: bool,
    ) -> impl IntoElement {
        let trigger_id = SharedString::from(format!("mcp-config-menu-{}", context_server_id.0));
        let fs = self.fs.clone();
        let language_registry = self.language_registry.clone();
        let workspace = self.workspace.clone();
        let context_server_registry = self.context_server_registry.clone();
        let context_server_store = self.context_server_store.clone();

        PopoverMenu::new(trigger_id)
            .trigger_with_tooltip(
                IconButton::new("mcp-config-menu-trigger", IconName::Settings)
                    .icon_color(Color::Muted)
                    .icon_size(IconSize::Small),
                Tooltip::text("Configure MCP Server"),
            )
            .anchor(Corner::TopRight)
            .menu(move |window, cx| {
                let language_registry = language_registry.clone();
                let workspace = workspace.clone();
                let context_server_registry = context_server_registry.clone();
                let context_server_store = context_server_store.clone();
                let fs = fs.clone();
                let context_server_id = context_server_id.clone();

                Some(ContextMenu::build(window, cx, move |menu, _window, _cx| {
                    menu.entry("Configure Server", None, {
                        let context_server_id = context_server_id.clone();
                        let language_registry = language_registry.clone();
                        let workspace = workspace.clone();
                        move |window, cx| {
                            ConfigureContextServerModal::show_modal_for_existing_server(
                                context_server_id.clone(),
                                language_registry.clone(),
                                workspace.clone(),
                                window,
                                cx,
                            )
                            .detach();
                            // is_remote is captured to keep the menu shape consistent
                            // with the Agent Configuration view; the same modal handles
                            // both stdio and http servers.
                            let _ = is_remote;
                        }
                    })
                    .when(tool_count > 0, |this| {
                        this.entry("View Tools", None, {
                            let context_server_id = context_server_id.clone();
                            let context_server_registry = context_server_registry.clone();
                            let workspace = workspace.clone();
                            move |window, cx| {
                                let context_server_id = context_server_id.clone();
                                workspace
                                    .update(cx, |workspace, cx| {
                                        ConfigureContextServerToolsModal::toggle(
                                            context_server_id,
                                            context_server_registry.clone(),
                                            workspace,
                                            window,
                                            cx,
                                        );
                                    })
                                    .ok();
                            }
                        })
                    })
                    .when(should_show_logout_button, |this| {
                        this.entry("Log Out", None, {
                            let context_server_store = context_server_store.clone();
                            let context_server_id = context_server_id.clone();
                            move |_window, cx| {
                                context_server_store.update(cx, |store, cx| {
                                    store.logout_server(&context_server_id, cx).log_err();
                                });
                            }
                        })
                    })
                    .separator()
                    .entry("Uninstall", None, {
                        let fs = fs.clone();
                        let context_server_id = context_server_id.clone();
                        let workspace = workspace.clone();
                        move |_, cx| {
                            let uninstall_extension_task = match (
                                provided_by_extension,
                                resolve_extension_for_context_server(&context_server_id, cx),
                            ) {
                                (true, Some((id, manifest))) => {
                                    if extension_only_provides_context_server(manifest.as_ref()) {
                                        ExtensionStore::global(cx).update(cx, |store, cx| {
                                            store.uninstall_extension(id, cx)
                                        })
                                    } else {
                                        workspace
                                            .update(cx, |workspace, cx| {
                                                show_unable_to_uninstall_extension_with_context_server(
                                                    workspace,
                                                    context_server_id.clone(),
                                                    cx,
                                                );
                                            })
                                            .log_err();
                                        Task::ready(Ok(()))
                                    }
                                }
                                _ => Task::ready(Ok(())),
                            };

                            cx.spawn({
                                let fs = fs.clone();
                                let context_server_id = context_server_id.clone();
                                async move |cx| {
                                    uninstall_extension_task.await?;
                                    cx.update(|cx| {
                                        update_settings_file(fs.clone(), cx, {
                                            let context_server_id = context_server_id.clone();
                                            move |settings, _| {
                                                settings
                                                    .project
                                                    .context_servers
                                                    .remove(&context_server_id.0);
                                            }
                                        })
                                    });
                                    anyhow::Ok(())
                                }
                            })
                            .detach_and_log_err(cx);
                        }
                    })
                }))
            })
    }
}

impl Render for McpServersPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let total = self.server_ids.len();
        let running = self
            .server_ids
            .iter()
            .filter(|id| matches!(self.status_for(id, cx), ContextServerStatus::Running))
            .count();

        v_flex()
            .size_full()
            .bg(cx.theme().colors().editor_background)
            .child(
                v_flex()
                    .p_4()
                    .gap_4()
                    .border_b_1()
                    .border_color(cx.theme().colors().border_variant)
                    .child(
                        h_flex()
                            .w_full()
                            .gap_1p5()
                            .justify_between()
                            .child(
                                h_flex()
                                    .gap_3()
                                    .child(Headline::new("MCP Servers").size(HeadlineSize::Large))
                                    .child(
                                        Label::new(format!("{running}/{total} running"))
                                            .size(LabelSize::Small)
                                            .color(Color::Muted),
                                    ),
                            )
                            .child(self.render_add_server_popover()),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .flex_wrap()
                            .gap_2()
                            .child(self.render_search(cx))
                            .child(div().child(
                                ToggleButtonGroup::single_row(
                                    "mcp-filter-buttons",
                                    [
                                        ToggleButtonSimple::new(
                                            "All",
                                            cx.listener(|this, _event, _, cx| {
                                                this.filter = McpFilter::All;
                                                this.filter_servers(cx);
                                                this.scroll_to_top(cx);
                                            }),
                                        ),
                                        ToggleButtonSimple::new(
                                            "Running",
                                            cx.listener(|this, _event, _, cx| {
                                                this.filter = McpFilter::Running;
                                                this.filter_servers(cx);
                                                this.scroll_to_top(cx);
                                            }),
                                        ),
                                        ToggleButtonSimple::new(
                                            "Stopped",
                                            cx.listener(|this, _event, _, cx| {
                                                this.filter = McpFilter::Stopped;
                                                this.filter_servers(cx);
                                                this.scroll_to_top(cx);
                                            }),
                                        ),
                                        ToggleButtonSimple::new(
                                            "Error",
                                            cx.listener(|this, _event, _, cx| {
                                                this.filter = McpFilter::Error;
                                                this.filter_servers(cx);
                                                this.scroll_to_top(cx);
                                            }),
                                        ),
                                    ],
                                )
                                .style(ToggleButtonGroupStyle::Outlined)
                                .size(ToggleButtonGroupSize::Custom(rems_from_px(30.)))
                                .label_size(LabelSize::Default)
                                .auto_width()
                                .selected_index(match self.filter {
                                    McpFilter::All => 0,
                                    McpFilter::Running => 1,
                                    McpFilter::Stopped => 2,
                                    McpFilter::Error => 3,
                                })
                                .into_any_element(),
                            )),
                    ),
            )
            .child(v_flex().px_4().size_full().overflow_y_hidden().map(|this| {
                let count = self.filtered_indices.len();
                let _ = DividerColor::BorderFaded;
                if count == 0 {
                    this.child(self.render_empty_state(cx)).into_any_element()
                } else {
                    let scroll_handle = &self.list;
                    this.child(
                        uniform_list("mcp-server-entries", count, cx.processor(Self::render_servers))
                            .flex_grow()
                            .pb_4()
                            .track_scroll(scroll_handle),
                    )
                    .vertical_scrollbar_for(scroll_handle, window, cx)
                    .into_any_element()
                }
            }))
    }
}

impl EventEmitter<ItemEvent> for McpServersPage {}

impl Focusable for McpServersPage {
    fn focus_handle(&self, cx: &App) -> gpui::FocusHandle {
        self.query_editor.read(cx).focus_handle(cx)
    }
}

impl Item for McpServersPage {
    type Event = ItemEvent;

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> SharedString {
        "MCP Servers".into()
    }

    fn tab_icon(&self, _window: &Window, _cx: &App) -> Option<Icon> {
        Some(Icon::new(IconName::Server))
    }

    fn telemetry_event_text(&self) -> Option<&'static str> {
        Some("MCP Servers Page Opened")
    }

    fn show_toolbar(&self) -> bool {
        false
    }

    fn to_item_events(event: &Self::Event, f: &mut dyn FnMut(workspace::item::ItemEvent)) {
        f(*event)
    }
}
