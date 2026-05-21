use std::sync::Arc;

use agent_settings::{AgentSettings, WindowLayout};
use auto_update::{AutoUpdater, release_notes_url};
use db::kvp::Dismissable;
use fs::Fs;
use gpui::{
    App, DismissEvent, EventEmitter, FocusHandle, Focusable, TaskExt, Window, actions, prelude::*,
};
use notifications::status_toast::StatusToast;
use release_channel::ReleaseChannel;
use semver::Version;
use settings::Settings as _;
use ui::{AnnouncementToast, ListBulletItem, ParallelAgentsIllustration, prelude::*};
use workspace::{
    FocusWorkspaceSidebar, Workspace,
    notifications::{
        Notification, NotificationId, SuppressEvent, show_app_notification,
        simple_message_notification::MessageNotification,
    },
};
use paddleboard_actions::{ShowUpdateNotification, assistant::FocusAgent};

actions!(
    auto_update,
    [
        /// Opens the release notes for the current version in a new tab.
        ViewReleaseNotesLocally
    ]
);

pub fn init(cx: &mut App) {
    // PaddleBoard: skip `notify_if_app_was_updated`. Upstream surfaces a "you updated to vX" toast
    // driven by a KV-store flag set during the install step; we never install through this path,
    // and we don't want a stale or externally-set flag to surface a phantom toast.
    cx.observe_new(|workspace: &mut Workspace, _window, cx| {
        workspace.register_action(|workspace, _: &ViewReleaseNotesLocally, window, cx| {
            view_release_notes_locally(workspace, window, cx);
        });

        if matches!(
            ReleaseChannel::global(cx),
            ReleaseChannel::Nightly | ReleaseChannel::Dev
        ) {
            workspace.register_action(|_workspace, _: &ShowUpdateNotification, _window, cx| {
                show_update_notification(cx);
            });
        }
    })
    .detach();
}

// PaddleBoard: `ReleaseNotesBody` (a serde DTO for zed.dev's /api/release_notes/v2 response)
// and `notify_release_notes_failed_to_show` (the error-toast for that fetch) were dropped
// alongside the inline-render path in `view_release_notes_locally`.
fn view_release_notes_locally(
    _workspace: &mut Workspace,
    _window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    // PaddleBoard: the upstream Stable/Preview path fetched release notes from zed.dev and rendered
    // them inline as a markdown buffer. We don't run that API, so collapse every channel to "open
    // the PaddleBoard releases page in a browser" — the same URL `release_notes_url` now returns.
    if let Some(url) = release_notes_url(cx) {
        cx.open_url(&url);
    }
}

#[derive(Clone)]
struct AnnouncementContent {
    heading: SharedString,
    description: SharedString,
    bullet_items: Vec<SharedString>,
    primary_action_label: SharedString,
    primary_action_url: Option<SharedString>,
    primary_action_callback: Option<Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>>,
    secondary_action_url: Option<SharedString>,
    on_dismiss: Option<Arc<dyn Fn(&mut App) + Send + Sync>>,
}

struct ParallelAgentAnnouncement;

impl Dismissable for ParallelAgentAnnouncement {
    const KEY: &'static str = "parallel-agent-announcement";
}

fn announcement_for_version(version: &Version, cx: &App) -> Option<AnnouncementContent> {
    let version_with_parallel_agents = match ReleaseChannel::global(cx) {
        ReleaseChannel::Stable => Version::new(0, 233, 0),
        ReleaseChannel::Dev | ReleaseChannel::Nightly | ReleaseChannel::Preview => {
            Version::new(0, 232, 0)
        }
    };

    if *version >= version_with_parallel_agents
        && !ParallelAgentAnnouncement::dismissed(cx)
        && !project::DisableAiSettings::get_global(cx).disable_ai
    {
        let fs = <dyn Fs>::global(cx);
        Some(AnnouncementContent {
            heading: "Introducing Parallel Agents".into(),
            description: "Run multiple threads of your favorite agents simultaneously across projects in a new workspace layout, tailored for agentic workflows.".into(),
            bullet_items: vec![
                "Use your favorite agents in parallel".into(),
                "Optionally isolate agents using worktrees".into(),
                "Combine multiple projects in one window".into(),
            ],
            primary_action_label: "Try Agentic Layout".into(),
            primary_action_url: None,
            primary_action_callback: Some(Arc::new(move |window, cx| {
                let get_layout = AgentSettings::get_layout(cx);
                let already_agent_layout = matches!(get_layout, WindowLayout::Agent(_));

                let update;
                if !already_agent_layout {
                    update = Some(AgentSettings::set_layout(
                        WindowLayout::Agent(None),
                        fs.clone(),
                        cx,
                    ));
                } else {
                    update = None;
                }

                let revert_fs = fs.clone();
                window
                    .spawn(cx, async move |cx| {
                        if let Some(update) = update {
                            update.await.ok();
                        }

                        cx.update(|window, cx| {
                            if !already_agent_layout {
                                if let Some(workspace) = Workspace::for_window(window, cx) {
                                    let toast = StatusToast::new(
                                        "You are in the new agentic layout!",
                                        cx,
                                        move |this, _cx| {
                                            this.icon(
                                                Icon::new(IconName::Check)
                                                    .size(IconSize::Small)
                                                    .color(Color::Success),
                                            )
                                            .action("Revert", move |_window, cx| {
                                                let _ = AgentSettings::set_layout(
                                                    get_layout.clone(),
                                                    revert_fs.clone(),
                                                    cx,
                                                );
                                            })
                                            .auto_dismiss(false)
                                            .dismiss_button(true)
                                        },
                                    );

                                    workspace.update(cx, |workspace, cx| {
                                        workspace.toggle_status_toast(toast, cx);
                                    });
                                }
                            }

                            window.dispatch_action(Box::new(FocusWorkspaceSidebar), cx);
                            window.dispatch_action(Box::new(FocusAgent), cx);
                        })
                    })
                    .detach();
            })),
            on_dismiss: Some(Arc::new(|cx| {
                ParallelAgentAnnouncement::set_dismissed(true, cx)
            })),
            secondary_action_url: Some("https://zed.dev/blog/".into()),
        })
    } else {
        None
    }
}

struct AnnouncementToastNotification {
    focus_handle: FocusHandle,
    content: AnnouncementContent,
}

impl AnnouncementToastNotification {
    fn new(content: AnnouncementContent, cx: &mut App) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content,
        }
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
        if let Some(on_dismiss) = &self.content.on_dismiss {
            on_dismiss(cx);
        }
    }
}

impl Focusable for AnnouncementToastNotification {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<DismissEvent> for AnnouncementToastNotification {}
impl EventEmitter<SuppressEvent> for AnnouncementToastNotification {}
impl Notification for AnnouncementToastNotification {}

impl Render for AnnouncementToastNotification {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        AnnouncementToast::new()
            .illustration(ParallelAgentsIllustration::new())
            .heading(self.content.heading.clone())
            .description(self.content.description.clone())
            .bullet_items(
                self.content
                    .bullet_items
                    .iter()
                    .map(|item| ListBulletItem::new(item.clone())),
            )
            .primary_action_label(self.content.primary_action_label.clone())
            .primary_on_click(cx.listener({
                let url = self.content.primary_action_url.clone();
                let callback = self.content.primary_action_callback.clone();
                move |this, _, window, cx| {
                    telemetry::event!("Parallel Agent Announcement Main Click");
                    if let Some(callback) = &callback {
                        callback(window, cx);
                    }
                    if let Some(url) = &url {
                        cx.open_url(url);
                    }
                    this.dismiss(cx);
                }
            }))
            .secondary_on_click(cx.listener({
                let url = self.content.secondary_action_url.clone();
                move |_, _, _window, cx| {
                    telemetry::event!("Parallel Agent Announcement Secondary Click");
                    if let Some(url) = &url {
                        cx.open_url(url);
                    }
                }
            }))
            .dismiss_on_click(cx.listener(|this, _, _window, cx| {
                telemetry::event!("Parallel Agent Announcement Dismiss");
                this.dismiss(cx);
            }))
    }
}

struct UpdateNotification;

fn show_update_notification(cx: &mut App) {
    let Some(updater) = AutoUpdater::get(cx) else {
        return;
    };

    let mut version = updater.read(cx).current_version();
    version.pre = semver::Prerelease::EMPTY;
    version.build = semver::BuildMetadata::EMPTY;
    let app_name = ReleaseChannel::global(cx).display_name();

    if let Some(content) = announcement_for_version(&version, cx) {
        show_app_notification(
            NotificationId::unique::<UpdateNotification>(),
            cx,
            move |cx| cx.new(|cx| AnnouncementToastNotification::new(content.clone(), cx)),
        );
    } else {
        show_app_notification(
            NotificationId::unique::<UpdateNotification>(),
            cx,
            move |cx| {
                let workspace_handle = cx.entity().downgrade();
                cx.new(|cx| {
                    MessageNotification::new(format!("Updated to {app_name} {}", version), cx)
                        .primary_message("View Release Notes")
                        .primary_on_click(move |window, cx| {
                            if let Some(workspace) = workspace_handle.upgrade() {
                                workspace.update(cx, |workspace, cx| {
                                    crate::view_release_notes_locally(workspace, window, cx);
                                })
                            }
                            cx.emit(DismissEvent);
                        })
                        .show_suppress_button(false)
                })
            },
        );
    }
}

/// Shows a notification across all workspaces if an update was previously automatically installed
/// and this notification had not yet been shown.
pub fn notify_if_app_was_updated(cx: &mut App) {
    let Some(updater) = AutoUpdater::get(cx) else {
        return;
    };

    if let ReleaseChannel::Nightly = ReleaseChannel::global(cx) {
        return;
    }

    let should_show_notification = updater.read(cx).should_show_update_notification(cx);

    cx.spawn(async move |cx| {
        let should_show_notification = should_show_notification.await?;

        if should_show_notification {
            cx.update(|cx| {
                show_update_notification(cx);
                updater.update(cx, |updater, cx| {
                    updater
                        .set_should_show_update_notification(false, cx)
                        .detach_and_log_err(cx);
                });
            });
        }
        anyhow::Ok(())
    })
    .detach();
}
