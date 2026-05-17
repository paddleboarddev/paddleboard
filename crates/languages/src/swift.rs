// PaddleBoard: Swift language adapter.
//
// `sourcekit-lsp` ships with the Swift toolchain (Xcode on macOS, the
// swift.org toolchain installer on Linux/Windows). We never attempt to
// download or build it ourselves — the right install vector is always the
// platform toolchain, and shipping a parallel sourcekit-lsp would invite
// ABI drift with the user's actual Swift compiler. The adapter is
// therefore $PATH-only: if sourcekit-lsp isn't reachable, surface a
// notification pointing the user at swift.org / Xcode and bail.

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

pub struct SwiftLspAdapter;

impl SwiftLspAdapter {
    const SERVER_NAME: LanguageServerName = LanguageServerName::new_static("sourcekit-lsp");

    const MISSING_TOOLCHAIN_NOTIFICATION: &'static str =
        "sourcekit-lsp was not found on PATH. Install the Swift toolchain (Xcode on macOS, \
         or the swift.org installer on Linux/Windows) and ensure `sourcekit-lsp` is on PATH.";
}

impl LspInstaller for SwiftLspAdapter {
    // sourcekit-lsp comes from the platform toolchain, so there is no
    // versioned artifact we'd download — a unit version type keeps the
    // trait satisfied without modeling something we never use.
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
                delegate.show_notification(Self::MISSING_TOOLCHAIN_NOTIFICATION, cx);
            });
        }
        bail!("{}", Self::MISSING_TOOLCHAIN_NOTIFICATION);
    }

    async fn fetch_server_binary(
        &self,
        _: (),
        _container_dir: PathBuf,
        _delegate: &dyn LspAdapterDelegate,
    ) -> Result<LanguageServerBinary> {
        bail!("{}", Self::MISSING_TOOLCHAIN_NOTIFICATION);
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
impl LspAdapter for SwiftLspAdapter {
    fn name(&self) -> LanguageServerName {
        Self::SERVER_NAME
    }
}
