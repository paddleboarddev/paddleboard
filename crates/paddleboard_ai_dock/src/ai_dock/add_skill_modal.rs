use std::path::PathBuf;

use gpui::{DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Render, WeakEntity};
use ui::{
    Button, KeyBinding, Label, LabelSize, Modal, ModalFooter, ModalHeader, ToggleButtonGroup,
    ToggleButtonGroupSize, ToggleButtonGroupStyle, ToggleButtonSimple, prelude::*,
};
use ui_input::InputField;
use workspace::{ModalView, Workspace};

use super::skills_tab::SkillScope;

pub struct AddSkillModal {
    workspace: WeakEntity<Workspace>,
    name_input: Entity<InputField>,
    content_input: Entity<InputField>,
    scope: SkillScope,
    focus_handle: FocusHandle,
}

impl AddSkillModal {
    pub fn new(
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name_input = cx.new(|cx| {
            InputField::new(window, cx, "my-skill")
                .label("Skill Name")
                .tab_index(1)
                .tab_stop(true)
        });

        let content_input = cx.new(|cx| {
            InputField::new(
                window,
                cx,
                "Describe what this skill does when invoked...",
            )
            .label("Prompt")
            .tab_index(2)
            .tab_stop(true)
        });

        Self {
            workspace,
            name_input,
            content_input,
            scope: SkillScope::Project,
            focus_handle: cx.focus_handle(),
        }
    }

    fn confirm(&mut self, _: &menu::Confirm, _window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name_input.read(cx).text(cx);
        let name = name.trim().to_string();
        let content = self.content_input.read(cx).text(cx);
        let content = content.trim().to_string();

        if name.is_empty() || content.is_empty() {
            return;
        }

        let dir = match self.resolve_dir(cx) {
            Some(dir) => dir,
            None => {
                if let Some(workspace) = self.workspace.upgrade() {
                    workspace.update(cx, |workspace, cx| {
                        workspace.show_error(
                            &anyhow::anyhow!(
                                "Could not resolve the {} skills directory.",
                                self.scope.label()
                            ),
                            cx,
                        );
                    });
                }
                return;
            }
        };

        if let Err(err) = write_skill(dir, &name, &content) {
            log::error!("failed to create skill `{name}`: {err:#}");
            if let Some(workspace) = self.workspace.upgrade() {
                workspace.update(cx, |workspace, cx| {
                    workspace
                        .show_error(&anyhow::anyhow!("Failed to create /{name}: {err}"), cx);
                });
            }
            return;
        }

        cx.emit(DismissEvent);
    }

    fn cancel(&mut self, _: &menu::Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }

    fn resolve_dir(&self, cx: &App) -> Option<PathBuf> {
        match self.scope {
            SkillScope::Project => {
                if let Some(workspace) = self.workspace.upgrade() {
                    let workspace = workspace.read(cx);
                    let project = workspace.project().read(cx);
                    if let Some(worktree) = project.visible_worktrees(cx).next() {
                        let root = worktree.read(cx).abs_path();
                        return Some(root.join(".claude").join("commands"));
                    }
                }
                let cwd = std::env::current_dir().ok()?;
                Some(cwd.join(".claude").join("commands"))
            }
            SkillScope::User => {
                let home = std::env::var_os("HOME").map(PathBuf::from)?;
                Some(home.join(".claude").join("commands"))
            }
        }
    }

    fn render_scope_selector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected_index = match self.scope {
            SkillScope::Project => 0,
            SkillScope::User => 1,
        };

        v_flex().gap_1().child(Label::new("Scope").size(LabelSize::Small)).child(
            ToggleButtonGroup::single_row(
                "add-skill-scope",
                [
                    ToggleButtonSimple::new(
                        "Project",
                        cx.listener(|this, _, _, cx| {
                            this.scope = SkillScope::Project;
                            cx.notify();
                        }),
                    ),
                    ToggleButtonSimple::new(
                        "User",
                        cx.listener(|this, _, _, cx| {
                            this.scope = SkillScope::User;
                            cx.notify();
                        }),
                    ),
                ],
            )
            .style(ToggleButtonGroupStyle::Outlined)
            .size(ToggleButtonGroupSize::Default)
            .selected_index(selected_index),
        )
    }
}

fn write_skill(dir: PathBuf, name: &str, content: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(format!("{name}.md")), content)
}

impl EventEmitter<DismissEvent> for AddSkillModal {}

impl Focusable for AddSkillModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ModalView for AddSkillModal {}

impl Render for AddSkillModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus_handle = self.focus_handle(cx);
        let name_empty = self.name_input.read(cx).is_empty(cx);
        let content_empty = self.content_input.read(cx).is_empty(cx);

        v_flex()
            .id("add-skill-modal")
            .key_context("AddSkillModal")
            .w(rems(32.))
            .elevation_3(cx)
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::cancel))
            .capture_any_mouse_down(cx.listener(|this, _, window, cx| {
                this.focus_handle(cx).focus(window, cx);
            }))
            .child(
                Modal::new("add-skill", None)
                    .header(
                        ModalHeader::new()
                            .headline("Create Skill")
                            .description(
                                "Create a new skill as a .claude/commands/ markdown file.",
                            ),
                    )
                    .child(
                        v_flex()
                            .px_3()
                            .pb_2()
                            .gap_2()
                            .child(self.name_input.clone())
                            .child(self.content_input.clone())
                            .child(self.render_scope_selector(cx)),
                    )
                    .footer(
                        ModalFooter::new().end_slot(
                            h_flex()
                                .gap_1()
                                .child(
                                    Button::new("cancel", "Cancel")
                                        .key_binding(
                                            KeyBinding::for_action_in(
                                                &menu::Cancel,
                                                &focus_handle,
                                                cx,
                                            )
                                            .map(|kb| kb.size(rems_from_px(12.))),
                                        )
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.cancel(&menu::Cancel, window, cx);
                                        })),
                                )
                                .child(
                                    Button::new("create-skill-confirm", "Create Skill")
                                        .style(ButtonStyle::Filled)
                                        .key_binding(
                                            KeyBinding::for_action_in(
                                                &menu::Confirm,
                                                &focus_handle,
                                                cx,
                                            )
                                            .map(|kb| kb.size(rems_from_px(12.))),
                                        )
                                        .disabled(name_empty || content_empty)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.confirm(&menu::Confirm, window, cx);
                                        })),
                                ),
                        ),
                    ),
            )
    }
}
