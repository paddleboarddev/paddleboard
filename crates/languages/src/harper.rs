// PaddleBoard: Harper spell/grammar checker adapter.
//
// Harper (https://writewithharper.com, Automattic) is an offline, privacy-first
// grammar + spell checker written in Rust. We attach its language server,
// `harper-ls`, to prose languages (Markdown, git commit messages) so PaddleBoard
// surfaces spelling/grammar diagnostics and quick-fix code actions without any
// text leaving the machine.
//
// Distribution mirrors `TyLspAdapter`: prebuilt, platform-specific binaries are
// published per GitHub release as `harper-ls-<arch>-<os>.tar.gz` (a bare
// `harper-ls` executable). Lifecycle:
//   1. `check_if_user_installed` looks for `harper-ls` on $PATH (cargo / brew).
//   2. Otherwise `fetch_server_binary` downloads the matching release asset and
//      runs `harper-ls --stdio`.

use std::env::consts;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use futures::StreamExt;
use gpui::AsyncApp;
use http_client::github::{AssetKind, GitHubLspBinaryVersion, latest_github_release};
use http_client::github_download::{GithubBinaryMetadata, download_server_binary};
pub use language::*;
use lsp::{LanguageServerBinary, LanguageServerName, Uri};
use project::lsp_store::language_server_settings;
use serde_json::{Value, json};
use smol::fs;
use util::fs::{make_file_executable, remove_matching};
use util::{ResultExt, maybe, merge_json_value_into};

pub struct HarperLspAdapter;

#[cfg(target_os = "macos")]
impl HarperLspAdapter {
    const GITHUB_ASSET_KIND: AssetKind = AssetKind::TarGz;
    const OS_SERVER_NAME: &str = "apple-darwin";
}

#[cfg(target_os = "linux")]
impl HarperLspAdapter {
    const GITHUB_ASSET_KIND: AssetKind = AssetKind::TarGz;
    const OS_SERVER_NAME: &str = "unknown-linux-gnu";
}

#[cfg(target_os = "windows")]
impl HarperLspAdapter {
    const GITHUB_ASSET_KIND: AssetKind = AssetKind::Zip;
    const OS_SERVER_NAME: &str = "pc-windows-msvc";
}

impl HarperLspAdapter {
    const SERVER_NAME: LanguageServerName = LanguageServerName::new_static("harper-ls");

    fn binary_name() -> &'static str {
        if cfg!(target_os = "windows") {
            "harper-ls.exe"
        } else {
            "harper-ls"
        }
    }

    fn asset_name() -> String {
        let arch = match consts::ARCH {
            "x86" => "i686",
            other => other,
        };
        let suffix = match Self::GITHUB_ASSET_KIND {
            AssetKind::Zip => "zip",
            _ => "tar.gz",
        };
        format!("harper-ls-{arch}-{}.{suffix}", Self::OS_SERVER_NAME)
    }
}

impl LspInstaller for HarperLspAdapter {
    type BinaryVersion = GitHubLspBinaryVersion;

    async fn fetch_latest_server_version(
        &self,
        delegate: &Arc<dyn LspAdapterDelegate>,
        pre_release: bool,
        _: &mut AsyncApp,
    ) -> Result<GitHubLspBinaryVersion> {
        let release =
            latest_github_release("Automattic/harper", true, pre_release, delegate.http_client())
                .await?;
        let asset_name = Self::asset_name();
        let asset = release
            .assets
            .into_iter()
            .find(|asset| asset.name == asset_name)
            .with_context(|| {
                format!(
                    "no asset found matching {asset_name:?} on harper release {:?}",
                    release.tag_name
                )
            })?;
        Ok(GitHubLspBinaryVersion {
            name: release.tag_name,
            url: asset.browser_download_url,
            digest: asset.digest,
        })
    }

    async fn check_if_user_installed(
        &self,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: Option<Toolchain>,
        _: &AsyncApp,
    ) -> Option<LanguageServerBinary> {
        let path = delegate.which(Self::SERVER_NAME.as_ref()).await?;
        Some(LanguageServerBinary {
            path,
            arguments: vec!["--stdio".into()],
            env: None,
        })
    }

    fn fetch_server_binary(
        &self,
        version: GitHubLspBinaryVersion,
        container_dir: PathBuf,
        delegate: &Arc<dyn LspAdapterDelegate>,
    ) -> impl Send + Future<Output = Result<LanguageServerBinary>> + use<> {
        let delegate = delegate.clone();
        async move {
            let GitHubLspBinaryVersion {
                name,
                url,
                digest: expected_digest,
            } = version;
            // The tarball contains a bare `harper-ls` executable (no inner
            // directory), so extract into a per-version slot and use the
            // binary directly beneath it.
            let destination_path = container_dir.join(format!("harper-ls-{name}"));
            fs::create_dir_all(&destination_path).await?;
            let server_path = destination_path.join(Self::binary_name());

            let binary = LanguageServerBinary {
                path: server_path.clone(),
                env: None,
                arguments: vec!["--stdio".into()],
            };

            let metadata_path = destination_path.with_extension("metadata");
            if let Some(metadata) = GithubBinaryMetadata::read_from_file(&metadata_path).await.ok() {
                let validity_check = async || {
                    delegate
                        .try_exec(LanguageServerBinary {
                            path: server_path.clone(),
                            arguments: vec!["--version".into()],
                            env: None,
                        })
                        .await
                        .inspect_err(|err| {
                            log::warn!("Unable to run {server_path:?}, redownloading: {err:#}")
                        })
                };
                if let (Some(actual_digest), Some(expected_digest)) =
                    (&metadata.digest, &expected_digest)
                {
                    if actual_digest == expected_digest && validity_check().await.is_ok() {
                        return Ok(binary);
                    }
                } else if validity_check().await.is_ok() {
                    return Ok(binary);
                }
            }

            download_server_binary(
                &*delegate.http_client(),
                &url,
                expected_digest.as_deref(),
                &destination_path,
                Self::GITHUB_ASSET_KIND,
            )
            .await?;
            make_file_executable(&server_path).await?;
            remove_matching(&container_dir, |path| path != destination_path).await;
            GithubBinaryMetadata::write_to_file(
                &GithubBinaryMetadata {
                    metadata_version: 1,
                    digest: expected_digest,
                },
                &metadata_path,
            )
            .await?;

            Ok(binary)
        }
    }

    async fn cached_server_binary(
        &self,
        container_dir: PathBuf,
        _: &dyn LspAdapterDelegate,
    ) -> Option<LanguageServerBinary> {
        maybe!(async {
            let mut last_dir = None;
            let mut entries = fs::read_dir(&container_dir).await?;
            while let Some(entry) = entries.next().await {
                let entry = entry?;
                if entry.file_type().await?.is_dir() {
                    last_dir = Some(entry.path());
                }
            }
            let dir = last_dir.context("no cached harper-ls install")?;
            let server_path = dir.join(Self::binary_name());
            anyhow::ensure!(
                server_path.exists(),
                "missing harper-ls binary at {server_path:?}"
            );
            Ok(LanguageServerBinary {
                path: server_path,
                env: None,
                arguments: vec!["--stdio".into()],
            })
        })
        .await
        .log_err()
    }
}

#[async_trait(?Send)]
impl LspAdapter for HarperLspAdapter {
    fn name(&self) -> LanguageServerName {
        Self::SERVER_NAME
    }

    async fn workspace_configuration(
        self: Arc<Self>,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: Option<Toolchain>,
        _: Option<Uri>,
        cx: &mut AsyncApp,
    ) -> Result<Value> {
        // PaddleBoard: ship a less-naggy default lint set for prose. Spelling and
        // clear errors (a/an, repeated words, unclosed quotes) stay on; the most
        // opinionated style linters are off because they fire constantly on
        // casual Markdown and commit messages (which routinely start lowercase or
        // run long). `diagnosticSeverity: hint` keeps any remaining flags as
        // unobtrusive underlines. Users can re-enable any of this via
        // `lsp.harper-ls.settings` — their values win via the merge below.
        let mut harper_settings = json!({
            "diagnosticSeverity": "hint",
            "linters": {
                "SentenceCapitalization": false,
                "LongSentences": false,
                "SpelledNumbers": false,
            }
        });

        let user_settings = cx.update(|cx| {
            language_server_settings(delegate.as_ref(), &self.name(), cx)
                .and_then(|settings| settings.settings.clone())
        });
        if let Some(user_settings) = user_settings {
            merge_json_value_into(user_settings, &mut harper_settings);
        }

        Ok(json!({ "harper-ls": harper_settings }))
    }
}
