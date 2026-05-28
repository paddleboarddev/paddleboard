mod scaffold_modal;

use std::path::PathBuf;
use std::time::Duration;

use gpui::{Action as _, App, Context, Window};
use task::{HideStrategy, RevealStrategy, RevealTarget, SaveStrategy, Shell, SpawnInTerminal, TaskId};
use workspace::Workspace;
use workspace::notifications::NotificationId;

pub use scaffold_modal::ScaffoldAgentModal;

struct AdkProjectDetected;

pub fn init(cx: &mut App) {
    cx.observe_new(
        |workspace: &mut Workspace,
         _window: Option<&mut Window>,
         cx: &mut Context<Workspace>| {
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

            detect_adk_project(cx);
        },
    )
    .detach();
}

fn detect_adk_project(cx: &mut Context<Workspace>) {
    cx.spawn(async move |weak_workspace, cx| {
        cx.background_executor()
            .timer(Duration::from_millis(500))
            .await;
        let _ = weak_workspace.update(&mut *cx, |workspace, cx| {
            if !has_adk_markers(workspace, cx) {
                return;
            }
            let id = NotificationId::unique::<AdkProjectDetected>();
            workspace.show_toast(
                workspace::Toast::new(id, "ADK project detected").on_click(
                    "Run Agent",
                    |window, cx| {
                        window.dispatch_action(
                            paddleboard_actions::adk::RunAgent.boxed_clone(),
                            cx,
                        );
                    },
                ),
                cx,
            );
        });
    })
    .detach();
}

fn has_adk_markers(workspace: &Workspace, cx: &App) -> bool {
    let project = workspace.project().read(cx);
    project.visible_worktrees(cx).any(|wt| {
        let root = wt.read(cx).abs_path();
        root.join("agent.py").exists() || root.join("agent.yaml").exists()
    })
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

    browser::ForwardedPorts::register(
        cx,
        browser::ForwardedPort {
            label: "ADK Web".into(),
            host_port: 8000,
            container_id: None,
        },
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
