// PaddleBoard: release discovery for in-app updates.
//
// Upstream asks zed.dev for "the release for this channel"; the server decides
// which build that is. PaddleBoard has no such server — releases are GitHub
// Releases on paddleboarddev/paddleboard — so the choice has to be made here.
//
// The whole reason this is its own module rather than an edit inside
// `auto_update.rs` is the selection rule below. It is the part most likely to
// need changing, it is the part worth unit-testing, and keeping it out of an
// upstream-shaped file means an upstream merge cannot conflict with it.

use anyhow::{Context as _, Result};
use http_client::{HttpClient, HttpRequestExt as _, RedirectPolicy, http::Request};
use semver::Version;
use serde::Deserialize;
use smol::io::AsyncReadExt as _;
use std::sync::Arc;

use crate::ReleaseAsset;

/// The repository whose Releases are the update source. This is the *public*
/// repo — the private dev repo has no published binaries.
pub const RELEASES_REPO: &str = "paddleboarddev/paddleboard";

const GITHUB_API_URL: &str = "https://api.github.com";

/// One page is plenty: releases come back newest-first, and an install more
/// than 30 releases behind is better served by a fresh download than by
/// walking pagination on every hourly poll.
const RELEASES_PER_PAGE: usize = 30;

#[derive(Debug, Deserialize)]
pub struct GithubRelease {
    pub tag_name: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Vec<GithubReleaseAsset>,
}

#[derive(Debug, Deserialize)]
pub struct GithubReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
}

/// The asset filename `release.yml` publishes for a given platform.
///
/// Returning `None` means "PaddleBoard does not ship a build you can install",
/// which is a normal answer, not a failure: the macOS build is Apple-silicon
/// only and the Linux build is x86_64 only.
///
/// Note there is no release *channel* in this lookup, only platform. PaddleBoard
/// publishes a single release line — `release.yml` forces the stable channel —
/// so there is nothing for a channel to select between. The macOS install path
/// depends on this: it rsyncs out of the mounted volume using the *running*
/// app's bundle name, and the DMG contains `PaddleBoard.app`. A Preview or
/// Nightly bundle would therefore fail to install; neither is built today, and
/// the failure would be a clear rsync error rather than a damaged install.
pub fn asset_name(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("macos", "aarch64") => Some("PaddleBoard-aarch64.dmg"),
        ("linux", "x86_64") => Some("paddleboard-linux-x86_64.tar.gz"),
        _ => None,
    }
}

/// Parse a release tag (`v0.3.0`) into a version. Tags that aren't versions are
/// skipped rather than failing the whole update check.
fn version_from_tag(tag: &str) -> Option<Version> {
    Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()
}

/// Pick the release to offer, given every release the API returned.
///
/// ⚠️ This deliberately does NOT use GitHub's `releases/latest`, which is the
/// obvious implementation and is wrong here. `latest` follows the `make_latest`
/// flag, which GitHub only computes when a *non-prerelease release is created*.
/// PaddleBoard's pipeline creates every release as a prerelease and promotes it
/// afterwards, so that flag does not move on promotion — it once left
/// `releases/latest` pointing at v0.1.17 for 13 days while two newer releases
/// were published. An updater built on it would have offered every user a build
/// two versions behind, silently and forever.
///
/// So: take the highest semver among releases that are actually installable.
/// Highest-semver rather than newest-by-date also means a re-published or
/// back-dated old release can never walk a user backwards.
pub fn select_release(
    releases: &[GithubRelease],
    include_prereleases: bool,
    os: &str,
    arch: &str,
) -> Option<ReleaseAsset> {
    let wanted_asset = asset_name(os, arch)?;

    releases
        .iter()
        // Drafts are unpublished. This is the go-live gate: a release is not
        // offered to anyone until a human publishes it.
        .filter(|release| !release.draft)
        .filter(|release| include_prereleases || !release.prerelease)
        .filter_map(|release| {
            let version = version_from_tag(&release.tag_name)?;
            let asset = release
                .assets
                .iter()
                .find(|asset| asset.name == wanted_asset)?;
            Some((version, asset))
        })
        .max_by(|(left, _), (right, _)| left.cmp(right))
        .map(|(version, asset)| ReleaseAsset {
            version: version.to_string(),
            url: asset.browser_download_url.clone(),
        })
}

/// Fetch the release PaddleBoard should update to, if this platform has one.
///
/// The caller decides whether it is actually newer than what is running —
/// `AutoUpdater::check_if_fetched_version_is_newer` owns that comparison.
pub async fn fetch_latest_release(
    http: Arc<dyn HttpClient>,
    include_prereleases: bool,
    os: &str,
    arch: &str,
) -> Result<ReleaseAsset> {
    anyhow::ensure!(
        asset_name(os, arch).is_some(),
        "PaddleBoard does not publish a build for {os} {arch}; \
         update from source or download a supported build"
    );

    let url = format!("{GITHUB_API_URL}/repos/{RELEASES_REPO}/releases?per_page={RELEASES_PER_PAGE}");

    // Unauthenticated is the norm here — 60 requests/hour against a 1/hour poll
    // is not close to the limit. GITHUB_TOKEN is honoured only because a rate
    // limit shared with other tooling on the same IP is otherwise invisible and
    // maddening to debug; it matches how the rest of the app talks to GitHub.
    let request = Request::get(&url)
        .header("Accept", "application/vnd.github+json")
        .follow_redirects(RedirectPolicy::FollowAll)
        .when_some(std::env::var("GITHUB_TOKEN").ok(), |builder, token| {
            builder.header("Authorization", format!("Bearer {token}"))
        })
        .body(Default::default())?;

    let mut response = http
        .send(request)
        .await
        .context("fetching PaddleBoard releases from GitHub")?;

    let mut body = Vec::new();
    response
        .body_mut()
        .read_to_end(&mut body)
        .await
        .context("reading the GitHub releases response")?;

    anyhow::ensure!(
        response.status().is_success(),
        "GitHub returned {} when listing releases: {:?}",
        response.status(),
        String::from_utf8_lossy(&body),
    );

    let releases: Vec<GithubRelease> = serde_json::from_slice(&body).with_context(|| {
        format!(
            "parsing the GitHub releases response: {:?}",
            String::from_utf8_lossy(&body)
        )
    })?;

    select_release(&releases, include_prereleases, os, arch).with_context(|| {
        format!(
            "no published release carries a {} asset",
            asset_name(os, arch).unwrap_or("supported")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, draft: bool, prerelease: bool, assets: &[&str]) -> GithubRelease {
        GithubRelease {
            tag_name: tag.to_string(),
            draft,
            prerelease,
            assets: assets
                .iter()
                .map(|name| GithubReleaseAsset {
                    name: name.to_string(),
                    browser_download_url: format!("https://example.invalid/{tag}/{name}"),
                })
                .collect(),
        }
    }

    const DMG: &str = "PaddleBoard-aarch64.dmg";
    const TARBALL: &str = "paddleboard-linux-x86_64.tar.gz";

    fn pick(releases: &[GithubRelease], include_prereleases: bool) -> Option<String> {
        select_release(releases, include_prereleases, "macos", "aarch64")
            .map(|asset| asset.version)
    }

    /// The regression this module exists for. These are the real flags from
    /// paddleboarddev/paddleboard: v0.1.18 is still marked prerelease even
    /// though v0.1.19 and v0.2.0 shipped after it, and GitHub's own
    /// `releases/latest` answered v0.1.17 for 13 days in this exact state.
    #[test]
    fn picks_highest_semver_not_whatever_github_calls_latest() {
        let releases = [
            release("v0.2.0", false, false, &[DMG]),
            release("v0.1.19", false, false, &[DMG]),
            release("v0.1.18", false, true, &[DMG]),
            release("v0.1.17", false, false, &[DMG]),
        ];

        assert_eq!(pick(&releases, false).as_deref(), Some("0.2.0"));
    }

    /// Ordering is by version, not by position in the response. A release
    /// re-published (or back-dated) out of order must not win.
    #[test]
    fn ordering_is_semver_not_api_order() {
        let releases = [
            release("v0.1.9", false, false, &[DMG]),
            release("v0.2.0", false, false, &[DMG]),
            release("v0.10.0", false, false, &[DMG]),
        ];

        assert_eq!(pick(&releases, false).as_deref(), Some("0.10.0"));
    }

    #[test]
    fn drafts_are_never_offered() {
        let releases = [
            release("v0.4.0", true, false, &[DMG]),
            release("v0.3.0", false, false, &[DMG]),
        ];

        assert_eq!(pick(&releases, false).as_deref(), Some("0.3.0"));
        // Even opting into prereleases must not reach an unpublished draft.
        assert_eq!(pick(&releases, true).as_deref(), Some("0.3.0"));
    }

    #[test]
    fn prereleases_are_opt_in() {
        let releases = [
            release("v0.3.0", false, true, &[DMG]),
            release("v0.2.0", false, false, &[DMG]),
        ];

        assert_eq!(pick(&releases, false).as_deref(), Some("0.2.0"));
        assert_eq!(pick(&releases, true).as_deref(), Some("0.3.0"));
    }

    /// A release whose build failed for one platform still has assets for the
    /// other. Offering it would download nothing installable.
    #[test]
    fn releases_without_this_platforms_asset_are_skipped() {
        let releases = [
            release("v0.3.0", false, false, &[TARBALL]),
            release("v0.2.0", false, false, &[DMG, TARBALL]),
        ];

        assert_eq!(pick(&releases, false).as_deref(), Some("0.2.0"));
        assert_eq!(
            select_release(&releases, false, "linux", "x86_64").map(|a| a.version),
            Some("0.3.0".to_string())
        );
    }

    #[test]
    fn asset_url_comes_from_the_matching_asset() {
        let releases = [release("v0.3.0", false, false, &[TARBALL, DMG])];

        let picked = select_release(&releases, false, "macos", "aarch64").unwrap();
        assert_eq!(picked.url, format!("https://example.invalid/v0.3.0/{DMG}"));
    }

    #[test]
    fn unversioned_tags_are_skipped_not_fatal() {
        let releases = [
            release("nightly", false, false, &[DMG]),
            release("v0.2.0", false, false, &[DMG]),
        ];

        assert_eq!(pick(&releases, false).as_deref(), Some("0.2.0"));
    }

    #[test]
    fn no_installable_release_is_none_not_a_panic() {
        assert_eq!(pick(&[], false), None);
        assert_eq!(pick(&[release("v0.3.0", true, false, &[DMG])], false), None);
    }

    #[test]
    fn unsupported_platforms_have_no_asset() {
        assert!(asset_name("macos", "aarch64").is_some());
        assert!(asset_name("linux", "x86_64").is_some());
        // Intel Macs, ARM Linux and Windows all build from source today.
        assert_eq!(asset_name("macos", "x86_64"), None);
        assert_eq!(asset_name("linux", "aarch64"), None);
        assert_eq!(asset_name("windows", "x86_64"), None);
        assert_eq!(
            select_release(
                &[release("v0.3.0", false, false, &[DMG])],
                false,
                "windows",
                "x86_64"
            )
            .is_none(),
            true
        );
    }
}
