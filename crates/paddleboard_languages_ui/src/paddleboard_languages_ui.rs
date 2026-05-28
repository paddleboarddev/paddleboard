use std::collections::HashMap;

use gpui::{
    Action as _, App, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Render,
    ScrollHandle, Window,
};
use language::{LanguageName, LanguageServerName, language_settings::all_language_settings};
use project::Project;
use settings::update_settings_file;
use ui::{Chip, Modal, ModalHeader, Tooltip, prelude::*};
use workspace::{ModalView, Workspace};

/// How a language's support is installed.
enum Install {
    /// A built-in language server: flip the `language_servers` setting on and
    /// download the binary. `adapter` is the primary server used for both the
    /// pre-download and the installed-state check; `enabled_servers` is the
    /// exact list written to user settings (mirrors the upstream default minus
    /// the PaddleBoard `!`-disable).
    Builtin {
        adapter: &'static str,
        enabled_servers: &'static [&'static str],
    },
    /// An extension-provided server (no built-in adapter). The button opens the
    /// Extensions page focused on this extension id.
    Extension { extension_id: &'static str },
}

/// A language that isn't enabled by default. `key` matches the language's name
/// in `assets/settings/default.json`.
struct InstallLanguage {
    key: &'static str,
    display: &'static str,
    prereq: &'static str,
    install: Install,
}

const INSTALL_TIER: &[InstallLanguage] = &[
    InstallLanguage {
        key: "Java",
        display: "Java",
        prereq: "Requires JDK 17+",
        install: Install::Builtin {
            adapter: "jdtls",
            enabled_servers: &["jdtls", "..."],
        },
    },
    InstallLanguage {
        key: "Kotlin",
        display: "Kotlin",
        prereq: "Requires JDK 17+",
        install: Install::Builtin {
            adapter: "kotlin-language-server",
            enabled_servers: &["kotlin-language-server", "..."],
        },
    },
    InstallLanguage {
        key: "PHP",
        display: "PHP",
        prereq: "Requires Node",
        install: Install::Builtin {
            adapter: "intelephense",
            enabled_servers: &["intelephense", "..."],
        },
    },
    InstallLanguage {
        key: "CSharp",
        display: "C#",
        prereq: "Requires .NET",
        install: Install::Builtin {
            adapter: "roslyn",
            enabled_servers: &["roslyn", "!omnisharp", "..."],
        },
    },
    InstallLanguage {
        key: "C++",
        display: "C++",
        prereq: "Downloads clangd",
        install: Install::Builtin {
            adapter: "clangd",
            enabled_servers: &["clangd", "..."],
        },
    },
    InstallLanguage {
        key: "Ruby",
        display: "Ruby",
        prereq: "Provided by an extension",
        install: Install::Extension {
            extension_id: "ruby",
        },
    },
    InstallLanguage {
        key: "Dart",
        display: "Dart",
        prereq: "Provided by an extension",
        install: Install::Extension {
            extension_id: "dart",
        },
    },
];

const DEFAULT_TIER: &[&str] = &[
    "Rust",
    "TypeScript",
    "JavaScript",
    "Python",
    "Go",
    "JSON",
    "YAML",
    "HTML/CSS",
];

#[derive(Clone)]
enum InstallState {
    Available,
    Installing,
    Installed,
    Failed(String),
}

pub struct ManageLanguagesModal {
    project: Entity<Project>,
    states: HashMap<&'static str, InstallState>,
    scroll_handle: ScrollHandle,
    focus_handle: FocusHandle,
}

impl ManageLanguagesModal {
    pub fn toggle(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
        let project = workspace.project().clone();
        workspace.toggle_modal(window, cx, |_window, cx| Self::new(project, cx));
    }

    fn new(project: Entity<Project>, cx: &mut Context<Self>) -> Self {
        let mut states = HashMap::default();
        let all_settings = all_language_settings(None, cx);
        for language in INSTALL_TIER {
            let Install::Builtin { adapter, .. } = &language.install else {
                continue;
            };
            let language_name = LanguageName::new(language.key);
            let enabled = all_settings
                .language(None, Some(&language_name), cx)
                .language_servers
                .iter()
                .any(|server| server == adapter);
            states.insert(
                language.key,
                if enabled {
                    InstallState::Installed
                } else {
                    InstallState::Available
                },
            );
        }

        Self {
            project,
            states,
            scroll_handle: ScrollHandle::new(),
            focus_handle: cx.focus_handle(),
        }
    }

    fn install(&mut self, language: &'static InstallLanguage, cx: &mut Context<Self>) {
        let Install::Builtin {
            adapter,
            enabled_servers,
        } = &language.install
        else {
            return;
        };
        let adapter = *adapter;
        let key = language.key;

        self.states.insert(key, InstallState::Installing);
        cx.notify();

        let fs = self.project.read(cx).fs().clone();
        let servers: Vec<String> = enabled_servers.iter().map(|s| s.to_string()).collect();
        update_settings_file(fs, cx, move |settings, _| {
            settings
                .project
                .all_languages
                .languages
                .0
                .entry(key.to_string())
                .or_default()
                .language_servers = Some(servers);
        });

        let registry = self.project.read(cx).languages().clone();
        let Some(cached_adapter) =
            registry.adapter_for_name(&LanguageServerName::new_static(adapter))
        else {
            // No registered adapter to pre-download; the setting is written and the
            // server (if any) will attach on first file open.
            self.states.insert(key, InstallState::Installed);
            cx.notify();
            return;
        };

        let worktree_id = self
            .project
            .read(cx)
            .visible_worktrees(cx)
            .next()
            .map(|worktree| worktree.read(cx).id());
        let Some(worktree_id) = worktree_id else {
            // No folder open to host the download. The server is enabled and will be
            // fetched automatically the first time a matching file is opened.
            self.states.insert(key, InstallState::Installed);
            cx.notify();
            return;
        };

        let install = self.project.read(cx).lsp_store().update(cx, |lsp_store, cx| {
            lsp_store.install_language_server(worktree_id, cached_adapter, cx)
        });
        cx.spawn(async move |this, cx| {
            let result = install.await;
            this.update(cx, |this, cx| {
                let state = match result {
                    Ok(()) => InstallState::Installed,
                    Err(error) => InstallState::Failed(error.to_string()),
                };
                this.states.insert(key, state);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn open_extension(
        &mut self,
        extension_id: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.emit(DismissEvent);
        let action = paddleboard_actions::Extensions {
            category_filter: None,
            id: Some(extension_id.to_string()),
        };
        window.dispatch_action(action.boxed_clone(), cx);
    }

    fn cancel(&mut self, _: &menu::Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }

    fn render_ready_section(&self) -> impl IntoElement {
        v_flex()
            .gap_1()
            .child(
                Label::new("Ready to use")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_1()
                    .children(DEFAULT_TIER.iter().map(|name| Chip::new(*name))),
            )
    }

    fn render_install_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut section = v_flex().gap_1().child(
            Label::new("Install support")
                .size(LabelSize::Small)
                .color(Color::Muted),
        );
        for language in INSTALL_TIER {
            section = section.child(self.render_install_row(language, cx));
        }
        section
    }

    fn render_install_row(
        &self,
        language: &'static InstallLanguage,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let action = if let Install::Extension { extension_id } = &language.install {
            let extension_id = *extension_id;
            Button::new(
                SharedString::from(format!("ext-{}", language.key)),
                "View in Extensions",
            )
            .on_click(
                cx.listener(move |this, _, window, cx| this.open_extension(extension_id, window, cx)),
            )
            .into_any_element()
        } else {
            let state = self
                .states
                .get(language.key)
                .cloned()
                .unwrap_or(InstallState::Available);
            match &state {
                InstallState::Available => Button::new(
                    SharedString::from(format!("install-{}", language.key)),
                    "Install",
                )
                .style(ButtonStyle::Filled)
                .on_click(cx.listener(move |this, _, _, cx| this.install(language, cx)))
                .into_any_element(),
                InstallState::Installing => h_flex()
                    .gap_1()
                    .child(
                        Icon::new(IconName::ArrowCircle)
                            .size(IconSize::Small)
                            .color(Color::Muted),
                    )
                    .child(
                        Label::new("Installing…")
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .into_any_element(),
                InstallState::Installed => h_flex()
                    .gap_1()
                    .child(
                        Icon::new(IconName::Check)
                            .size(IconSize::Small)
                            .color(Color::Success),
                    )
                    .child(
                        Label::new("Installed")
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .into_any_element(),
                InstallState::Failed(error) => Button::new(
                    SharedString::from(format!("retry-{}", language.key)),
                    "Retry",
                )
                .style(ButtonStyle::Filled)
                .tooltip(Tooltip::text(error.clone()))
                .on_click(cx.listener(move |this, _, _, cx| this.install(language, cx)))
                .into_any_element(),
            }
        };

        h_flex()
            .justify_between()
            .gap_2()
            .px_2()
            .py_1p5()
            .child(
                v_flex()
                    .child(Label::new(language.display))
                    .child(
                        Label::new(language.prereq)
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    ),
            )
            .child(action)
    }
}

impl EventEmitter<DismissEvent> for ManageLanguagesModal {}

impl Focusable for ManageLanguagesModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ModalView for ManageLanguagesModal {}

impl Render for ManageLanguagesModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("manage-languages-modal")
            .key_context("ManageLanguagesModal")
            .w(rems(34.))
            .elevation_3(cx)
            .on_action(cx.listener(Self::cancel))
            .capture_any_mouse_down(cx.listener(|this, _, window, cx| {
                this.focus_handle(cx).focus(window, cx);
            }))
            .child(
                Modal::new("manage-languages", None)
                    .header(
                        ModalHeader::new().headline("Languages").description(
                            "Core languages work out of the box. Install support for \
                             languages that need an external toolchain.",
                        ),
                    )
                    .child(
                        v_flex()
                            .id("manage-languages-body")
                            .px_3()
                            .pb_2()
                            .gap_3()
                            .max_h(vh(0.7, window))
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll_handle)
                            .child(self.render_ready_section())
                            .child(self.render_install_section(cx)),
                    ),
            )
    }
}

pub fn init(cx: &mut App) {
    cx.observe_new(
        |workspace: &mut Workspace, _window: Option<&mut Window>, _cx: &mut Context<Workspace>| {
            workspace.register_action(
                |workspace, _: &paddleboard_actions::languages::ManageLanguages, window, cx| {
                    ManageLanguagesModal::toggle(workspace, window, cx);
                },
            );
        },
    )
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT_SETTINGS: &str = include_str!("../../../assets/settings/default.json");

    #[test]
    fn install_tier_entries_are_well_formed() {
        let mut keys = std::collections::HashSet::new();
        for language in INSTALL_TIER {
            assert!(!language.key.is_empty(), "empty key");
            assert!(!language.display.is_empty(), "empty display for {}", language.key);
            assert!(!language.prereq.is_empty(), "empty prereq for {}", language.key);
            assert!(keys.insert(language.key), "duplicate key {}", language.key);

            match &language.install {
                Install::Builtin {
                    adapter,
                    enabled_servers,
                } => {
                    assert!(!adapter.is_empty(), "empty adapter for {}", language.key);
                    assert!(
                        enabled_servers.contains(adapter),
                        "{} enable list must include its adapter {adapter}",
                        language.key
                    );
                }
                Install::Extension { extension_id } => {
                    assert!(
                        !extension_id.is_empty(),
                        "empty extension id for {}",
                        language.key
                    );
                }
            }
        }
    }

    #[test]
    fn builtin_adapters_are_disabled_by_default() {
        // Each built-in opt-in language's adapter must be `!`-disabled in the
        // bundled defaults, otherwise it would auto-download and the "Install
        // support" flow would be meaningless. Extension-provided languages
        // (Ruby, Dart) are gated by extension availability, not by this list.
        for language in INSTALL_TIER {
            if let Install::Builtin { adapter, .. } = &language.install {
                let disabled = format!("\"!{adapter}\"");
                assert!(
                    DEFAULT_SETTINGS.contains(&disabled),
                    "expected default settings to disable {} via {disabled}",
                    language.display
                );
            }
        }
    }
}
