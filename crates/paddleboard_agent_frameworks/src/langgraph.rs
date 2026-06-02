use std::time::Duration;

use gpui::{Action as _, App, Context, Window};
use workspace::Workspace;
use workspace::notifications::NotificationId;

use crate::{RunConfig, run_framework_server, stop_framework};

struct LangGraphProjectDetected;

pub(crate) fn init(cx: &mut App) {
    cx.observe_new(
        |workspace: &mut Workspace,
         _window: Option<&mut Window>,
         cx: &mut Context<Workspace>| {
            workspace.register_action(
                |workspace, _: &paddleboard_actions::langgraph::ScaffoldAgent, window, cx| {
                    crate::ScaffoldAgentModal::toggle(workspace, "langgraph", window, cx);
                },
            );

            workspace.register_action(
                |workspace, _: &paddleboard_actions::langgraph::RunAgent, window, cx| {
                    run_framework_server(
                        workspace,
                        window,
                        cx,
                        RunConfig {
                            command: "langgraph",
                            args: &["dev"],
                            label: "LangGraph Studio",
                            fallback_port: Some(2024),
                            get_state: |s| &mut s.langgraph,
                        },
                    );
                },
            );

            workspace.register_action(
                |_workspace, _: &paddleboard_actions::langgraph::StopAgent, _window, cx| {
                    stop_framework(cx, "LangGraph Studio", |s| &mut s.langgraph);
                },
            );

            detect_langgraph_project(cx);
        },
    )
    .detach();
}

fn detect_langgraph_project(cx: &mut Context<Workspace>) {
    cx.spawn(async move |weak_workspace, cx| {
        cx.background_executor()
            .timer(Duration::from_millis(500))
            .await;
        let _ = weak_workspace.update(&mut *cx, |workspace, cx| {
            if !has_langgraph_markers(workspace, cx) {
                return;
            }
            let id = NotificationId::unique::<LangGraphProjectDetected>();
            workspace.show_toast(
                workspace::Toast::new(id, "LangGraph project detected").on_click(
                    "Run Agent",
                    |window, cx| {
                        window.dispatch_action(
                            paddleboard_actions::langgraph::RunAgent.boxed_clone(),
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

fn has_langgraph_markers(workspace: &Workspace, cx: &App) -> bool {
    let project = workspace.project().read(cx);
    project.visible_worktrees(cx).any(|wt| {
        let root = wt.read(cx).abs_path();
        root.join("langgraph.json").exists()
    })
}
