use std::path::PathBuf;

use gpui::ClickEvent;
use ui::prelude::*;

use crate::catalog::SkillEntry;
use crate::ai_dock::AiDock;

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
        .children(
            catalog
                .skills
                .iter()
                .map(|entry| render_skill_row(entry, scope_of(&entry.id), cx)),
        )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SkillScope {
    Project,
    User,
}

impl SkillScope {
    fn label(self) -> &'static str {
        match self {
            SkillScope::Project => "Project",
            SkillScope::User => "User",
        }
    }
}

fn project_skills_dir(_modal: &AiDock, _cx: &App) -> Option<PathBuf> {
    // For v1, use the current working directory's `.claude/commands/`.
    // A future polish pass can read this from the active workspace's
    // first worktree instead, since modals don't track which workspace
    // they belong to as cleanly as panel items do.
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
    cx: &mut Context<AiDock>,
) -> AnyElement {
    let icon = Icon::new(IconName::Sparkle)
        .size(IconSize::Small)
        .color(Color::Muted);

    let action_button: AnyElement = if let Some(scope) = scope {
        Button::new(
            SharedString::from(format!("ai-dock-skill-installed-{}", entry.id)),
            format!("Installed ({})", scope.label()),
        )
        .style(ButtonStyle::Outlined)
        .label_size(LabelSize::Small)
        .disabled(true)
        .into_any_element()
    } else {
        // No bundled content yet — direct to homepage if present, otherwise
        // surface a disabled button so users see the skill exists.
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
        .child(action_button)
        .into_any_element()
}
