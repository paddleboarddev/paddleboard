// PaddleBoard: Dockerfile language adapter.
//
// Uses `dockerfile-language-server-nodejs` (the canonical Dockerfile LSP from
// rcjsuen, binary `docker-langserver`), distributed as a regular npm package —
// completion, hover, diagnostics, and formatting for Dockerfiles.
//
// Lifecycle mirrors `php.rs` / `css.rs`:
//   1. `check_if_user_installed` looks for `docker-langserver` on $PATH
//      (a global `npm install -g dockerfile-language-server-nodejs`).
//   2. Otherwise the adapter `npm install`s the latest package into
//      `<container>/node_modules/` and invokes
//      `node <container>/node_modules/dockerfile-language-server-nodejs/lib/server.js --stdio`.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use gpui::AsyncApp;
pub use language::*;
use language::{LspAdapterDelegate, LspInstaller, Toolchain};
use lsp::{LanguageServerBinary, LanguageServerName};
use node_runtime::{NodeRuntime, VersionStrategy};
use semver::Version;
use smol::fs;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};
use util::{ResultExt, maybe};

pub struct DockerfileLspAdapter {
    node: NodeRuntime,
}

impl DockerfileLspAdapter {
    const PACKAGE_NAME: &'static str = "dockerfile-language-server-nodejs";
    const SERVER_PATH: &'static str =
        "node_modules/dockerfile-language-server-nodejs/lib/server.js";
    const SERVER_NAME: LanguageServerName = LanguageServerName::new_static("docker-langserver");

    pub fn new(node: NodeRuntime) -> Self {
        Self { node }
    }
}

fn docker_langserver_binary_arguments(server_path: &Path) -> Vec<OsString> {
    vec![server_path.into(), "--stdio".into()]
}

impl LspInstaller for DockerfileLspAdapter {
    type BinaryVersion = Version;

    async fn fetch_latest_server_version(
        &self,
        _: &Arc<dyn LspAdapterDelegate>,
        _: bool,
        _: &mut AsyncApp,
    ) -> Result<Version> {
        self.node.npm_package_latest_version(Self::PACKAGE_NAME).await
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

    fn check_if_version_installed(
        &self,
        version: &Version,
        container_dir: &PathBuf,
        _: &Arc<dyn LspAdapterDelegate>,
    ) -> impl Send + std::future::Future<Output = Option<LanguageServerBinary>> + use<> {
        let node = self.node.clone();
        let version = version.clone();
        let container_dir = container_dir.clone();
        async move {
            let server_path = container_dir.join(Self::SERVER_PATH);
            if node
                .should_install_npm_package(
                    Self::PACKAGE_NAME,
                    &server_path,
                    &container_dir,
                    VersionStrategy::Latest(&version),
                )
                .await
            {
                return None;
            }
            Some(LanguageServerBinary {
                path: node.binary_path().await.ok()?,
                env: None,
                arguments: docker_langserver_binary_arguments(&server_path),
            })
        }
    }

    fn fetch_server_binary(
        &self,
        latest_version: Version,
        container_dir: PathBuf,
        _: &Arc<dyn LspAdapterDelegate>,
    ) -> impl Send + std::future::Future<Output = Result<LanguageServerBinary>> + use<> {
        let node = self.node.clone();
        async move {
            let server_path = container_dir.join(Self::SERVER_PATH);
            node.npm_install_packages(
                &container_dir,
                &[(Self::PACKAGE_NAME, &latest_version.to_string())],
            )
            .await?;
            Ok(LanguageServerBinary {
                path: node.binary_path().await?,
                env: None,
                arguments: docker_langserver_binary_arguments(&server_path),
            })
        }
    }

    async fn cached_server_binary(
        &self,
        container_dir: PathBuf,
        _: &dyn LspAdapterDelegate,
    ) -> Option<LanguageServerBinary> {
        maybe!(async {
            let server_path = container_dir.join(Self::SERVER_PATH);
            anyhow::ensure!(
                fs::metadata(&server_path).await.is_ok(),
                "missing docker-langserver binary at {server_path:?}"
            );
            Ok(LanguageServerBinary {
                path: self.node.binary_path().await?,
                env: None,
                arguments: docker_langserver_binary_arguments(&server_path),
            })
        })
        .await
        .log_err()
    }
}

#[async_trait(?Send)]
impl LspAdapter for DockerfileLspAdapter {
    fn name(&self) -> LanguageServerName {
        Self::SERVER_NAME
    }
}
