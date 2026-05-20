use std::sync::Arc;
// PaddleBoard: PHP language adapter.
//
// Uses `intelephense` (https://intelephense.com), distributed as a
// regular npm package. The free tier is plenty for editor features
// (completion, hover, go-to-def, diagnostics); paid features (rename,
// code actions across files) require a license key the user sets via
// `lsp.intelephense.initializationOptions.licenceKey` in settings.
//
// **License note.** intelephense is proprietary, not OSS. We don't
// redistribute it — `npm_install_packages` fetches it from npmjs.com at
// runtime. Users who prefer open-source alternatives can replace it via
// the `language_servers` override in their settings (e.g. point at
// phpactor or phpls).
//
// Lifecycle mirrors `typescript.rs`:
//   1. `check_if_user_installed` looks for `intelephense` on $PATH
//      (Homebrew / scoop / system npm-global installs).
//   2. Otherwise the adapter `npm install`s the latest `intelephense`
//      into `<container>/node_modules/` and invokes
//      `node <container>/node_modules/intelephense/lib/intelephense.js --stdio`.

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

pub struct PhpLspAdapter {
    node: NodeRuntime,
}

impl PhpLspAdapter {
    const PACKAGE_NAME: &'static str = "intelephense";
    const SERVER_PATH: &'static str = "node_modules/intelephense/lib/intelephense.js";
    const SERVER_NAME: LanguageServerName = LanguageServerName::new_static("intelephense");

    pub fn new(node: NodeRuntime) -> Self {
        Self { node }
    }
}

fn intelephense_server_binary_arguments(server_path: &Path) -> Vec<OsString> {
    vec![server_path.into(), "--stdio".into()]
}

impl LspInstaller for PhpLspAdapter {
    type BinaryVersion = Version;

    async fn fetch_latest_server_version(
        &self,
        _: &dyn LspAdapterDelegate,
        _: bool,
        _: &mut AsyncApp,
    ) -> Result<Version> {
        self.node.npm_package_latest_version(Self::PACKAGE_NAME).await
    }

    async fn check_if_user_installed(
        &self,
        delegate: &dyn LspAdapterDelegate,
        _: Option<Toolchain>,
        _: &AsyncApp,
    ) -> Option<LanguageServerBinary> {
        let path = delegate.which(Self::SERVER_NAME.as_ref()).await?;
        // intelephense installed via `npm install -g intelephense` ends
        // up as an executable shim that already includes `--stdio`
        // behavior when invoked without arguments — but to match the
        // download-path invocation, we still pass `--stdio` explicitly
        // so the launch shape is identical regardless of install source.
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
                arguments: intelephense_server_binary_arguments(&server_path),
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
                arguments: intelephense_server_binary_arguments(&server_path),
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
                "missing intelephense binary at {server_path:?}"
            );
            Ok(LanguageServerBinary {
                path: self.node.binary_path().await?,
                env: None,
                arguments: intelephense_server_binary_arguments(&server_path),
            })
        })
        .await
        .log_err()
    }
}

#[async_trait(?Send)]
impl LspAdapter for PhpLspAdapter {
    fn name(&self) -> LanguageServerName {
        Self::SERVER_NAME
    }
}
