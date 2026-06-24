use std::time::Duration;

use gpui::{Action as _, App, Context, Window};
use workspace::Workspace;
use workspace::notifications::NotificationId;

use crate::{RunConfig, run_framework_server, stop_framework, worktree_declares_dependency};

struct CrewAiProjectDetected;

pub(crate) fn init(cx: &mut App) {
    cx.observe_new(
        |workspace: &mut Workspace,
         _window: Option<&mut Window>,
         cx: &mut Context<Workspace>| {
            workspace.register_action(
                |workspace, _: &paddleboard_actions::crewai::ScaffoldAgent, window, cx| {
                    crate::ScaffoldAgentModal::toggle(workspace, "crewai", window, cx);
                },
            );

            workspace.register_action(
                |workspace, _: &paddleboard_actions::crewai::RunAgent, window, cx| {
                    run_framework_server(
                        workspace,
                        window,
                        cx,
                        RunConfig {
                            command: "crewai",
                            args: &["run"],
                            label: "CrewAI Run",
                            // `crewai run` executes the crew and exits — no server/port.
                            fallback_port: None,
                            landing_path: None,
                            get_state: |s| &mut s.crewai,
                        },
                    );
                },
            );

            workspace.register_action(
                |_workspace, _: &paddleboard_actions::crewai::StopAgent, _window, cx| {
                    stop_framework(cx, "CrewAI Run", |s| &mut s.crewai);
                },
            );

            detect_crewai_project(cx);
        },
    )
    .detach();
}

fn detect_crewai_project(cx: &mut Context<Workspace>) {
    cx.spawn(async move |weak_workspace, cx| {
        cx.background_executor()
            .timer(Duration::from_millis(500))
            .await;
        let _ = weak_workspace.update(&mut *cx, |workspace, cx| {
            if !worktree_declares_dependency(workspace, cx, "crewai") {
                return;
            }
            let id = NotificationId::unique::<CrewAiProjectDetected>();
            workspace.show_toast(
                workspace::Toast::new(id, "CrewAI project detected").on_click(
                    "Run Agent",
                    |window, cx| {
                        window.dispatch_action(
                            paddleboard_actions::crewai::RunAgent.boxed_clone(),
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
