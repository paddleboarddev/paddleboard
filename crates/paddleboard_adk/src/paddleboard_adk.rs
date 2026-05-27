mod scaffold_modal;

use std::path::PathBuf;

use gpui::{App, Context, Window};
use task::{HideStrategy, RevealStrategy, RevealTarget, SaveStrategy, Shell, SpawnInTerminal, TaskId};
use workspace::Workspace;

pub use scaffold_modal::ScaffoldAgentModal;

pub fn init(cx: &mut App) {
    cx.observe_new(
        |workspace: &mut Workspace,
         _window: Option<&mut Window>,
         _cx: &mut Context<Workspace>| {
            workspace.register_action(
                |workspace, _: &paddleboard_actions::adk::ScaffoldAgent, window, cx| {
                    handle_scaffold_agent(workspace, window, cx);
                },
            );

            workspace.register_action(
                |workspace, _: &paddleboard_actions::adk::RunAgent, window, cx| {
                    handle_run_agent(workspace, window, cx);
                },
            );
        },
    )
    .detach();
}

fn handle_scaffold_agent(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    ScaffoldAgentModal::toggle(workspace, window, cx);
}

fn handle_run_agent(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let cwd = project_root(workspace, cx);

    let _ = workspace.spawn_in_terminal(
        SpawnInTerminal {
            id: TaskId("adk-run".into()),
            full_label: "ADK Web Server".into(),
            label: "ADK Web".into(),
            command: Some("adk".into()),
            args: vec!["web".into()],
            command_label: "adk web".into(),
            cwd,
            env: Default::default(),
            use_new_terminal: true,
            allow_concurrent_runs: false,
            reveal: RevealStrategy::Always,
            reveal_target: RevealTarget::Dock,
            hide: HideStrategy::Never,
            shell: Shell::System,
            show_summary: true,
            show_command: true,
            show_rerun: true,
            save: SaveStrategy::None,
        },
        window,
        cx,
    );
}

pub(crate) fn spawn_adk_create(
    name: &str,
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let cwd = project_root(workspace, cx);

    let _ = workspace.spawn_in_terminal(
        SpawnInTerminal {
            id: TaskId("adk-scaffold".into()),
            full_label: format!("ADK Create: {name}"),
            label: format!("ADK Create: {name}"),
            command: Some("adk".into()),
            args: vec!["create".into(), name.to_string()],
            command_label: format!("adk create {name}"),
            cwd,
            env: Default::default(),
            use_new_terminal: true,
            allow_concurrent_runs: false,
            reveal: RevealStrategy::Always,
            reveal_target: RevealTarget::Dock,
            hide: HideStrategy::Never,
            shell: Shell::System,
            show_summary: true,
            show_command: true,
            show_rerun: false,
            save: SaveStrategy::None,
        },
        window,
        cx,
    );
}

fn project_root(workspace: &Workspace, cx: &App) -> Option<PathBuf> {
    workspace
        .project()
        .read(cx)
        .visible_worktrees(cx)
        .next()
        .map(|wt| wt.read(cx).abs_path().to_path_buf())
}
