use std::borrow::Cow;
use std::sync::Arc;
// PaddleBoard: Java language adapter with auto-download.
//
// `jdtls` (Eclipse JDT Language Server) is downloaded from GitHub releases
// at `eclipse-jdtls/eclipse.jdt.ls`. Each release publishes a
// `jdt-language-server-<version>.tar.gz` containing a `bin/jdtls`
// launcher script that wraps the equinox invocation. Users who install
// via Homebrew (`brew install jdtls`) or apt get priority via
// `check_if_user_installed` — the auto-download is a fallback.

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use futures::StreamExt;
use gpui::AsyncApp;
use http_client::github::{AssetKind, GitHubLspBinaryVersion, latest_github_release};
use http_client::github_download::{GithubBinaryMetadata, download_server_binary};
pub use language::*;
use language::{LspAdapterDelegate, LspInstaller, Toolchain};
use lsp::{LanguageServerBinary, LanguageServerName};
use smol::fs;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering::SeqCst},
};
use util::{ResultExt, fs::remove_matching, maybe};

use crate::kotlin::parse_java_major_version;

const GITHUB_REPO: &str = "eclipse-jdtls/eclipse.jdt.ls";

pub struct JavaLspAdapter;

impl JavaLspAdapter {
    const SERVER_NAME: LanguageServerName = LanguageServerName::new_static("jdtls");
    const MIN_JDK_VERSION: u32 = 21;

    fn binary_name() -> &'static str {
        if cfg!(target_os = "windows") {
            "jdtls.bat"
        } else {
            "jdtls"
        }
    }
}

static DID_WARN_JAVA_JDK: AtomicBool = AtomicBool::new(false);

async fn check_java_jdk_version(delegate: &Arc<dyn LspAdapterDelegate>, cx: &mut AsyncApp) {
    if DID_WARN_JAVA_JDK.load(SeqCst) {
        return;
    }

    let java_path = match delegate.which("java".as_ref()).await {
        Some(path) => path,
        None => {
            if DID_WARN_JAVA_JDK
                .compare_exchange(false, true, SeqCst, SeqCst)
                .is_ok()
            {
                cx.update(|cx| {
                    delegate.show_notification(
                        "jdtls requires Java 21+ but `java` was not found on PATH. \
                         Install a JDK (macOS: `brew install openjdk@21`, \
                         Debian/Ubuntu: `apt install openjdk-21-jdk`) and restart PaddleBoard.",
                        cx,
                    );
                });
            }
            return;
        }
    };

    let output = match smol::process::Command::new(&java_path)
        .arg("-version")
        .output()
        .await
    {
        Ok(output) => output,
        Err(_) => return,
    };

    let stderr = String::from_utf8_lossy(&output.stderr);
    if let Some(major) = parse_java_major_version(&stderr) {
        if major < JavaLspAdapter::MIN_JDK_VERSION {
            if DID_WARN_JAVA_JDK
                .compare_exchange(false, true, SeqCst, SeqCst)
                .is_ok()
            {
                let min = JavaLspAdapter::MIN_JDK_VERSION;
                cx.update(|cx| {
                    delegate.show_notification(
                        &format!(
                            "jdtls requires Java {min}+ but found Java {major}. \
                             Install a newer JDK (macOS: `brew install openjdk@21`, \
                             Debian/Ubuntu: `apt install openjdk-21-jdk`) and restart PaddleBoard."
                        ),
                        cx,
                    );
                });
            }
        }
    }
}

impl LspInstaller for JavaLspAdapter {
    type BinaryVersion = GitHubLspBinaryVersion;

    async fn check_if_user_installed(
        &self,
        delegate: &Arc<dyn LspAdapterDelegate>,
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

    async fn fetch_latest_server_version(
        &self,
        delegate: &Arc<dyn LspAdapterDelegate>,
        pre_release: bool,
        cx: &mut AsyncApp,
    ) -> Result<GitHubLspBinaryVersion> {
        check_java_jdk_version(delegate, cx).await;
        let release = latest_github_release(
            GITHUB_REPO,
            true,
            pre_release,
            delegate.http_client(),
        )
        .await?;
        let asset = release
            .assets
            .iter()
            .find(|asset| {
                asset.name.starts_with("jdt-language-server-") && asset.name.ends_with(".tar.gz")
            })
            .with_context(|| {
                format!(
                    "no `jdt-language-server-*.tar.gz` asset on jdtls release {:?}",
                    release.tag_name
                )
            })?;
        Ok(GitHubLspBinaryVersion {
            name: release.tag_name,
            url: asset.browser_download_url.clone(),
            digest: asset.digest.clone(),
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
            let version_dir = container_dir.join(format!("jdtls_{name}"));
            let binary_path = version_dir.join("bin").join(Self::binary_name());

            let binary = LanguageServerBinary {
                path: binary_path.clone(),
                env: None,
                arguments: Vec::new(),
            };

            let metadata_path = version_dir.join("metadata");
            if let Some(metadata) =
                GithubBinaryMetadata::read_from_file(&metadata_path).await.ok()
            {
                let still_present = fs::metadata(&binary_path).await.is_ok();
                if let (Some(actual_digest), Some(expected_digest)) =
                    (&metadata.digest, &expected_digest)
                {
                    if actual_digest == expected_digest && still_present {
                        return Ok(binary);
                    }
                    log::info!(
                        "SHA-256 mismatch for {binary_path:?}, redownloading. \
                         Expected: {expected_digest}, Got: {actual_digest}"
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
                AssetKind::TarGz,
            )
            .await?;

            // `download_server_binary` extracts into `container_dir`. The
            // tarball's top-level contains `bin/`, `plugins/`, `config_*/`,
            // etc. Move everything into a versioned slot.
            if !version_dir.exists() {
                fs::create_dir_all(&version_dir).await?;
            }
            // Move the extracted `bin/` directory into the version slot.
            let extracted_bin = container_dir.join("bin");
            if extracted_bin.exists() {
                let target_bin = version_dir.join("bin");
                if target_bin.exists() {
                    fs::remove_dir_all(&target_bin).await.log_err();
                }
                fs::rename(&extracted_bin, &target_bin).await?;
            }
            // Move `plugins/` and `config_*/` directories too — jdtls
            // needs them at runtime relative to its launcher.
            for dir_name in ["plugins", "config_mac", "config_linux", "config_win", "features"] {
                let src = container_dir.join(dir_name);
                if src.exists() {
                    let dst = version_dir.join(dir_name);
                    if dst.exists() {
                        fs::remove_dir_all(&dst).await.log_err();
                    }
                    fs::rename(&src, &dst).await.log_err();
                }
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
            let dir = latest_dir.context("no cached jdtls install")?;
            let binary_path = dir.join("bin").join(Self::binary_name());
            anyhow::ensure!(
                binary_path.exists(),
                "missing jdtls binary at {binary_path:?}"
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
impl LspAdapter for JavaLspAdapter {
    fn name(&self) -> LanguageServerName {
        Self::SERVER_NAME
    }
}

// --- Build tool context provider (Gradle / Maven) ---

use collections::HashMap;
use gpui::Task;
use task::{TaskVariables, VariableName};

const JAVA_BUILD_TOOL_VARIABLE: VariableName =
    VariableName::Custom(Cow::Borrowed("JAVA_BUILD_TOOL"));
const JAVA_PROJECT_ROOT_VARIABLE: VariableName =
    VariableName::Custom(Cow::Borrowed("JAVA_PROJECT_ROOT"));

static GRADLE_MANIFESTS: &[&str] = &["build.gradle", "build.gradle.kts"];
static MAVEN_MANIFEST: &str = "pom.xml";

pub(crate) struct JavaBuildContextProvider;

impl ContextProvider for JavaBuildContextProvider {
    fn build_context(
        &self,
        _variables: &TaskVariables,
        location: ContextLocation<'_>,
        _: Option<HashMap<String, String>>,
        _: Arc<dyn language::LanguageToolchainStore>,
        cx: &mut gpui::App,
    ) -> Task<Result<TaskVariables>> {
        let local_abs_path = location
            .file_location
            .buffer
            .read(cx)
            .file()
            .and_then(|file| Some(file.as_local()?.abs_path(cx)));

        let mut variables = TaskVariables::default();

        if let Some(path) = local_abs_path.as_deref().and_then(|p| p.parent()) {
            for ancestor in path.ancestors() {
                if GRADLE_MANIFESTS.iter().any(|m| ancestor.join(m).is_file()) {
                    variables.insert(
                        JAVA_BUILD_TOOL_VARIABLE.clone(),
                        "gradle".to_string(),
                    );
                    variables.insert(
                        JAVA_PROJECT_ROOT_VARIABLE.clone(),
                        ancestor.to_string_lossy().into_owned(),
                    );
                    break;
                }
                if ancestor.join(MAVEN_MANIFEST).is_file() {
                    variables.insert(
                        JAVA_BUILD_TOOL_VARIABLE.clone(),
                        "maven".to_string(),
                    );
                    variables.insert(
                        JAVA_PROJECT_ROOT_VARIABLE.clone(),
                        ancestor.to_string_lossy().into_owned(),
                    );
                    break;
                }
            }
        }

        Task::ready(Ok(variables))
    }
}
