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
// After locating the binary, the adapter probes `java -version` and
// shows a once-per-session notification if the JDK is below 17.

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
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
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
        delegate: &Arc<dyn LspAdapterDelegate>,
        pre_release: bool,
        cx: &mut AsyncApp,
    ) -> Result<GitHubLspBinaryVersion> {
        check_jdk_version(delegate, cx).await;
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

const MIN_JDK_VERSION: u32 = 17;

static DID_WARN_JDK: AtomicBool = AtomicBool::new(false);

async fn check_jdk_version(delegate: &Arc<dyn LspAdapterDelegate>, cx: &mut AsyncApp) {
    if DID_WARN_JDK.load(SeqCst) {
        return;
    }

    let java_path = match delegate.which("java".as_ref()).await {
        Some(path) => path,
        None => {
            if DID_WARN_JDK.compare_exchange(false, true, SeqCst, SeqCst).is_ok() {
                cx.update(|cx| {
                    delegate.show_notification(
                        "kotlin-language-server requires Java 17+ but `java` was not found on PATH. \
                         Install a JDK (macOS: `brew install openjdk@21`, Debian/Ubuntu: \
                         `apt install openjdk-21-jdk`) and restart PaddleBoard.",
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

    // `java -version` prints to stderr, e.g.:
    //   openjdk version "21.0.2" 2024-01-16
    // or older:
    //   java version "1.8.0_392"
    let stderr = String::from_utf8_lossy(&output.stderr);
    if let Some(major) = parse_java_major_version(&stderr) {
        if major < MIN_JDK_VERSION {
            if DID_WARN_JDK.compare_exchange(false, true, SeqCst, SeqCst).is_ok() {
                cx.update(|cx| {
                    delegate.show_notification(
                        &format!(
                            "kotlin-language-server requires Java {MIN_JDK_VERSION}+ but found \
                             Java {major}. Install a newer JDK (macOS: `brew install openjdk@21`, \
                             Debian/Ubuntu: `apt install openjdk-21-jdk`) and restart PaddleBoard."
                        ),
                        cx,
                    );
                });
            }
        }
    }

    // PaddleBoard: the kotlin-language-server launcher script checks JAVA_HOME
    // independently of `java` on PATH. If JAVA_HOME is set but invalid, the
    // script fails with a confusing error ("JAVA_HOME is set to an invalid
    // directory") that surfaces as "server reset the connection". Warn early.
    if let Ok(java_home) = std::env::var("JAVA_HOME") {
        if !java_home.is_empty() && !std::path::Path::new(&java_home).is_dir() {
            if DID_WARN_JDK.compare_exchange(false, true, SeqCst, SeqCst).is_ok() {
                cx.update(|cx| {
                    delegate.show_notification(
                        &format!(
                            "JAVA_HOME is set to \"{java_home}\" which is not a valid directory. \
                             kotlin-language-server will fail to start. Fix JAVA_HOME or unset it \
                             so the launcher can find Java automatically."
                        ),
                        cx,
                    );
                });
            }
        }
    }
}

pub fn parse_java_major_version(version_output: &str) -> Option<u32> {
    // Match patterns like `version "21.0.2"` or `version "1.8.0_392"`
    let version_str = version_output
        .lines()
        .find(|line| line.contains("version"))?;
    let start = version_str.find('"')? + 1;
    let end = version_str[start..].find('"')? + start;
    let version_num = &version_str[start..end];

    let first_component: u32 = version_num
        .split('.')
        .next()?
        .parse()
        .ok()?;

    // JDK 8 and earlier use "1.x" versioning (e.g. "1.8.0_392" = JDK 8)
    if first_component == 1 {
        version_num.split('.').nth(1)?.parse().ok()
    } else {
        Some(first_component)
    }
}

#[async_trait(?Send)]
impl LspAdapter for KotlinLspAdapter {
    fn name(&self) -> LanguageServerName {
        Self::SERVER_NAME
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_modern_jdk_version() {
        assert_eq!(
            parse_java_major_version(r#"openjdk version "21.0.2" 2024-01-16"#),
            Some(21)
        );
    }

    #[test]
    fn parse_legacy_jdk_version() {
        assert_eq!(
            parse_java_major_version(r#"java version "1.8.0_392""#),
            Some(8)
        );
    }

    #[test]
    fn parse_jdk_17() {
        assert_eq!(
            parse_java_major_version(r#"openjdk version "17.0.10" 2024-01-16"#),
            Some(17)
        );
    }

    #[test]
    fn parse_garbage_returns_none() {
        assert_eq!(parse_java_major_version("not a version string"), None);
    }
}
