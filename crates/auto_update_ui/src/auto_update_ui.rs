use std::sync::Arc;

use agent_skills::GLOBAL_SKILLS_DIR_DISPLAY;
use auto_update::{AutoUpdater, release_notes_url};
use client::zed_urls;
use db::kvp::Dismissable;
<<<<<<< HEAD
use fs::Fs;
=======
use editor::{Editor, MultiBuffer};
>>>>>>> zed/main
use gpui::{
    App, DismissEvent, EventEmitter, FocusHandle, Focusable, TaskExt, Window, actions, prelude::*,
};
<<<<<<< HEAD
use notifications::status_toast::StatusToast;
use release_channel::ReleaseChannel;
use semver::Version;
use settings::Settings as _;
use ui::{AnnouncementToast, ListBulletItem, ParallelAgentsIllustration, prelude::*};
=======
use markdown_preview::markdown_preview_view::{MarkdownPreviewMode, MarkdownPreviewView};
use prompt_store::rules_to_skills_migration;
use release_channel::{AppVersion, ReleaseChannel};
use semver::Version;
use serde::Deserialize;
use smol::io::AsyncReadExt;
use ui::{AnnouncementToast, ListBulletItem, SkillsIllustration, prelude::*};
use util::{ResultExt as _, maybe};
>>>>>>> zed/main
use workspace::{
    Workspace,
    notifications::{
        Notification, NotificationId, SuppressEvent, show_app_notification,
        simple_message_notification::MessageNotification,
    },
};
<<<<<<< HEAD
use paddleboard_actions::{ShowUpdateNotification, assistant::FocusAgent};
=======
use zed_actions::ShowUpdateNotification;
>>>>>>> zed/main

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
    secondary_action_label: SharedString,
    primary_action_url: Option<SharedString>,
    primary_action_callback: Option<Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>>,
    secondary_action_url: Option<SharedString>,
    on_dismiss: Option<Arc<dyn Fn(&mut App) + Send + Sync>>,
}

struct SkillsAnnouncement;

impl Dismissable for SkillsAnnouncement {
    const KEY: &'static str = "skills_announcement_dismissed";
}

fn announcement_for_version(version: &Version, cx: &App) -> Option<AnnouncementContent> {
    let version_with_skills = match ReleaseChannel::global(cx) {
        ReleaseChannel::Stable => Version::new(1, 4, 0),
        ReleaseChannel::Dev | ReleaseChannel::Nightly | ReleaseChannel::Preview => {
            Version::new(1, 4, 0)
        }
    };

    if *version >= version_with_skills && !SkillsAnnouncement::dismissed(cx) {
        // Only mention the Rules → Skills migration if the user actually
        // had Rules that got migrated. New users (and existing users who
        // never created a Rule) would otherwise be confused by a bullet
        // referring to "your rules" that don't exist.
        let migrated_anything =
            rules_to_skills_migration::migration_result().is_some_and(|result| !result.is_empty());

        let mut bullet_items: Vec<SharedString> = Vec::with_capacity(3);
        bullet_items
            .push(format!("Skills live in {GLOBAL_SKILLS_DIR_DISPLAY}/<name>/SKILL.md").into());
        if migrated_anything {
            bullet_items.push(
                "Default Rules are converted into your global AGENTS.md; all other rules become skills".into(),
            );
        }
        bullet_items.push("Type / to manually invoke a skill".into());

        Some(AnnouncementContent {
            heading: "Introducing Skills Support".into(),
            description: "Extend the agent with focused instructions and domain knowledge.".into(),
            bullet_items,
            primary_action_label: "Try Now".into(),
            secondary_action_label: "Read Documentation".into(),
            primary_action_url: None,
            primary_action_callback: Some(Arc::new(move |window, cx| {
                window.dispatch_action(Box::new(zed_actions::assistant::FocusAgent), cx);
            })),
<<<<<<< HEAD
            on_dismiss: Some(Arc::new(|cx| {
                ParallelAgentAnnouncement::set_dismissed(true, cx)
            })),
            // PaddleBoard: upstream points "Learn More" at zed.dev/blog. PaddleBoard ships its own
            // parallel-agents/multi-workspace docs in the repo README, so route there instead.
            secondary_action_url: Some("https://github.com/jasonsmithio/paddleboard".into()),
=======
            on_dismiss: Some(Arc::new(|cx| SkillsAnnouncement::set_dismissed(true, cx))),
            secondary_action_url: Some(zed_urls::skills_docs(cx).into()),
>>>>>>> zed/main
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
            .illustration(SkillsIllustration::new())
            .heading(self.content.heading.clone())
            .description(self.content.description.clone())
            .bullet_items(
                self.content
                    .bullet_items
                    .iter()
                    .map(|item| ListBulletItem::new(item.clone())),
            )
            .primary_action_label(self.content.primary_action_label.clone())
            .secondary_action_label(self.content.secondary_action_label.clone())
            .primary_on_click(cx.listener({
                let url = self.content.primary_action_url.clone();
                let callback = self.content.primary_action_callback.clone();
                move |this, _, window, cx| {
                    telemetry::event!("Skills Announcement Main Click");
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
                    telemetry::event!("Skills Announcement Secondary Click");
                    if let Some(url) = &url {
                        cx.open_url(url);
                    }
                }
            }))
            .dismiss_on_click(cx.listener(|this, _, _window, cx| {
                telemetry::event!("Skills Announcement Dismiss");
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
