mod scaffold_modal;

use std::path::PathBuf;
use std::time::Duration;

use gpui::{Action as _, App, AppContext as _, Context, Window};
use task::{HideStrategy, RevealStrategy, RevealTarget, SaveStrategy, Shell, SpawnInTerminal, TaskId};
use workspace::Workspace;
use workspace::notifications::NotificationId;

pub use scaffold_modal::ScaffoldAgentModal;

struct AdkProjectDetected;
struct AdkWebRunning;

// PaddleBoard: global holding the running `adk web` child process for StopAgent.
#[derive(Default)]
struct AdkChildProcess(Option<smol::process::Child>);

impl gpui::Global for AdkChildProcess {}

pub fn init(cx: &mut App) {
    cx.set_global(AdkChildProcess(None));

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

            workspace.register_action(
                |_workspace, _: &paddleboard_actions::adk::StopAgent, _window, cx| {
                    handle_stop_agent(cx);
                },
            );

            detect_adk_project(cx);
        },
    )
    .detach();
}

fn handle_stop_agent(cx: &mut App) {
    let global = cx.default_global::<AdkChildProcess>();
    if let Some(mut child) = global.0.take() {
        let _ = child.kill();
        log::info!("ADK Web server stopped");
    }
    // Remove the forwarded port entry. We don't know the exact port,
    // but all ADK-registered ports use the "ADK Web" label and no container_id.
    if let Some(ports) = browser::ForwardedPorts::try_global(cx) {
        let adk_port = ports.ports().iter().find(|p| p.label.as_ref() == "ADK Web").map(|p| p.host_port);
        if let Some(port) = adk_port {
            browser::ForwardedPorts::stop(cx, port);
        }
    }
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

fn parse_port_from_line(line: &str) -> Option<u16> {
    // Matches patterns like "http://0.0.0.0:8000" or "http://127.0.0.1:8080"
    let http_prefix = "http://";
    let idx = line.find(http_prefix)?;
    let rest = &line[idx + http_prefix.len()..];
    let colon = rest.rfind(':')?;
    let after_colon = &rest[colon + 1..];
    let port_str: String = after_colon.chars().take_while(|c| c.is_ascii_digit()).collect();
    port_str.parse().ok()
}

fn handle_run_agent(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let cwd = project_root(workspace, cx);
    let project = workspace.project().clone();
    let workspace_handle = workspace.weak_handle();
    let window_handle = window.window_handle();

    let (line_tx, line_rx) = async_channel::bounded::<String>(64);

    cx.spawn(async move |_weak_workspace, cx| {
        let mut cmd = smol::process::Command::new("adk");
        cmd.args(["web"]);
        if let Some(ref dir) = cwd {
            cmd.current_dir(dir);
        }
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(err) => {
                log::error!("failed to spawn adk web: {err:#}");
                return;
            }
        };

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        // PaddleBoard: store the child process so StopAgent can kill it.
        cx.update(|cx| {
            cx.default_global::<AdkChildProcess>().0 = Some(child);
        });

        let tx_for_stdout = line_tx.clone();
        let tx_for_stderr = line_tx;

        cx.background_executor().spawn({
            async move {
                use smol::io::{AsyncBufReadExt, BufReader};
                if let Some(stdout) = stdout {
                    let mut reader = BufReader::new(stdout);
                    let mut line = String::new();
                    loop {
                        line.clear();
                        match reader.read_line(&mut line).await {
                            Ok(0) | Err(_) => break,
                            Ok(_) => {
                                if tx_for_stdout.send(line.clone()).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }).detach();

        cx.background_executor().spawn({
            async move {
                use smol::io::{AsyncBufReadExt, BufReader};
                if let Some(stderr) = stderr {
                    let mut reader = BufReader::new(stderr);
                    let mut line = String::new();
                    loop {
                        line.clear();
                        match reader.read_line(&mut line).await {
                            Ok(0) | Err(_) => break,
                            Ok(_) => {
                                if tx_for_stderr.send(line.clone()).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }).detach();

        let create_buffer = project.update(cx, |project, cx| {
            project.create_buffer(None, false, cx)
        });

        let buffer = match create_buffer.await {
            Ok(buffer) => buffer,
            Err(err) => {
                log::error!("failed to create ADK log buffer: {err:#}");
                return;
            }
        };

        buffer.update(cx, |buffer, cx| {
            buffer.set_capability(language::Capability::ReadOnly, cx);
        });

        let tab_title = "ADK Web".to_string();
        let _ = cx.update_window(window_handle, |_view, window, cx| {
            let multibuffer = cx.new(|cx| {
                multi_buffer::MultiBuffer::singleton(buffer.clone(), cx)
                    .with_title(tab_title)
            });
            let editor_entity = cx.new(|cx| {
                let mut editor_view =
                    editor::Editor::for_multibuffer(multibuffer, None, window, cx);
                editor_view.set_read_only(true);
                editor_view
            });
            workspace_handle
                .update(cx, |workspace, cx| {
                    workspace.add_item_to_active_pane(
                        Box::new(editor_entity),
                        None,
                        true,
                        window,
                        cx,
                    );
                })
                .ok();
        });

        // Stream lines into the buffer. Register port when detected.
        let weak_buffer = buffer.downgrade();
        let mut port_registered = false;
        let mut lines_seen = 0u32;

        while let Ok(line) = line_rx.recv().await {
            lines_seen += 1;

            if !port_registered {
                if let Some(port) = parse_port_from_line(&line) {
                    port_registered = true;
                    cx.update(|cx| {
                        browser::ForwardedPorts::register(
                            cx,
                            browser::ForwardedPort {
                                label: "ADK Web".into(),
                                host_port: port,
                                container_id: None,
                            },
                        );
                    });
                    let _ = workspace_handle.update(&mut *cx, |workspace, cx| {
                        workspace.show_toast(
                            workspace::Toast::new(
                                NotificationId::unique::<AdkWebRunning>(),
                                format!("ADK Web running on port {port}"),
                            )
                            .autohide(),
                            cx,
                        );
                    });
                } else if lines_seen > 50 && !port_registered {
                    port_registered = true;
                    log::warn!(
                        "ADK port not detected in first 50 lines, falling back to port 8000"
                    );
                    cx.update(|cx| {
                        browser::ForwardedPorts::register(
                            cx,
                            browser::ForwardedPort {
                                label: "ADK Web".into(),
                                host_port: 8000,
                                container_id: None,
                            },
                        );
                    });
                }
            }

            let result = weak_buffer.update(cx, |buffer, cx| {
                let len = buffer.len();
                buffer.edit([(len..len, line.as_str())], None, cx);
            });
            if result.is_err() {
                break;
            }
        }

        // Child process is stored in AdkChildProcess global; it will be
        // cleaned up by StopAgent or when the app exits.
    })
    .detach();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_uvicorn_port() {
        assert_eq!(
            parse_port_from_line("INFO:     Uvicorn running on http://0.0.0.0:8000 (Press CTRL+C to quit)"),
            Some(8000)
        );
    }

    #[test]
    fn parse_custom_port() {
        assert_eq!(
            parse_port_from_line("INFO:     Uvicorn running on http://127.0.0.1:9090 (Press CTRL+C to quit)"),
            Some(9090)
        );
    }

    #[test]
    fn parse_no_port() {
        assert_eq!(
            parse_port_from_line("INFO:     Started server process [12345]"),
            None
        );
    }

    #[test]
    fn parse_https_port() {
        assert_eq!(
            parse_port_from_line("Server at https://localhost:8443/api"),
            None // only matches http:// not https://
        );
    }

    #[test]
    fn parse_port_in_middle_of_text() {
        assert_eq!(
            parse_port_from_line("Navigate to http://localhost:8080/ for the UI"),
            Some(8080)
        );
    }
}
