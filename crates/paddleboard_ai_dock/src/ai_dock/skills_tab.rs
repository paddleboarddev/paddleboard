use std::path::PathBuf;

use gpui::ClickEvent;
use ui::prelude::*;

use crate::ai_dock::AiDock;
use crate::ai_dock::add_skill_modal::AddSkillModal;
use crate::catalog::{SkillEntry, bundled_skill_content};

pub(super) fn render(modal: &AiDock, cx: &mut Context<AiDock>) -> impl IntoElement {
    let catalog = modal.catalog.clone();

    // Detect installed skills by scanning the two well-known directories.
    // Skills are markdown files named `<id>.md` under `.claude/commands/`
    // (project-scoped) or `~/.claude/commands/` (user-scoped).
    let project_dir = project_skills_dir(modal, cx);
    let user_dir = user_skills_dir();
    let scope_of = |id: &str| -> Option<SkillScope> {
        if let Some(dir) = project_dir.as_ref() {
            if dir.join(format!("{id}.md")).exists() {
                return Some(SkillScope::Project);
            }
        }
        if let Some(dir) = user_dir.as_ref() {
            if dir.join(format!("{id}.md")).exists() {
                return Some(SkillScope::User);
            }
        }
        None
    };

    v_flex()
        .id("ai-dock-skills-list")
        .size_full()
        .p_4()
        .gap_2()
        .overflow_y_scroll()
        .child(render_tab_header(modal, cx))
        .children(catalog.skills.iter().map(|entry| {
            render_skill_row(
                entry,
                scope_of(&entry.id),
                project_dir.is_some(),
                user_dir.is_some(),
                cx,
            )
        }))
}

fn render_tab_header(modal: &AiDock, _cx: &mut Context<AiDock>) -> impl IntoElement {
    let workspace = modal.workspace.clone();
    h_flex()
        .w_full()
        .justify_between()
        .pb_1()
        .child(
            Label::new("Available Skills")
                .size(LabelSize::Small)
                .color(Color::Muted),
        )
        .child(
            Button::new("create-skill-btn", "Create Skill")
                .style(ButtonStyle::Filled)
                .label_size(LabelSize::Small)
                // PaddleBoard: plain on_click (NOT cx.listener) — the AI Dock is a
                // modal, so toggle_modal dismisses it (re-entering AiDock.update);
                // leasing AiDock via cx.listener here would double-lease and panic.
                .on_click(move |_: &ClickEvent, window, cx| {
                    if let Some(workspace) = workspace.upgrade() {
                        let weak = workspace.read(cx).weak_handle();
                        workspace.update(cx, |workspace, cx| {
                            workspace.toggle_modal(window, cx, |window, cx| {
                                AddSkillModal::new(weak, window, cx)
                            });
                        });
                    }
                }),
        )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SkillScope {
    Project,
    User,
}

impl SkillScope {
    pub(crate) fn label(self) -> &'static str {
        match self {
            SkillScope::Project => "Project",
            SkillScope::User => "User",
        }
    }

    fn resolve_dir(self, modal: &AiDock, cx: &App) -> Option<PathBuf> {
        match self {
            SkillScope::Project => project_skills_dir(modal, cx),
            SkillScope::User => user_skills_dir(),
        }
    }
}

fn project_skills_dir(modal: &AiDock, cx: &App) -> Option<PathBuf> {
    // Prefer the active workspace's first visible worktree — that's the
    // "project" the user thinks they're in. Fall back to the process CWD
    // when there's no workspace (e.g. an empty PaddleBoard window) so the
    // detection still picks up `.claude/commands/` in the launch dir.
    if let Some(workspace) = modal.workspace.upgrade() {
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

fn user_skills_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    Some(home.join(".claude").join("commands"))
}

fn render_skill_row(
    entry: &SkillEntry,
    scope: Option<SkillScope>,
    has_project_dir: bool,
    has_user_dir: bool,
    cx: &mut Context<AiDock>,
) -> AnyElement {
    let icon = Icon::new(IconName::Sparkle)
        .size(IconSize::Small)
        .color(Color::Muted);

    let bundled = bundled_skill_content(&entry.id).is_some();

    let action_area: AnyElement = if entry.builtin {
        Button::new(
            SharedString::from(format!("ai-dock-skill-builtin-{}", entry.id)),
            "Built-in",
        )
        .style(ButtonStyle::Subtle)
        .label_size(LabelSize::Small)
        .disabled(true)
        .into_any_element()
    } else if let Some(scope) = scope {
        Button::new(
            SharedString::from(format!("ai-dock-skill-installed-{}", entry.id)),
            format!("Installed ({})", scope.label()),
        )
        .style(ButtonStyle::Outlined)
        .label_size(LabelSize::Small)
        .disabled(true)
        .into_any_element()
    } else if bundled {
        let entry_id = entry.id.clone();
        let project_btn = {
            let id = entry_id.clone();
            Button::new(
                SharedString::from(format!("ai-dock-skill-add-project-{}", entry.id)),
                "Add to project",
            )
            .style(ButtonStyle::Outlined)
            .label_size(LabelSize::Small)
            .disabled(!has_project_dir)
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                install_skill(this, &id, SkillScope::Project, window, cx);
            }))
        };
        let user_btn = {
            let id = entry_id;
            Button::new(
                SharedString::from(format!("ai-dock-skill-add-user-{}", entry.id)),
                "Add to user",
            )
            .style(ButtonStyle::Outlined)
            .label_size(LabelSize::Small)
            .disabled(!has_user_dir)
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                install_skill(this, &id, SkillScope::User, window, cx);
            }))
        };
        h_flex()
            .gap_1()
            .child(project_btn)
            .child(user_btn)
            .into_any_element()
    } else {
        match entry.homepage.clone() {
            Some(url) => Button::new(
                SharedString::from(format!("ai-dock-skill-info-{}", entry.id)),
                "Learn More",
            )
            .style(ButtonStyle::Outlined)
            .label_size(LabelSize::Small)
            .on_click(move |_: &ClickEvent, _window, cx| {
                cx.open_url(&url);
            })
            .into_any_element(),
            None => Button::new(
                SharedString::from(format!("ai-dock-skill-na-{}", entry.id)),
                "Not installed",
            )
            .style(ButtonStyle::Outlined)
            .label_size(LabelSize::Small)
            .disabled(true)
            .into_any_element(),
        }
    };

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
                .child(Label::new(SharedString::from(entry.name.clone())))
                .child(
                    Label::new(SharedString::from(entry.description.clone()))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                ),
        )
        .child(action_area)
        .into_any_element()
}

fn install_skill(
    modal: &mut AiDock,
    id: &str,
    scope: SkillScope,
    _window: &mut Window,
    cx: &mut Context<AiDock>,
) {
    let Some(content) = bundled_skill_content(id) else {
        log::error!("paddleboard_ai_dock: install_skill called for unbundled id `{id}`");
        return;
    };
    let Some(dir) = scope.resolve_dir(modal, cx) else {
        report_install_error(
            modal,
            format!("Could not resolve the {} skills directory.", scope.label()),
            cx,
        );
        return;
    };

    if let Err(err) = write_skill_file(&dir, id, content) {
        log::error!("paddleboard_ai_dock: failed to install skill `{id}`: {err}");
        report_install_error(
            modal,
            format!("Failed to install /{id}: {err}"),
            cx,
        );
        return;
    }

    cx.notify();
}

fn write_skill_file(dir: &PathBuf, id: &str, content: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(format!("{id}.md")), content)
}

fn report_install_error(modal: &AiDock, message: String, cx: &mut Context<AiDock>) {
    if let Some(workspace) = modal.workspace.upgrade() {
        workspace.update(cx, |workspace, cx| {
            workspace.show_error(anyhow::anyhow!(message), cx);
        });
    } else {
        log::error!("paddleboard_ai_dock: install error (no workspace to notify): {message}");
    }
}
