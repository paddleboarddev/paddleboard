mod start_agent_modal;

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use gpui::{App, AppContext as _, Context, Entity, EventEmitter, Global, Task, Window};
use gpui_tokio::Tokio;
use paddleboard_scion::{
    AgentActivity, AgentInfo, AgentPhase, ScionCli, StartAgentOptions, TemplateInfo,
};
use settings::Settings;
use workspace::{Toast, Workspace, notifications::NotificationId};

pub use start_agent_modal::StartAgentModal;

const POLL_INTERVAL: Duration = Duration::from_secs(5);
/// Cap on the retained activity feed (newest events are kept).
const MAX_EVENTS: usize = 60;

#[derive(Clone, Debug, PartialEq)]
struct AgentSnapshot {
    phase: Option<AgentPhase>,
    activity: Option<AgentActivity>,
}

impl From<&AgentInfo> for AgentSnapshot {
    fn from(agent: &AgentInfo) -> Self {
        Self {
            phase: agent.phase,
            activity: agent.activity,
        }
    }
}

/// The kind of lifecycle change recorded in the activity feed. Mirrors the
/// transitions emitted to OpenTelemetry in `detect_transitions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScionEventKind {
    Discovered,
    Phase,
    Activity,
    Disappeared,
}

/// A user-facing record of a Scion agent lifecycle change. The same transitions
/// are emitted to OTEL; these are retained in-memory so the orchestration panel
/// can render a live activity feed without a collector round-trip.
#[derive(Clone, Debug)]
pub struct ScionEvent {
    pub seq: u64,
    pub agent: String,
    pub kind: ScionEventKind,
    pub summary: String,
}

fn phase_label(phase: Option<AgentPhase>) -> String {
    phase
        .map(|p| p.to_string())
        .unwrap_or_else(|| "none".to_string())
}

fn activity_label(activity: Option<AgentActivity>) -> String {
    activity
        .map(|a| a.to_string())
        .unwrap_or_else(|| "none".to_string())
}

pub struct ScionStore {
    pub cli: Arc<ScionCli>,
    agents: Vec<AgentInfo>,
    prev_agent_states: HashMap<String, AgentSnapshot>,
    templates: Vec<TemplateInfo>,
    events: VecDeque<ScionEvent>,
    event_seq: u64,
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
            prev_agent_states: HashMap::new(),
            templates: Vec::new(),
            events: VecDeque::new(),
            event_seq: 0,
            available,
            _poll_task: None,
        };

        if available {
            store.fetch_templates(cx);
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
            let _span = tracing::info_span!("scion.poll_cycle").entered();

            match task.await {
                Ok(agents) => {
                    this.update(cx, |store, cx| {
                        store.detect_transitions(&agents);
                        store.agents = agents;
                        cx.emit(ScionStoreEvent::AgentsUpdated);
                        cx.notify();
                    })
                    .ok();
                }
                Err(err) => {
                    tracing::warn!(error = %err, "scion poll failed");
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

    fn detect_transitions(&mut self, new_agents: &[AgentInfo]) {
        let new_states: HashMap<String, AgentSnapshot> = new_agents
            .iter()
            .map(|a| (a.name.clone(), AgentSnapshot::from(a)))
            .collect();

        for agent in new_agents {
            let new_snap = AgentSnapshot::from(agent);
            // `.cloned()` releases the borrow of `prev_agent_states` so we can
            // record events on `self` inside the branch.
            if let Some(old_snap) = self.prev_agent_states.get(&agent.name).cloned() {
                if old_snap.phase != new_snap.phase {
                    let old_phase = phase_label(old_snap.phase);
                    let new_phase = phase_label(new_snap.phase);
                    tracing::info!(
                        scion.agent_name = %agent.name,
                        scion.phase.old = %old_phase,
                        scion.phase.new = %new_phase,
                        "scion agent phase transition"
                    );
                    self.push_event(
                        &agent.name,
                        ScionEventKind::Phase,
                        format!("{old_phase} → {new_phase}"),
                    );
                }
                if old_snap.activity != new_snap.activity {
                    let old_activity = activity_label(old_snap.activity);
                    let new_activity = activity_label(new_snap.activity);
                    tracing::info!(
                        scion.agent_name = %agent.name,
                        scion.activity.old = %old_activity,
                        scion.activity.new = %new_activity,
                        "scion agent activity transition"
                    );
                    self.push_event(
                        &agent.name,
                        ScionEventKind::Activity,
                        format!("{old_activity} → {new_activity}"),
                    );
                }
            } else {
                let phase = phase_label(new_snap.phase);
                let activity = activity_label(new_snap.activity);
                tracing::info!(
                    scion.agent_name = %agent.name,
                    scion.phase = %phase,
                    scion.activity = %activity,
                    "scion agent discovered"
                );
                self.push_event(
                    &agent.name,
                    ScionEventKind::Discovered,
                    format!("discovered ({phase})"),
                );
            }
        }

        // Collect first so the `keys()` borrow ends before we record events.
        let disappeared: Vec<String> = self
            .prev_agent_states
            .keys()
            .filter(|name| !new_states.contains_key(*name))
            .cloned()
            .collect();
        for name in disappeared {
            tracing::info!(scion.agent_name = %name, "scion agent disappeared");
            self.push_event(&name, ScionEventKind::Disappeared, "removed".to_string());
        }

        self.prev_agent_states = new_states;
    }

    fn push_event(&mut self, agent: &str, kind: ScionEventKind, summary: String) {
        self.event_seq += 1;
        self.events.push_back(ScionEvent {
            seq: self.event_seq,
            agent: agent.to_string(),
            kind,
            summary,
        });
        while self.events.len() > MAX_EVENTS {
            self.events.pop_front();
        }
    }

    /// The retained activity feed, oldest first.
    pub fn events(&self) -> &VecDeque<ScionEvent> {
        &self.events
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

    pub fn templates(&self) -> &[TemplateInfo] {
        &self.templates
    }

    fn fetch_templates(&mut self, cx: &mut Context<Self>) {
        let task = Tokio::spawn_result(cx, {
            let cli = self.cli.clone();
            async move { cli.list_templates().await }
        });

        cx.spawn(async move |this, cx| {
            if let Ok(templates) = task.await {
                this.update(cx, |store, cx| {
                    store.templates = templates;
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
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
            // PaddleBoard: opt-in. Don't poll or register Scion unless enabled in settings.
            if !paddleboard_scion_settings::ScionSettings::get_global(cx).enabled {
                return;
            }
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
                move |workspace, _: &paddleboard_actions::scion::StartAgent, window, cx| {
                    handle_start_agent(workspace, window, &store, cx);
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
    window: &mut Window,
    store: &Entity<ScionStore>,
    cx: &mut Context<Workspace>,
) {
    if !store.read(cx).is_available() {
        workspace.show_error(anyhow::anyhow!(
                "Scion is not installed. Use \"Install Scion in Terminal\" in the \
                 orchestration panel's Scion Agents section, or run: \
                 go install github.com/GoogleCloudPlatform/scion/cmd/scion@latest"
            ),
            cx,
        );
        return;
    }

    StartAgentModal::toggle(store.clone(), workspace, window, cx);
}

fn handle_stop_agent(
    workspace: &mut Workspace,
    store: &Entity<ScionStore>,
    cx: &mut Context<Workspace>,
) {
    let running = store.read(cx).running_agents();
    let Some(first) = running.first() else {
        workspace.show_error(anyhow::anyhow!("No running Scion agents to stop."), cx);
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
                    workspace.show_error(err, cx);
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
        workspace.show_error(anyhow::anyhow!("No Scion agents to sync from."), cx);
        return;
    };

    let name = first.name.clone();
    let task = store.update(cx, |store, cx| store.sync_from(name.clone(), cx));

    let store = store.clone();
    cx.spawn(async move |this, cx| {
        match task.await {
            Ok(_output) => {
                store.update(cx, |store, cx| store.refresh(cx));
                this.update(cx, |workspace, cx| {
                    workspace.show_toast(
                        Toast::new(
                            NotificationId::unique::<ScionStore>(),
                            format!("Synced changes from {name}"),
                        )
                        .autohide(),
                        cx,
                    );
                })
                .ok();
            }
            Err(err) => {
                this.update(cx, |workspace, cx| {
                    workspace.show_error(err, cx);
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
        workspace.show_error(anyhow::anyhow!("No Scion agents to show logs for."), cx);
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
                    workspace.show_error(err, cx);
                })
                .ok();
            }
        }
    })
    .detach();
}
