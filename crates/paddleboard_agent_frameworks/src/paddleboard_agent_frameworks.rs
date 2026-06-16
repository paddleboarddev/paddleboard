mod a2a;
mod adk;
mod autogen;
mod crewai;
mod langgraph;
mod scaffold_modal;

use std::path::PathBuf;

use gpui::{App, AppContext as _, WeakEntity};
use workspace::Workspace;

pub use scaffold_modal::ScaffoldAgentModal;

#[derive(Default)]
pub(crate) struct FrameworkStates {
    pub adk: FrameworkState,
    pub langgraph: FrameworkState,
    pub crewai: FrameworkState,
    pub autogen: FrameworkState,
    pub a2a: FrameworkState,
}

impl gpui::Global for FrameworkStates {}

#[derive(Default)]
pub(crate) struct FrameworkState {
    pub child: Option<smol::process::Child>,
    pub output_buffer: Option<WeakEntity<language::Buffer>>,
}

pub fn init(cx: &mut App) {
    cx.set_global(FrameworkStates::default());
    adk::init(cx);
    langgraph::init(cx);
    crewai::init(cx);
    autogen::init(cx);
    a2a::init(cx);
}

pub(crate) fn parse_port_from_line(line: &str) -> Option<u16> {
    let http_prefix = "http://";
    let idx = line.find(http_prefix)?;
    let rest = &line[idx + http_prefix.len()..];
    let colon = rest.rfind(':')?;
    let after_colon = &rest[colon + 1..];
    let port_str: String = after_colon.chars().take_while(|c| c.is_ascii_digit()).collect();
    port_str.parse().ok()
}

pub(crate) fn project_root(workspace: &Workspace, cx: &App) -> Option<PathBuf> {
    workspace
        .project()
        .read(cx)
        .visible_worktrees(cx)
        .next()
        .map(|wt| wt.read(cx).abs_path().to_path_buf())
}

/// Best-effort check for whether a visible worktree declares a Python dependency
/// (scans `pyproject.toml` / `requirements.txt` for the package name). Used by the
/// framework auto-detection toasts where there's no single marker file.
pub(crate) fn worktree_declares_dependency(workspace: &Workspace, cx: &App, needle: &str) -> bool {
    workspace
        .project()
        .read(cx)
        .visible_worktrees(cx)
        .any(|wt| {
            let root = wt.read(cx).abs_path();
            ["pyproject.toml", "requirements.txt"].iter().any(|file| {
                std::fs::read_to_string(root.join(file))
                    .map(|contents| contents.contains(needle))
                    .unwrap_or(false)
            })
        })
}

pub(crate) fn run_framework_server(
    workspace: &mut Workspace,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<Workspace>,
    config: RunConfig,
) {
    let cwd = project_root(workspace, cx);
    let project = workspace.project().clone();
    let workspace_handle = workspace.weak_handle();
    let window_handle = window.window_handle();

    let (line_tx, line_rx) = async_channel::bounded::<String>(64);

    cx.spawn(async move |_weak_workspace, cx| {
        let mut cmd = smol::process::Command::new(config.command);
        cmd.args(config.args);
        if let Some(ref dir) = cwd {
            cmd.current_dir(dir);
        }
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(err) => {
                log::error!("failed to spawn {}: {err:#}", config.label);
                return;
            }
        };

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let existing_buffer = cx.update(|cx| {
            let states = cx.default_global::<FrameworkStates>();
            let state = (config.get_state)(states);
            if let Some(mut prev) = state.child.take() {
                let _ = prev.kill();
                log::info!("killed previous {} process before starting new one", config.label);
            }
            state.child = Some(child);
            state.output_buffer.as_ref().and_then(|w| w.upgrade())
        });

        spawn_readers(cx.background_executor(), stdout, stderr, line_tx);

        let buffer = if let Some(buffer) = existing_buffer {
            buffer.update(cx, |buffer, cx| {
                let len = buffer.len();
                if len > 0 {
                    buffer.edit([(0..len, "")], None, cx);
                }
            });
            buffer
        } else {
            let create_buffer = project.update(cx, |project, cx| {
                project.create_buffer(None, false, cx)
            });
            let buffer = match create_buffer.await {
                Ok(buffer) => buffer,
                Err(err) => {
                    log::error!("failed to create {} log buffer: {err:#}", config.label);
                    return;
                }
            };
            buffer.update(cx, |buffer, cx| {
                buffer.set_capability(language::Capability::ReadOnly, cx);
            });
            let title = config.label.to_string();
            let weak_ws = workspace_handle.clone();
            let _ = cx.update_window(window_handle, |_view, window, cx| {
                let multibuffer = cx.new(|cx| {
                    multi_buffer::MultiBuffer::singleton(buffer.clone(), cx)
                        .with_title(title)
                });
                let editor_entity = cx.new(|cx| {
                    let mut editor_view =
                        editor::Editor::for_multibuffer(multibuffer, None, window, cx);
                    editor_view.set_read_only(true);
                    editor_view
                });
                weak_ws
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
            buffer
        };

        cx.update(|cx| {
            let states = cx.default_global::<FrameworkStates>();
            (config.get_state)(states).output_buffer = Some(buffer.downgrade());
        });

        let weak_buffer = buffer.downgrade();
        let mut port_registered = false;
        let mut lines_seen = 0u32;
        let label = config.label;
        let fallback_port = config.fallback_port;
        let landing_path = config.landing_path;

        while let Ok(line) = line_rx.recv().await {
            lines_seen += 1;

            if let Some(fallback) = fallback_port
                && !port_registered
            {
                if let Some(port) = parse_port_from_line(&line) {
                    port_registered = true;
                    register_port(&mut *cx, &workspace_handle, label, port, landing_path);
                } else if lines_seen > 50 {
                    port_registered = true;
                    log::warn!(
                        "{label} port not detected in first 50 lines, falling back to port {fallback}"
                    );
                    register_port(cx, &workspace_handle, label, fallback, landing_path);
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
    })
    .detach();
}

pub(crate) struct RunConfig {
    pub command: &'static str,
    pub args: &'static [&'static str],
    pub label: &'static str,
    /// Port to register a forward for if none is parsed from the process output.
    /// `None` for one-shot/non-server frameworks (e.g. `crewai run`), which skips
    /// port forwarding entirely and just streams output to the tab.
    pub fallback_port: Option<u16>,
    /// Optional landing path the forwarded-port chip should deep-link to (must start
    /// with `/`). Most frameworks serve a real homepage at `/` and leave this `None`;
    /// A2A serves a POST-only JSON-RPC endpoint at `/` (a GET returns 405), so it points
    /// at the browser-viewable Agent Card instead.
    pub landing_path: Option<&'static str>,
    pub get_state: fn(&mut FrameworkStates) -> &mut FrameworkState,
}

fn spawn_readers(
    executor: &gpui::BackgroundExecutor,
    stdout: Option<smol::process::ChildStdout>,
    stderr: Option<smol::process::ChildStderr>,
    line_tx: async_channel::Sender<String>,
) {
    let tx_for_stdout = line_tx.clone();
    executor
        .spawn(async move {
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
        })
        .detach();

    executor
        .spawn(async move {
            use smol::io::{AsyncBufReadExt, BufReader};
            if let Some(stderr) = stderr {
                let mut reader = BufReader::new(stderr);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            if line_tx.send(line.clone()).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            }
        })
        .detach();
}

struct PortToast;

fn register_port(
    cx: &mut gpui::AsyncApp,
    workspace_handle: &WeakEntity<Workspace>,
    label: &str,
    port: u16,
    landing_path: Option<&'static str>,
) {
    cx.update(|cx| {
        browser::ForwardedPorts::register(
            cx,
            browser::ForwardedPort {
                label: label.into(),
                host_port: port,
                container_id: None,
                path: landing_path.map(Into::into),
            },
        );
    });
    let toast_label = format!("{label} running on port {port}");
    let _ = workspace_handle.update(&mut *cx, |workspace, cx| {
        workspace.show_toast(
            workspace::Toast::new(
                workspace::notifications::NotificationId::unique::<PortToast>(),
                toast_label,
            )
            .autohide(),
            cx,
        );
    });
}

pub(crate) fn stop_framework(
    cx: &mut App,
    label: &str,
    get_state: fn(&mut FrameworkStates) -> &mut FrameworkState,
) {
    let states = cx.default_global::<FrameworkStates>();
    let state = get_state(states);
    if let Some(mut child) = state.child.take() {
        let _ = child.kill();
        log::info!("{label} server stopped");
    }
    if let Some(ports) = browser::ForwardedPorts::try_global(cx) {
        let port = ports
            .ports()
            .iter()
            .find(|p| p.label.as_ref() == label)
            .map(|p| p.host_port);
        if let Some(port) = port {
            browser::ForwardedPorts::stop(cx, port);
        }
    }
}

pub(crate) fn spawn_in_terminal(
    workspace: &mut Workspace,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<Workspace>,
    task_id: &str,
    label: &str,
    command: &str,
    args: Vec<String>,
) {
    let cwd = project_root(workspace, cx);
    let command_label = format!("{command} {}", args.join(" "));
    let _ = workspace.spawn_in_terminal(
        task::SpawnInTerminal {
            id: task::TaskId(task_id.into()),
            full_label: label.to_string(),
            label: label.to_string(),
            command: Some(command.into()),
            args,
            command_label,
            cwd,
            env: Default::default(),
            use_new_terminal: true,
            allow_concurrent_runs: false,
            reveal: task::RevealStrategy::Always,
            reveal_target: task::RevealTarget::Dock,
            hide: task::HideStrategy::Never,
            shell: task::Shell::System,
            show_summary: true,
            show_command: true,
            show_rerun: false,
            save: task::SaveStrategy::None,
        },
        window,
        cx,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_uvicorn_port() {
        assert_eq!(
            parse_port_from_line(
                "INFO:     Uvicorn running on http://0.0.0.0:8000 (Press CTRL+C to quit)"
            ),
            Some(8000)
        );
    }

    #[test]
    fn parse_custom_port() {
        assert_eq!(
            parse_port_from_line(
                "INFO:     Uvicorn running on http://127.0.0.1:9090 (Press CTRL+C to quit)"
            ),
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
            None
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
