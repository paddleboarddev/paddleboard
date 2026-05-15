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

    pub fn status(cx: &App) -> Option<&SandboxStatus> {
        cx.global::<SandboxPrereqs>().status.as_ref()
    }

    pub fn is_refreshing(cx: &App) -> bool {
        cx.global::<SandboxPrereqs>().refreshing
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
