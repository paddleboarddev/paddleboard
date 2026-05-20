use std::sync::Arc;
// PaddleBoard: Kotlin language adapter.
//
// Uses fwcd/kotlin-language-server, distributed as a single `server.zip`
// per GitHub release (no platform suffix — the launcher is a shell
// script and the implementation is a fat JAR, so one artifact works
// across macOS/Linux/Windows). Lifecycle:
//   1. `check_if_user_installed` looks for `kotlin-language-server` on
//      $PATH (Homebrew / scoop / system package installs).
//   2. Otherwise `fetch_server_binary` downloads `server.zip` from the
//      latest release, extracts it under
//      `<container>/kotlin-language-server_<tag>/server/`, and uses
//      `server/bin/kotlin-language-server[.bat]` as the binary.
// The launcher requires a JVM on $PATH to run; we don't pre-check
// because the LSP host surfaces the spawn failure clearly enough — a
// future polish could probe `java -version` and show a notification.

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use futures::StreamExt;
use gpui::AsyncApp;
use http_client::github::{AssetKind, GitHubLspBinaryVersion, latest_github_release};
use http_client::github_download::{GithubBinaryMetadata, download_server_binary};
pub use language::*;
use lsp::{LanguageServerBinary, LanguageServerName};
use smol::fs;
use std::path::PathBuf;
use util::{ResultExt, fs::remove_matching, maybe};

pub struct KotlinLspAdapter;

impl KotlinLspAdapter {
    const SERVER_NAME: LanguageServerName =
        LanguageServerName::new_static("kotlin-language-server");

    fn binary_name() -> &'static str {
        if cfg!(target_os = "windows") {
            "kotlin-language-server.bat"
        } else {
            "kotlin-language-server"
        }
    }
}

impl LspInstaller for KotlinLspAdapter {
    type BinaryVersion = GitHubLspBinaryVersion;

    async fn fetch_latest_server_version(
        &self,
        delegate: &dyn LspAdapterDelegate,
        pre_release: bool,
        _: &mut AsyncApp,
    ) -> Result<GitHubLspBinaryVersion> {
        let release = latest_github_release(
            "fwcd/kotlin-language-server",
            true,
            pre_release,
            delegate.http_client(),
        )
        .await?;
        // Every fwcd/kotlin-language-server release ships exactly one
        // asset named `server.zip`.
        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == "server.zip")
            .with_context(|| {
                format!(
                    "no `server.zip` asset on kotlin-language-server release {:?}",
                    release.tag_name
                )
            })?;
        Ok(GitHubLspBinaryVersion {
            name: release.tag_name,
            url: asset.browser_download_url.clone(),
            digest: asset.digest.clone(),
        })
    }

    async fn check_if_user_installed(
        &self,
        delegate: &dyn LspAdapterDelegate,
        _: Option<Toolchain>,
        _: &AsyncApp,
    ) -> Option<LanguageServerBinary> {
        let path = delegate.which(Self::SERVER_NAME.as_ref()).await?;
        Some(LanguageServerBinary {
            path,
            arguments: Vec::new(),
            env: None,
        })
    }

    fn fetch_server_binary(
        &self,
        version: GitHubLspBinaryVersion,
        container_dir: PathBuf,
        delegate: &Arc<dyn LspAdapterDelegate>,
    ) -> impl Send + std::future::Future<Output = Result<LanguageServerBinary>> + use<> {
        let delegate = delegate.clone();
        async move {
        let GitHubLspBinaryVersion {
            name,
            url,
            digest: expected_digest,
        } = version;
        let version_dir = container_dir.join(format!("kotlin-language-server_{name}"));
        let binary_path = version_dir
            .join("server")
            .join("bin")
            .join(Self::binary_name());

        let binary = LanguageServerBinary {
            path: binary_path.clone(),
            env: None,
            arguments: Vec::new(),
        };

        let metadata_path = version_dir.join("metadata");
        if let Some(metadata) = GithubBinaryMetadata::read_from_file(&metadata_path).await.ok() {
            // The launcher is a shell script that shells out to `java`,
            // so a `--version` smoke test would inadvertently boot a JVM
            // every check — just confirm the file is on disk.
            let still_present = fs::metadata(&binary_path).await.is_ok();
            if let (Some(actual_digest), Some(expected_digest)) =
                (&metadata.digest, &expected_digest)
            {
                if actual_digest == expected_digest && still_present {
                    return Ok(binary);
                }
                log::info!(
                    "SHA-256 mismatch for {binary_path:?}, redownloading. Expected: {expected_digest}, Got: {actual_digest}"
                );
            } else if still_present {
                return Ok(binary);
            }
        }

        download_server_binary(
            &*delegate.http_client(),
            &url,
            expected_digest.as_deref(),
            &container_dir,
            AssetKind::Zip,
        )
        .await?;

        // `download_server_binary` extracts directly into `container_dir`.
        // The zip's top-level is a `server/` directory; rename it under a
        // versioned slot so we can keep multiple installs and prune the
        // others.
        let extracted_server = container_dir.join("server");
        if extracted_server.exists() {
            if version_dir.exists() {
                fs::remove_dir_all(&version_dir).await.log_err();
            }
            fs::create_dir_all(&version_dir).await?;
            fs::rename(&extracted_server, version_dir.join("server")).await?;
        }
        remove_matching(&container_dir, |entry| entry != version_dir).await;

        GithubBinaryMetadata::write_to_file(
            &GithubBinaryMetadata {
                metadata_version: 1,
                digest: expected_digest,
            },
            &metadata_path,
        )
        .await?;

        // Make sure the launcher is executable; the unzip step may have
        // dropped the mode bits on some platforms.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(metadata) = fs::metadata(&binary_path).await {
                let mut perms = metadata.permissions();
                perms.set_mode(perms.mode() | 0o755);
                fs::set_permissions(&binary_path, perms).await.log_err();
            }
        }

        Ok(binary)
        }
    }

    async fn cached_server_binary(
        &self,
        container_dir: PathBuf,
        _: &dyn LspAdapterDelegate,
    ) -> Option<LanguageServerBinary> {
        maybe!(async {
            let mut latest_dir = None;
            let mut entries = fs::read_dir(&container_dir).await?;
            while let Some(entry) = entries.next().await {
                let entry = entry?;
                if entry.file_type().await?.is_dir() {
                    latest_dir = Some(entry.path());
                }
            }
            let dir = latest_dir.context("no cached kotlin-language-server install")?;
            let binary_path = dir
                .join("server")
                .join("bin")
                .join(Self::binary_name());
            anyhow::ensure!(
                binary_path.exists(),
                "missing kotlin-language-server binary at {binary_path:?}"
            );
            Ok(LanguageServerBinary {
                path: binary_path,
                env: None,
                arguments: Vec::new(),
            })
        })
        .await
        .log_err()
    }
}

#[async_trait(?Send)]
impl LspAdapter for KotlinLspAdapter {
    fn name(&self) -> LanguageServerName {
        Self::SERVER_NAME
    }
}
