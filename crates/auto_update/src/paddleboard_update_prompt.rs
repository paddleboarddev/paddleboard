// PaddleBoard: the manual "Check for Updates" flow.
//
// Upstream's `Check` action calls `poll`, which checks, downloads and installs
// in one uninterruptible run. That is the right shape for the hourly background
// poll and the wrong shape for a menu item: the user asked a question ("is
// there an update?") and the answer was ~160 MB of network traffic and a staged
// install they never agreed to — or, when already current, no answer at all.
//
// So the manual path checks first, reports what it found, and only then asks.
// All of it lives here rather than inside `auto_update.rs` so an upstream merge
// has a single call site to reconcile instead of a rewritten `check`. Being a
// child module of the crate root is load-bearing: it lets this reuse
// `get_release_asset` and `check_if_fetched_version_is_newer` directly, so the
// question this asks and the download `poll` performs can never disagree about
// what counts as newer.

use anyhow::Result;
use gpui::{
    AnyWindowHandle, App, AppContext as _, AsyncApp, Entity, PromptButton, PromptLevel,
    TaskExt as _,
};
use release_channel::{AppCommitSha, ReleaseChannel};
use semver::Version;
use std::env::consts::{ARCH, OS};

use crate::{AutoUpdateStatus, AutoUpdater, UpdateCheckType};

/// Check for a newer release, tell the user what was found, and download only
/// if they say so. Called from `auto_update::check`.
pub(crate) fn check_and_prompt(
    updater: Entity<AutoUpdater>,
    window: AnyWindowHandle,
    cx: &mut App,
) {
    let installed_version = updater.read(cx).current_version();

    // A background poll may already be mid-flight. Starting a second check would
    // overwrite its status — the download would carry on with the status bar
    // showing nothing — so report what's happening and stay out of its way.
    let already_running = match updater.read(cx).status() {
        AutoUpdateStatus::Checking => {
            Some("PaddleBoard is already checking for updates.".to_string())
        }
        AutoUpdateStatus::Downloading { version, .. } => {
            Some(format!("PaddleBoard {version} is already downloading."))
        }
        AutoUpdateStatus::Installing { version } => {
            Some(format!("PaddleBoard {version} is already installing."))
        }
        AutoUpdateStatus::Idle
        | AutoUpdateStatus::Updated { .. }
        | AutoUpdateStatus::Errored { .. } => None,
    };

    cx.spawn(async move |cx| {
        if let Some(detail) = already_running {
            prompt(
                window,
                PromptLevel::Info,
                "An update is already under way".to_string(),
                detail,
                &[PromptButton::ok("OK")],
                cx,
            )
            .await?;
            return anyhow::Ok(());
        }

        let found = newer_version(&updater, cx).await;

        match found {
            Err(error) => {
                // The status bar deliberately stays silent about update errors,
                // so a check the user asked for has nowhere else to report a
                // failure — offline, rate-limited, or no build for this platform.
                prompt(
                    window,
                    PromptLevel::Warning,
                    "Couldn't check for updates".to_string(),
                    format!("{error:#}"),
                    &[PromptButton::ok("OK")],
                    cx,
                )
                .await?;
            }

            Ok(None) => {
                // A background poll may already have staged an install. Saying
                // "you're up to date" then would be a lie the user can act on
                // only by restarting — so say that instead.
                let staged = updater.read_with(cx, |updater, _| match updater.status() {
                    AutoUpdateStatus::Updated { version } => Some(version),
                    _ => None,
                });

                match staged {
                    Some(version) => {
                        let answer = prompt(
                            window,
                            PromptLevel::Info,
                            format!("PaddleBoard {version} is ready to install"),
                            "It's already downloaded. Restarting finishes the update.".to_string(),
                            &[
                                PromptButton::ok("Restart Now"),
                                PromptButton::cancel("Later"),
                            ],
                            cx,
                        )
                        .await?;
                        if answer == 0 {
                            cx.update(|cx| workspace::reload(cx));
                        }
                    }
                    None => {
                        prompt(
                            window,
                            PromptLevel::Info,
                            "PaddleBoard is up to date".to_string(),
                            format!("You're running {installed_version}, the latest release."),
                            &[PromptButton::ok("OK")],
                            cx,
                        )
                        .await?;
                    }
                }
            }

            Ok(Some(version)) => {
                loop {
                    let answer = prompt(
                        window,
                        PromptLevel::Info,
                        format!("PaddleBoard {version} is available"),
                        format!(
                            "You're running {installed_version}. \
                             Updating downloads the new version in the background; \
                             it takes effect the next time you restart PaddleBoard."
                        ),
                        &[
                            PromptButton::ok("Update Now"),
                            PromptButton::new("Release Notes"),
                            PromptButton::cancel("Not Now"),
                        ],
                        cx,
                    )
                    .await?;

                    match answer {
                        0 => {
                            // Consent given: run the normal pipeline, which
                            // re-fetches and then downloads. One extra API call
                            // buys leaving `update` untouched.
                            updater.update(cx, |updater, cx| {
                                updater.poll(UpdateCheckType::Manual, cx)
                            });
                            break;
                        }
                        // Release notes open in a browser, so the question is
                        // still on the table — ask it again rather than making
                        // the user run the check a second time.
                        1 => {
                            if let Some(url) = cx.update(crate::release_notes_url) {
                                cx.update(|cx| cx.open_url(&url));
                            }
                        }
                        _ => break,
                    }
                }
            }
        }

        anyhow::Ok(())
    })
    .detach_and_log_err(cx);
}

/// The check half of `AutoUpdater::update`: fetch the release PaddleBoard would
/// install and report whether it is newer than what is running. Downloads
/// nothing.
async fn newer_version(
    updater: &Entity<AutoUpdater>,
    cx: &mut AsyncApp,
) -> Result<Option<Version>> {
    AutoUpdater::check_dependencies()?;

    let (installed_version, previous_status, release_channel) =
        updater.read_with(cx, |updater, cx| {
            (
                updater.current_version(),
                updater.status(),
                ReleaseChannel::try_global(cx).unwrap_or(ReleaseChannel::Stable),
            )
        });

    updater.update(cx, |updater, cx| {
        updater.status = AutoUpdateStatus::Checking;
        cx.notify();
    });

    let fetched =
        AutoUpdater::get_release_asset(updater, release_channel, None, "zed", OS, ARCH, cx).await;

    // Whatever the fetch did, the status-bar spinner has to stop: this pass
    // installs nothing, so leaving `Checking` up would strand it there until
    // the next hourly poll. A staged update outranks Idle — it still needs a
    // restart and the status bar is the only place that says so.
    updater.update(cx, |updater, cx| {
        updater.status = match &previous_status {
            AutoUpdateStatus::Updated { .. } => previous_status.clone(),
            _ => AutoUpdateStatus::Idle,
        };
        cx.notify();
    });

    let app_commit_sha = Ok(cx.update(|cx| AppCommitSha::try_global(cx).map(|sha| sha.full())));

    AutoUpdater::check_if_fetched_version_is_newer(
        release_channel,
        app_commit_sha,
        installed_version,
        fetched?.version,
        previous_status,
    )
}

async fn prompt(
    window: AnyWindowHandle,
    level: PromptLevel,
    message: String,
    detail: String,
    answers: &[PromptButton],
    cx: &mut AsyncApp,
) -> Result<usize> {
    let answer = cx.update_window(window, |_, window, cx| {
        window.prompt(level, &message, Some(&detail), answers, cx)
    })?;

    Ok(answer.await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use client::Client;
    use clock::FakeSystemClock;
    use gpui::TestAppContext;
    use http_client::{FakeHttpClient, Response};
    use settings::{SettingsStore, default_settings};
    use std::sync::Arc;

    /// A releases payload shaped like GitHub's, carrying both platform assets so
    /// the fixture works on macOS and on CI's Linux.
    fn releases_body(tags: &[&str]) -> String {
        let releases = tags
            .iter()
            .map(|tag| {
                format!(
                    r#"{{"tag_name":"{tag}","draft":false,"prerelease":false,"assets":[
                         {{"name":"PaddleBoard-aarch64.dmg","browser_download_url":"https://test.example/{tag}"}},
                         {{"name":"paddleboard-linux-x86_64.tar.gz","browser_download_url":"https://test.example/{tag}"}}
                       ]}}"#
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("[{releases}]")
    }

    /// Build an updater that is NOT wired to `crate::init`, so no background poll
    /// competes with the check under test.
    fn updater_for(
        installed: Version,
        published: &'static [&'static str],
        cx: &mut TestAppContext,
    ) -> (Entity<AutoUpdater>, Arc<std::sync::atomic::AtomicUsize>) {
        let asset_requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let updater = cx.update(|cx| {
            let mut store = SettingsStore::new(cx, &settings::default_settings());
            store
                .set_default_settings(&default_settings(), cx)
                .expect("Unable to set default settings");
            store
                .set_user_settings("{}", cx)
                .expect("Unable to set user settings");
            cx.set_global(store);
            release_channel::init_test(installed.clone(), ReleaseChannel::Stable, cx);

            let asset_requests = asset_requests.clone();
            let http = FakeHttpClient::create(move |request| {
                let asset_requests = asset_requests.clone();
                async move {
                    if request.uri().path() == "/repos/paddleboarddev/paddleboard/releases" {
                        return Ok(Response::builder()
                            .status(200)
                            .body(releases_body(published).into())
                            .unwrap());
                    }
                    // Any other path means something tried to fetch the build
                    // itself — exactly what a check must not do.
                    asset_requests.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    Ok(Response::builder().status(200).body("dmg".into()).unwrap())
                }
            });
            let client = Client::new(Arc::new(FakeSystemClock::new()), http, cx);
            cx.new(|cx| AutoUpdater::new(installed, client, cx))
        });

        (updater, asset_requests)
    }

    #[gpui::test]
    async fn check_reports_a_newer_release_without_downloading_it(cx: &mut TestAppContext) {
        let (updater, asset_requests) =
            updater_for(Version::new(0, 2, 4), &["v0.3.0", "v0.2.4"], cx);

        let found = newer_version(&updater, &mut cx.to_async())
            .await
            .expect("the check should succeed");

        assert_eq!(found, Some(Version::new(0, 3, 0)));
        // The whole point of the manual flow: nothing moves until the user says so.
        assert_eq!(asset_requests.load(std::sync::atomic::Ordering::Relaxed), 0);
        updater.read_with(cx, |updater, _| {
            assert_eq!(updater.status(), AutoUpdateStatus::Idle)
        });
    }

    #[gpui::test]
    async fn check_reports_nothing_when_already_current(cx: &mut TestAppContext) {
        let (updater, asset_requests) = updater_for(Version::new(0, 2, 4), &["v0.2.4"], cx);

        let found = newer_version(&updater, &mut cx.to_async())
            .await
            .expect("the check should succeed");

        assert_eq!(found, None);
        assert_eq!(asset_requests.load(std::sync::atomic::Ordering::Relaxed), 0);
        updater.read_with(cx, |updater, _| {
            assert_eq!(updater.status(), AutoUpdateStatus::Idle)
        });
    }

    /// A check run while an update is already staged must leave that state alone —
    /// the status bar's "Restart to update" button is the only thing telling the
    /// user the new build is waiting.
    #[gpui::test]
    async fn check_preserves_a_staged_update(cx: &mut TestAppContext) {
        let staged = Version::new(0, 3, 0);
        let (updater, _) = updater_for(Version::new(0, 2, 4), &["v0.3.0", "v0.2.4"], cx);
        updater.update(cx, |updater, _| {
            updater.status = AutoUpdateStatus::Updated {
                version: staged.clone(),
            };
        });

        let found = newer_version(&updater, &mut cx.to_async())
            .await
            .expect("the check should succeed");

        assert_eq!(found, None, "0.3.0 is already staged, so nothing is newer");
        updater.read_with(cx, |updater, _| {
            assert_eq!(
                updater.status(),
                AutoUpdateStatus::Updated { version: staged }
            )
        });
    }
}
