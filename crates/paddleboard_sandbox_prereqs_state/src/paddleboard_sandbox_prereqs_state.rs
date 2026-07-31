//! GPUI `Global` that caches the latest sandbox-prereqs probe result and
//! schedules background refreshes. Lives in its own crate so non-UI callers
//! (the agent tool layer, `project::context_server_store`) can read the cached
//! status without taking a dependency on the heavyweight `workspace` crate.

use gpui::{App, BorrowAppContext, actions};
use gpui_tokio::Tokio;
use paddleboard_sandbox_prereqs::SandboxStatus;

actions!(
    paddleboard,
    [
        /// Opens the sandbox prerequisites status modal.
        OpenSandboxPrereqs
    ]
);

#[derive(Default)]
pub struct SandboxPrereqs {
    status: Option<SandboxStatus>,
    refreshing: bool,
}

impl gpui::Global for SandboxPrereqs {}

impl SandboxPrereqs {
    /// Register the Global and kick off the first probe.
    pub fn init(cx: &mut App) {
        cx.set_global(SandboxPrereqs::default());
        Self::refresh(cx);
    }

    // `try_global` rather than `global`: these are read from render paths (the
    // status bar item, the agent's sandbox tools), and `global` PANICS when the
    // Global has not been set. `init` runs from `paddleboard_sandbox_prereqs_ui`,
    // so any context that renders before — or without — that init would take down
    // the app. Degrading to "no status yet" is the correct answer there, and it is
    // what the callers already handle. This was masking 18 test failures.
    pub fn status(cx: &App) -> Option<&SandboxStatus> {
        cx.try_global::<SandboxPrereqs>()?.status.as_ref()
    }

    pub fn is_refreshing(cx: &App) -> bool {
        cx.try_global::<SandboxPrereqs>().is_some_and(|prereqs| prereqs.refreshing)
    }

    /// Run a fresh probe in the background and update the cached status. Any
    /// observer registered against the `SandboxPrereqs` global is notified
    /// once the probe completes.
    pub fn refresh(cx: &mut App) {
        cx.update_global::<SandboxPrereqs, _>(|prereqs, _| {
            prereqs.refreshing = true;
        });

        let task = Tokio::spawn(cx, async { paddleboard_sandbox_prereqs::check().await });

        cx.spawn(async move |cx| {
            let status = task.await.ok();
            cx.update(|cx| {
                cx.update_global::<SandboxPrereqs, _>(|prereqs, _| {
                    if let Some(status) = status {
                        prereqs.status = Some(status);
                    }
                    prereqs.refreshing = false;
                });
            });
        })
        .detach();
    }
}
