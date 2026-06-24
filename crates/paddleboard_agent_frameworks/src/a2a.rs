use std::time::Duration;

use gpui::{Action as _, App, Context, Window};
use workspace::Workspace;
use workspace::notifications::NotificationId;

use crate::{RunConfig, run_framework_server, stop_framework, worktree_declares_dependency};

struct A2aProjectDetected;

pub(crate) fn init(cx: &mut App) {
    cx.observe_new(
        |workspace: &mut Workspace,
         _window: Option<&mut Window>,
         cx: &mut Context<Workspace>| {
            // A2A is an SDK, not a CLI with a scaffold command, so there's no ScaffoldAgent
            // here (like AutoGen). The canonical run for an a2a-sdk project is `uv run .`.
            workspace.register_action(
                |workspace, _: &paddleboard_actions::a2a::RunAgent, window, cx| {
                    run_framework_server(
                        workspace,
                        window,
                        cx,
                        RunConfig {
                            command: "uv",
                            args: &["run", "."],
                            label: "A2A Agent",
                            fallback_port: Some(9999),
                            // A2A's `/` is a POST-only JSON-RPC/SSE endpoint (a browser GET
                            // returns 405); the Agent Card is the viewable surface.
                            landing_path: Some("/.well-known/agent-card.json"),
                            get_state: |s| &mut s.a2a,
                        },
                    );
                },
            );

            workspace.register_action(
                |_workspace, _: &paddleboard_actions::a2a::StopAgent, _window, cx| {
                    stop_framework(cx, "A2A Agent", |s| &mut s.a2a);
                },
            );

            detect_a2a_project(cx);
        },
    )
    .detach();
}

fn detect_a2a_project(cx: &mut Context<Workspace>) {
    cx.spawn(async move |weak_workspace, cx| {
        cx.background_executor()
            .timer(Duration::from_millis(500))
            .await;
        let _ = weak_workspace.update(&mut *cx, |workspace, cx| {
            if !worktree_declares_dependency(workspace, cx, "a2a-sdk") {
                return;
            }
            let id = NotificationId::unique::<A2aProjectDetected>();
            workspace.show_toast(
                workspace::Toast::new(id, "A2A project detected").on_click(
                    "Run Agent",
                    |window, cx| {
                        window
                            .dispatch_action(paddleboard_actions::a2a::RunAgent.boxed_clone(), cx);
                    },
                ),
                cx,
            );
        });
    })
    .detach();
}
