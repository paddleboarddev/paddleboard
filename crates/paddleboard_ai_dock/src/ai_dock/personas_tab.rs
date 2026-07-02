use std::path::PathBuf;

use gpui::ClickEvent;
use ui::prelude::*;

use crate::ai_dock::AiDock;
use crate::catalog::{PersonaEntry, bundled_persona_content};

pub(super) fn render(modal: &AiDock, cx: &mut Context<AiDock>) -> impl IntoElement {
    let catalog = modal.catalog.clone();

    // Discovered personas: the project root PERSONA.md plus the project and
    // user `.claude/personas/` libraries. Files are tiny, and the modal only
    // re-renders on interaction, so synchronous discovery matches how the
    // Skills tab probes for installed files.
    let project_root = project_root(modal, cx);
    let discovered = paddleboard_personas::discover(project_root.as_deref());

    let project_dir = project_root
        .as_ref()
        .map(|root| root.join(".claude").join("personas"));
    let user_dir = user_personas_dir();
    let installed_scope = |id: &str| -> Option<PersonaScope> {
        let file_name = format!("{id}.persona.md");
        if let Some(dir) = project_dir.as_ref()
            && dir.join(&file_name).exists()
        {
            return Some(PersonaScope::Project);
        }
        if let Some(dir) = user_dir.as_ref()
            && dir.join(&file_name).exists()
        {
            return Some(PersonaScope::User);
        }
        None
    };

    v_flex()
        .id("ai-dock-personas-list")
        .size_full()
        .p_4()
        .gap_2()
        .overflow_y_scroll()
        .child(
            Label::new("Personas describe who the agent should be — pick one per thread in the agent panel, or drop a PERSONA.md at your project root to set a default.")
                .size(LabelSize::Small)
                .color(Color::Muted),
        )
        .when(!discovered.is_empty(), |this| {
            this.child(
                Label::new("Discovered")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .children(
                discovered
                    .iter()
                    .map(|persona| render_discovered_row(persona, cx)),
            )
        })
        .child(
            Label::new("Starter Personas")
                .size(LabelSize::Small)
                .color(Color::Muted),
        )
        .children(catalog.personas.iter().map(|entry| {
            render_starter_row(
                entry,
                installed_scope(&entry.id),
                project_dir.is_some(),
                user_dir.is_some(),
                cx,
            )
        }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PersonaScope {
    Project,
    User,
}

impl PersonaScope {
    fn label(self) -> &'static str {
        match self {
            PersonaScope::Project => "Project",
            PersonaScope::User => "User",
        }
    }

    fn resolve_dir(
        self,
        project_dir: Option<&PathBuf>,
        user_dir: Option<&PathBuf>,
    ) -> Option<PathBuf> {
        match self {
            PersonaScope::Project => project_dir.cloned(),
            PersonaScope::User => user_dir.cloned(),
        }
    }
}

fn project_root(modal: &AiDock, cx: &App) -> Option<PathBuf> {
    if let Some(workspace) = modal.workspace.upgrade() {
        let workspace = workspace.read(cx);
        let project = workspace.project().read(cx);
        if let Some(worktree) = project.visible_worktrees(cx).next() {
            return Some(worktree.read(cx).abs_path().to_path_buf());
        }
    }
    std::env::current_dir().ok()
}

fn user_personas_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    Some(home.join(".claude").join("personas"))
}

fn render_discovered_row(
    persona: &paddleboard_personas::Persona,
    cx: &mut Context<AiDock>,
) -> AnyElement {
    h_flex()
        .w_full()
        .p_3()
        .gap_3()
        .items_start()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().colors().border_variant)
        .bg(cx.theme().colors().elevated_surface_background.opacity(0.5))
        .child(
            div().pt_0p5().child(
                Icon::new(IconName::Person)
                    .size(IconSize::Small)
                    .color(Color::Accent),
            ),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(Label::new(SharedString::from(persona.name.clone())))
                .child(
                    Label::new(SharedString::from(persona.description.clone()))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                ),
        )
        .child(
            Button::new(
                SharedString::from(format!("ai-dock-persona-source-{}", persona.name)),
                persona.source.label(),
            )
            .style(ButtonStyle::Subtle)
            .label_size(LabelSize::Small)
            .disabled(true),
        )
        .into_any_element()
}

fn render_starter_row(
    entry: &PersonaEntry,
    scope: Option<PersonaScope>,
    has_project_dir: bool,
    has_user_dir: bool,
    cx: &mut Context<AiDock>,
) -> AnyElement {
    let action_area: AnyElement = if let Some(scope) = scope {
        Button::new(
            SharedString::from(format!("ai-dock-persona-installed-{}", entry.id)),
            format!("Installed ({})", scope.label()),
        )
        .style(ButtonStyle::Outlined)
        .label_size(LabelSize::Small)
        .disabled(true)
        .into_any_element()
    } else {
        let project_btn = {
            let id = entry.id.clone();
            Button::new(
                SharedString::from(format!("ai-dock-persona-add-project-{}", entry.id)),
                "Add to project",
            )
            .style(ButtonStyle::Outlined)
            .label_size(LabelSize::Small)
            .disabled(!has_project_dir)
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                install_persona(this, &id, PersonaScope::Project, window, cx);
            }))
        };
        let user_btn = {
            let id = entry.id.clone();
            Button::new(
                SharedString::from(format!("ai-dock-persona-add-user-{}", entry.id)),
                "Add to user",
            )
            .style(ButtonStyle::Outlined)
            .label_size(LabelSize::Small)
            .disabled(!has_user_dir)
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                install_persona(this, &id, PersonaScope::User, window, cx);
            }))
        };
        h_flex()
            .gap_1()
            .child(project_btn)
            .child(user_btn)
            .into_any_element()
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
        .child(
            div().pt_0p5().child(
                Icon::new(IconName::Person)
                    .size(IconSize::Small)
                    .color(Color::Muted),
            ),
        )
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

fn install_persona(
    modal: &mut AiDock,
    id: &str,
    scope: PersonaScope,
    _window: &mut Window,
    cx: &mut Context<AiDock>,
) {
    let Some(content) = bundled_persona_content(id) else {
        log::error!("paddleboard_ai_dock: install_persona called for unbundled id `{id}`");
        return;
    };
    let project_dir = project_root(modal, cx).map(|root| root.join(".claude").join("personas"));
    let user_dir = user_personas_dir();
    let Some(dir) = scope.resolve_dir(project_dir.as_ref(), user_dir.as_ref()) else {
        report_install_error(
            modal,
            format!("Could not resolve the {} personas directory.", scope.label()),
            cx,
        );
        return;
    };

    if let Err(err) = write_persona_file(&dir, id, content) {
        log::error!("paddleboard_ai_dock: failed to install persona `{id}`: {err}");
        report_install_error(modal, format!("Failed to install {id}: {err}"), cx);
        return;
    }

    cx.notify();
}

fn write_persona_file(dir: &PathBuf, id: &str, content: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(format!("{id}.persona.md")), content)
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
