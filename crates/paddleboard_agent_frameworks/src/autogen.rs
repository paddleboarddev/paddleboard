use std::time::Duration;

use gpui::{Action as _, App, Context, Window};
use workspace::Workspace;
use workspace::notifications::NotificationId;

use crate::{RunConfig, run_framework_server, stop_framework, worktree_declares_dependency};

struct AutoGenProjectDetected;

pub(crate) fn init(cx: &mut App) {
    cx.observe_new(
        |workspace: &mut Workspace,
         _window: Option<&mut Window>,
         cx: &mut Context<Workspace>| {
            // AutoGen has no project-scaffold CLI; it provides AutoGen Studio, a web UI.
            workspace.register_action(
                |workspace, _: &paddleboard_actions::autogen::RunAgent, window, cx| {
                    run_framework_server(
                        workspace,
                        window,
                        cx,
                        RunConfig {
                            command: "autogenstudio",
                            args: &["ui", "--port", "8081"],
                            label: "AutoGen Studio",
                            fallback_port: Some(8081),
                            landing_path: None,
                            get_state: |s| &mut s.autogen,
                        },
                    );
                },
            );

            workspace.register_action(
                |_workspace, _: &paddleboard_actions::autogen::StopAgent, _window, cx| {
                    stop_framework(cx, "AutoGen Studio", |s| &mut s.autogen);
                },
            );

            detect_autogen_project(cx);
        },
    )
    .detach();
}

fn detect_autogen_project(cx: &mut Context<Workspace>) {
    cx.spawn(async move |weak_workspace, cx| {
        cx.background_executor()
            .timer(Duration::from_millis(500))
            .await;
        let _ = weak_workspace.update(&mut *cx, |workspace, cx| {
            if !worktree_declares_dependency(workspace, cx, "autogen") {
                return;
            }
            let id = NotificationId::unique::<AutoGenProjectDetected>();
            workspace.show_toast(
                workspace::Toast::new(id, "AutoGen project detected").on_click(
                    "Open Studio",
                    |window, cx| {
                        window.dispatch_action(
                            paddleboard_actions::autogen::RunAgent.boxed_clone(),
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
