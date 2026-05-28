use std::time::Duration;

use gpui::{Action as _, App, Context, Window};
use workspace::Workspace;
use workspace::notifications::NotificationId;

use crate::{RunConfig, run_framework_server, stop_framework};

struct AdkProjectDetected;

pub(crate) fn init(cx: &mut App) {
    cx.observe_new(
        |workspace: &mut Workspace,
         _window: Option<&mut Window>,
         cx: &mut Context<Workspace>| {
            workspace.register_action(
                |workspace, _: &paddleboard_actions::adk::ScaffoldAgent, window, cx| {
                    crate::ScaffoldAgentModal::toggle(workspace, "adk", window, cx);
                },
            );

            workspace.register_action(
                |workspace, _: &paddleboard_actions::adk::RunAgent, window, cx| {
                    run_framework_server(
                        workspace,
                        window,
                        cx,
                        RunConfig {
                            command: "adk",
                            args: &["web"],
                            label: "ADK Web",
                            fallback_port: 8000,
                            get_state: |s| &mut s.adk,
                        },
                    );
                },
            );

            workspace.register_action(
                |_workspace, _: &paddleboard_actions::adk::StopAgent, _window, cx| {
                    stop_framework(cx, "ADK Web", |s| &mut s.adk);
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
