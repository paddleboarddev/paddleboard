// PaddleBoard: persona picker for the native agent. Shows the thread's active
// persona in the message-editor toolbar, lets the user switch or clear it, and
// auto-adopts a project-root `PERSONA.md` for fresh threads. Discovery runs on
// the background executor; the menu renders from the cached list.

use agent::{Thread, ThreadPersona};
use gpui::{Action as _, App, Context, Entity, Task, WeakEntity, Window, prelude::*};
use paddleboard_personas::{Persona, PersonaSource, build_overlay, discover};
use paddleboard_personas_settings::PersonasSettings;
use project::Project;
use settings::Settings as _;
use std::path::PathBuf;
use ui::{ContextMenu, PopoverMenu, Tooltip, prelude::*};

pub struct PersonaPicker {
    /// `Some` for native-agent threads (full picker); `None` for external
    /// agents, where the picker renders as a hint that personas exist but
    /// only apply to the PaddleBoard Agent.
    thread: Option<Entity<Thread>>,
    project: WeakEntity<Project>,
    personas: Vec<Persona>,
    _refresh: Task<()>,
}

impl PersonaPicker {
    pub fn new(
        thread: Option<Entity<Thread>>,
        project: WeakEntity<Project>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            thread,
            project,
            personas: Vec::new(),
            _refresh: Task::ready(()),
        };
        this.refresh(true, cx);
        this
    }

    fn project_root(&self, cx: &App) -> Option<PathBuf> {
        let project = self.project.upgrade()?;
        let worktree = project.read(cx).visible_worktrees(cx).next()?;
        Some(worktree.read(cx).abs_path().to_path_buf())
    }

    /// On external-agent threads, personas don't apply (those agents own
    /// their system prompts). When the project actually has personas, show a
    /// muted hint that routes to the AI Dock's Personas tab instead of
    /// silently rendering nothing — first-time users otherwise conclude the
    /// feature doesn't exist.
    fn render_external_agent_hint(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        if !PersonasSettings::get_global(cx).enabled || self.personas.is_empty() {
            return gpui::Empty.into_any_element();
        }
        IconButton::new("persona-hint", IconName::Person)
            .icon_size(IconSize::Small)
            .icon_color(Color::Muted)
            .tooltip(Tooltip::text(
                "Personas apply to the PaddleBoard Agent only. Click to browse them in the AI Dock.",
            ))
            .on_click(|_, window, cx| {
                window.dispatch_action(
                    paddleboard_actions::ai_dock::OpenPersonas.boxed_clone(),
                    cx,
                );
            })
            .into_any_element()
    }

    /// Re-discover personas. When `adopt_default` is set and the thread is
    /// fresh (no messages, no persona), a root `PERSONA.md` becomes the
    /// thread's persona — the zero-config path.
    fn refresh(&mut self, adopt_default: bool, cx: &mut Context<Self>) {
        let root = self.project_root(cx);
        self._refresh = cx.spawn(async move |this, cx| {
            let discovered = cx
                .background_spawn(async move { discover(root.as_deref()) })
                .await;
            this.update(cx, |this, cx| {
                if adopt_default
                    && PersonasSettings::get_global(cx).enabled
                    && let Some(thread) = this.thread.clone()
                    && let Some(root_persona) = discovered
                        .iter()
                        .find(|persona| persona.source == PersonaSource::ProjectRoot)
                        .cloned()
                {
                    thread.update(cx, |thread, cx| {
                        if thread.persona().is_none() && thread.is_empty() {
                            thread.set_persona(
                                Some(ThreadPersona {
                                    name: root_persona.name.clone().into(),
                                    overlay: build_overlay(&root_persona, &discovered),
                                }),
                                cx,
                            );
                        }
                    });
                }
                this.personas = discovered;
                cx.notify();
            })
            .ok();
        });
    }
}

impl Render for PersonaPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(thread) = self.thread.clone() else {
            return self.render_external_agent_hint(cx);
        };
        let active_persona = thread.read(cx).persona().cloned();

        if !PersonasSettings::get_global(cx).enabled
            || (self.personas.is_empty() && active_persona.is_none())
        {
            return gpui::Empty.into_any_element();
        }

        let label: SharedString = active_persona
            .as_ref()
            .map(|persona| persona.name.clone())
            .unwrap_or_else(|| "Persona".into());

        let trigger_button = Button::new("persona-picker", label)
            .label_size(LabelSize::Small)
            .color(if active_persona.is_some() {
                Color::Accent
            } else {
                Color::Muted
            })
            .start_icon(
                Icon::new(IconName::Person)
                    .size(IconSize::XSmall)
                    .color(Color::Muted),
            );

        let picker = cx.entity();
        PopoverMenu::new("persona-picker-menu")
            .trigger_with_tooltip(
                trigger_button,
                Tooltip::text("Choose who the agent should be for this thread"),
            )
            .anchor(gpui::Anchor::BottomRight)
            .menu({
                move |window, cx| {
                    picker.update(cx, |picker, cx| picker.refresh(false, cx));
                    let picker = picker.clone();
                    Some(ContextMenu::build(window, cx, |mut menu, _window, cx| {
                        let (Some(thread), personas) = ({
                            let picker = picker.read(cx);
                            (picker.thread.clone(), picker.personas.clone())
                        }) else {
                            return menu;
                        };
                        let active = thread.read(cx).persona().map(|p| p.name.clone());
                        menu = menu.toggleable_entry("No Persona", active.is_none(), ui::IconPosition::End, None, {
                            let thread = thread.clone();
                            move |_window, cx| {
                                thread.update(cx, |thread, cx| thread.set_persona(None, cx));
                            }
                        });
                        for persona in personas.clone() {
                            let is_active = active.as_deref() == Some(persona.name.as_str());
                            let label = format!("{} — {}", persona.name, persona.source.label());
                            menu = menu.toggleable_entry(label, is_active, ui::IconPosition::End, None, {
                                let thread = thread.clone();
                                let personas = personas.clone();
                                move |_window, cx| {
                                    thread.update(cx, |thread, cx| {
                                        thread.set_persona(
                                            Some(ThreadPersona {
                                                name: persona.name.clone().into(),
                                                overlay: build_overlay(&persona, &personas),
                                            }),
                                            cx,
                                        );
                                    });
                                }
                            });
                        }
                        menu
                    }))
                }
            })
            .into_any_element()
    }
}
