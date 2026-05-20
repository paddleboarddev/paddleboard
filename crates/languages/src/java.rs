use std::sync::Arc;
// PaddleBoard: Java language adapter.
//
// `jdtls` (Eclipse JDT Language Server) is the de-facto Java LSP, but
// its native distribution is an Eclipse RCP tarball with a versioned
// `org.eclipse.equinox.launcher_*.jar`, platform-specific
// `config_{mac,linux,win}/` directories, and a ~50-line `java -jar …`
// invocation that needs JDK 21+ at runtime. Building all that into the
// adapter (matching the Zed Java extension at `zed-extensions/java`)
// would be ~600 LOC of JDK probing, version checking, equinox launcher
// discovery, and JVM argument construction.
//
// This adapter ships a simpler v1: $PATH-only. Both Homebrew
// (`brew install jdtls`) and most Linux package managers (`apt
// install jdtls` / equivalent) install a launcher script named `jdtls`
// that wraps the whole equinox invocation, so a $PATH lookup
// delegates the hard distribution problem to the user's package
// manager. Highlighting, outline, indentation, and runnable tests
// still work without the LSP — only completion / hover / go-to-def
// require it. The fetch_* methods bail with a notification listing
// canonical install paths, so users get a clear next step.
//
// Auto-download of the equinox launcher tarball is a worthwhile
// follow-up but intentionally out of scope here.

use anyhow::{Result, bail};
use async_trait::async_trait;
use gpui::AsyncApp;
pub use language::*;
use language::{LspAdapterDelegate, LspInstaller, Toolchain};
use lsp::{LanguageServerBinary, LanguageServerName};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering::SeqCst},
};

pub struct JavaLspAdapter;

impl JavaLspAdapter {
    const SERVER_NAME: LanguageServerName = LanguageServerName::new_static("jdtls");

    const MISSING_JDTLS_NOTIFICATION: &'static str =
        "jdtls (Eclipse JDT Language Server) was not found on PATH. Install it via your \
         package manager (macOS: `brew install jdtls`, Debian/Ubuntu: `apt install jdtls`) \
         or download the binary from https://download.eclipse.org/jdtls/snapshots/ and put \
         the `jdtls` launcher on PATH. JDK 21+ is also required at runtime.";
}

impl LspInstaller for JavaLspAdapter {
    // No versioned artifact to download in v1 — the adapter delegates
    // installation to the user's package manager.
    type BinaryVersion = ();

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

    async fn fetch_latest_server_version(
        &self,
        delegate: &dyn LspAdapterDelegate,
        _pre_release: bool,
        cx: &mut AsyncApp,
    ) -> Result<()> {
        static DID_SHOW_NOTIFICATION: AtomicBool = AtomicBool::new(false);
        if DID_SHOW_NOTIFICATION
            .compare_exchange(false, true, SeqCst, SeqCst)
            .is_ok()
        {
            cx.update(|cx| {
                delegate.show_notification(Self::MISSING_JDTLS_NOTIFICATION, cx);
            });
        }
        bail!("{}", Self::MISSING_JDTLS_NOTIFICATION);
    }

    fn fetch_server_binary(
        &self,
        _: (),
        _container_dir: PathBuf,
        _delegate: &Arc<dyn LspAdapterDelegate>,
    ) -> impl Send + std::future::Future<Output = Result<LanguageServerBinary>> + use<> {
        async move { bail!("{}", Self::MISSING_JDTLS_NOTIFICATION) }
    }

    async fn cached_server_binary(
        &self,
        _container_dir: PathBuf,
        _: &dyn LspAdapterDelegate,
    ) -> Option<LanguageServerBinary> {
        None
    }
}

#[async_trait(?Send)]
impl LspAdapter for JavaLspAdapter {
    fn name(&self) -> LanguageServerName {
        Self::SERVER_NAME
    }
}
