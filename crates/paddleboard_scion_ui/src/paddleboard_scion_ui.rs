use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use gpui::{App, AppContext as _, Context, Entity, EventEmitter, Global, Task, Window};
use gpui_tokio::Tokio;
use paddleboard_scion::{AgentInfo, AgentPhase, ScionCli, StartAgentOptions};
use workspace::Workspace;

const POLL_INTERVAL: Duration = Duration::from_secs(5);

pub struct ScionStore {
    pub cli: Arc<ScionCli>,
    agents: Vec<AgentInfo>,
    available: bool,
    _poll_task: Option<Task<()>>,
}

#[derive(Clone, Debug)]
pub enum ScionStoreEvent {
    AgentsUpdated,
}

impl EventEmitter<ScionStoreEvent> for ScionStore {}

impl ScionStore {
    pub fn new(project_dir: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        let mut cli = ScionCli::new();
        if let Some(dir) = project_dir {
            cli = cli.with_project_dir(dir);
        }
        let cli = Arc::new(cli);
        let available = cli.is_available();

        let mut store = Self {
            cli,
            agents: Vec::new(),
            available,
            _poll_task: None,
        };

        if available {
            store.schedule_poll(cx);
        }

        store
    }

    fn schedule_poll(&mut self, cx: &mut Context<Self>) {
        let task = Tokio::spawn_result(cx, {
            let cli = self.cli.clone();
            async move { cli.list_agents(false, false).await }
        });

        self._poll_task = Some(cx.spawn(async move |this, cx| {
            match task.await {
                Ok(agents) => {
                    this.update(cx, |store, cx| {
                        store.agents = agents;
                        cx.emit(ScionStoreEvent::AgentsUpdated);
                        cx.notify();
                    })
                    .ok();
                }
                Err(err) => {
                    log::warn!("scion poll failed: {err:#}");
                }
            }

            cx.background_executor().timer(POLL_INTERVAL).await;

            this.update(cx, |store, cx| {
                store.schedule_poll(cx);
            })
            .ok();
        }));
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let task = Tokio::spawn_result(cx, {
            let cli = self.cli.clone();
            async move { cli.list_agents(false, false).await }
        });

        cx.spawn(async move |this, cx| {
            if let Ok(agents) = task.await {
                this.update(cx, |store, cx| {
                    store.agents = agents;
                    cx.emit(ScionStoreEvent::AgentsUpdated);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    pub fn agents(&self) -> &[AgentInfo] {
        &self.agents
    }

    pub fn is_available(&self) -> bool {
        self.available
    }

    pub fn running_agents(&self) -> Vec<&AgentInfo> {
        self.agents
            .iter()
            .filter(|a| a.phase == Some(AgentPhase::Running))
            .collect()
    }

    pub fn start_agent(
        &self,
        name: String,
        task_text: Option<String>,
        options: StartAgentOptions,
        cx: &Context<Self>,
    ) -> Task<Result<String>> {
        let cli = self.cli.clone();
        Tokio::spawn_result(cx, async move {
            cli.start_agent(&name, task_text.as_deref(), &options).await
        })
    }

    pub fn stop_agent(&self, name: String, cx: &Context<Self>) -> Task<Result<()>> {
        let cli = self.cli.clone();
        Tokio::spawn_result(cx, async move {
            cli.stop_agent(Some(&name), false).await
        })
    }

    pub fn sync_from(&self, name: String, cx: &Context<Self>) -> Task<Result<String>> {
        let cli = self.cli.clone();
        Tokio::spawn_result(cx, async move { cli.sync_from(&name).await })
    }
}

pub struct ScionStoreGlobal(pub Entity<ScionStore>);

impl Global for ScionStoreGlobal {}

pub fn init(cx: &mut App) {
    cx.observe_new(
        |workspace: &mut Workspace,
         _window: Option<&mut Window>,
         cx: &mut Context<Workspace>| {
            let project_dir = workspace
                .project()
                .read(cx)
                .visible_worktrees(cx)
                .next()
                .map(|wt| wt.read(cx).abs_path().to_path_buf());

            let scion_store = cx.new(|cx| ScionStore::new(project_dir, cx));

            if !cx.has_global::<ScionStoreGlobal>() {
                cx.set_global(ScionStoreGlobal(scion_store.clone()));
            }

            let store = scion_store.clone();
            workspace.register_action(
                move |workspace, _: &paddleboard_actions::scion::StartAgent, _window, cx| {
                    handle_start_agent(workspace, &store, cx);
                },
            );

            let store = scion_store.clone();
            workspace.register_action(
                move |workspace, _: &paddleboard_actions::scion::StopAgent, _window, cx| {
                    handle_stop_agent(workspace, &store, cx);
                },
            );

            let store = scion_store.clone();
            workspace.register_action(
                move |workspace, _: &paddleboard_actions::scion::SyncFromAgent, _window, cx| {
                    handle_sync_from(workspace, &store, cx);
                },
            );

            let store = scion_store;
            workspace.register_action(
                move |workspace, _: &paddleboard_actions::scion::ShowAgentLogs, _window, cx| {
                    handle_show_logs(workspace, &store, cx);
                },
            );
        },
    )
    .detach();
}

fn handle_start_agent(
    workspace: &mut Workspace,
    store: &Entity<ScionStore>,
    cx: &mut Context<Workspace>,
) {
    if !store.read(cx).is_available() {
        workspace.show_error(
            &anyhow::anyhow!(
                "Scion is not installed. Install it with: \
                 go install github.com/GoogleCloudPlatform/scion/cmd/scion@latest"
            ),
            cx,
        );
        return;
    }

    let task = store.update(cx, |store, cx| {
        let name = format!("pb-agent-{}", cx.entity_id().as_u64() % 10000);
        store.start_agent(name, None, StartAgentOptions::default(), cx)
    });

    let store = store.clone();
    cx.spawn(async move |this, cx| {
        match task.await {
            Ok(_) => {
                store.update(cx, |store, cx| store.refresh(cx));
            }
            Err(err) => {
                this.update(cx, |workspace, cx| {
                    workspace.show_error(&err, cx);
                })
                .ok();
            }
        }
    })
    .detach();
}

fn handle_stop_agent(
    workspace: &mut Workspace,
    store: &Entity<ScionStore>,
    cx: &mut Context<Workspace>,
) {
    let running = store.read(cx).running_agents();
    let Some(first) = running.first() else {
        workspace.show_error(&anyhow::anyhow!("No running Scion agents to stop."), cx);
        return;
    };

    let name = first.name.clone();
    let task = store.update(cx, |store, cx| store.stop_agent(name, cx));

    let store = store.clone();
    cx.spawn(async move |this, cx| {
        match task.await {
            Ok(()) => {
                store.update(cx, |store, cx| store.refresh(cx));
            }
            Err(err) => {
                this.update(cx, |workspace, cx| {
                    workspace.show_error(&err, cx);
                })
                .ok();
            }
        }
    })
    .detach();
}

fn handle_sync_from(
    workspace: &mut Workspace,
    store: &Entity<ScionStore>,
    cx: &mut Context<Workspace>,
) {
    let agents = store.read(cx).agents();
    let Some(first) = agents.first() else {
        workspace.show_error(&anyhow::anyhow!("No Scion agents to sync from."), cx);
        return;
    };

    let name = first.name.clone();
    let task = store.update(cx, |store, cx| store.sync_from(name, cx));

    cx.spawn(async move |this, cx| {
        match task.await {
            Ok(_output) => {
                log::info!("scion sync from completed");
            }
            Err(err) => {
                this.update(cx, |workspace, cx| {
                    workspace.show_error(&err, cx);
                })
                .ok();
            }
        }
    })
    .detach();
}

fn handle_show_logs(
    workspace: &mut Workspace,
    store: &Entity<ScionStore>,
    cx: &mut Context<Workspace>,
) {
    let agents = store.read(cx).agents();
    let Some(first) = agents.first() else {
        workspace.show_error(&anyhow::anyhow!("No Scion agents to show logs for."), cx);
        return;
    };

    let name = first.name.clone();
    let cli = store.read(cx).cli.clone();

    let task = Tokio::spawn_result(cx, async move {
        cli.agent_logs(&name, Some(100)).await
    });

    cx.spawn(async move |this, cx| {
        match task.await {
            Ok(logs) => {
                log::info!("scion agent logs ({} bytes):\n{logs}", logs.len());
            }
            Err(err) => {
                this.update(cx, |workspace, cx| {
                    workspace.show_error(&err, cx);
                })
                .ok();
            }
        }
    })
    .detach();
}
