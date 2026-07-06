//! Container engines for PaddleBoard's sandboxed tools.
//!
//! A [`ContainerEngine`] turns an [`ExecRequest`] (image + workspace dir +
//! shell command) into a host shell command that the agent's terminal can
//! spawn. Two backends exist:
//!
//! * [`EngineKind::PodmanGvisor`] — the preferred tier; wraps the same
//!   `podman run --rm --runtime=runsc …` invocation the sandbox tool has
//!   always issued.
//! * [`EngineKind::BuiltInKrun`] — the zero-install tier; boots a libkrun
//!   microVM via the `paddleboard-krun-helper` binary, with the image pulled
//!   and unpacked into a content-addressed cache by [`image_store`].
//! * [`EngineKind::AppleContainer`] — the macOS-native tier on macOS 26+ (Apple
//!   silicon). Like Podman it is a CLI shell-out (`container run …`) that owns
//!   its own OCI image pull/unpack, so preparation is pure string assembly and
//!   this crate never touches [`image_store`] for it.
//!
//! Which tier to use is decided by `paddleboard_sandbox_settings::decide_gate`;
//! this crate only executes that decision.

pub mod image_store;

use anyhow::{Context as _, Result, anyhow};
use futures::FutureExt as _;
use futures::future::BoxFuture;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use image_store::ImageStore;

/// Default image for one-shot sandboxed execs. Must stay in sync between the
/// Podman and built-in tiers so switching tiers never changes the userland a
/// command sees.
pub const DEFAULT_SANDBOX_IMAGE: &str = "ubuntu:latest";

/// Fixed path where the project worktree is mounted inside the container.
pub const CONTAINER_WORKDIR: &str = "/workspace";

/// Guest resources for built-in microVMs. Deliberately modest: one-shot tool
/// commands are compiles and test runs, not services.
const KRUN_VCPUS: u8 = 2;
const KRUN_MEM_MIB: u32 = 2048;

/// Where the user's shell command is written inside the ephemeral rootfs.
/// It travels as a file because libkrun's exec/env transport (the kernel
/// command line) is ASCII-only and single-line; a file carries arbitrary
/// multi-line UTF-8 commands intact.
const GUEST_COMMAND_FILE: &str = "/.pb-sandbox-cmd.sh";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineKind {
    PodmanGvisor,
    BuiltInKrun,
    AppleContainer,
}

/// A one-shot command to run inside a container.
pub struct ExecRequest {
    pub image: String,
    /// Host directory mounted read-write at [`CONTAINER_WORKDIR`].
    pub host_workdir: PathBuf,
    /// User shell command, run via `bash -c` inside the container.
    pub command: String,
}

/// The resolved invocation: a self-contained shell command for a host
/// terminal. For the built-in tier this includes cleanup of the per-run
/// ephemeral rootfs after the microVM exits.
pub struct PreparedExec {
    pub shell_command: String,
}

pub trait ContainerEngine: Send + Sync {
    fn kind(&self) -> EngineKind;

    /// Resolve everything needed to run `request` (for the built-in tier:
    /// pull + unpack the image, create the per-run rootfs) and return the
    /// host shell command. May take a while on a cold image cache.
    fn prepare_exec(&self, request: ExecRequest) -> BoxFuture<'static, Result<PreparedExec>>;

    /// True when `prepare_exec` can complete without network access for this
    /// image (used to decide whether to surface pull progress).
    fn is_image_ready(&self, image: &str) -> bool;
}

pub fn engine(kind: EngineKind) -> Arc<dyn ContainerEngine> {
    match kind {
        EngineKind::PodmanGvisor => Arc::new(PodmanGvisorEngine),
        EngineKind::BuiltInKrun => Arc::new(BuiltInKrunEngine::global()),
        EngineKind::AppleContainer => Arc::new(AppleContainerEngine),
    }
}

/// The Podman + gVisor tier. Podman owns image management, so preparation is
/// pure string assembly.
pub struct PodmanGvisorEngine;

impl ContainerEngine for PodmanGvisorEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::PodmanGvisor
    }

    fn prepare_exec(&self, request: ExecRequest) -> BoxFuture<'static, Result<PreparedExec>> {
        futures::future::ready(Ok(PreparedExec {
            shell_command: podman_exec_command(&request),
        }))
        .boxed()
    }

    fn is_image_ready(&self, _image: &str) -> bool {
        // `podman run` pulls on demand and streams its own progress into the
        // terminal, so there is nothing to prepare ahead of time.
        true
    }
}

fn podman_exec_command(request: &ExecRequest) -> String {
    let host_wd = shell_single_quote(&request.host_workdir.to_string_lossy());
    let container_wd = shell_single_quote(CONTAINER_WORKDIR);
    let image = shell_single_quote(&request.image);
    let user_command = shell_single_quote(&request.command);
    format!(
        "podman run --rm --runtime=runsc -v {host_wd}:{container_wd} -w {container_wd} {image} bash -c {user_command}",
    )
}

/// The macOS-native tier: Apple's `container` CLI (macOS 26+, Apple silicon).
/// Like Podman, `container` owns image management and pulls on demand, so
/// preparation is pure string assembly — this backend never touches
/// [`ImageStore`]. Selection is gated by `paddleboard_sandbox_prereqs`, which
/// only resolves this backend when the CLI and its background service are
/// present; the command builder itself is host-agnostic so its pin test runs
/// everywhere.
pub struct AppleContainerEngine;

impl ContainerEngine for AppleContainerEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::AppleContainer
    }

    fn prepare_exec(&self, request: ExecRequest) -> BoxFuture<'static, Result<PreparedExec>> {
        futures::future::ready(Ok(PreparedExec {
            shell_command: apple_container_exec_command(&request),
        }))
        .boxed()
    }

    fn is_image_ready(&self, _image: &str) -> bool {
        // `container run` pulls on demand and streams its own progress into the
        // terminal, so there is nothing to prepare ahead of time.
        true
    }
}

fn apple_container_exec_command(request: &ExecRequest) -> String {
    let host_wd = shell_single_quote(&request.host_workdir.to_string_lossy());
    let container_wd = shell_single_quote(CONTAINER_WORKDIR);
    let image = shell_single_quote(&request.image);
    let user_command = shell_single_quote(&request.command);
    // `container run` mirrors the Podman invocation: bind-mount the worktree at
    // the fixed guest path, set it as the workdir, remove the container on exit,
    // and run the user command through `bash -c`. Adjacent single-quoted strings
    // concatenate in the shell, so `'host':'guest'` reaches `container` as one
    // `-v host:guest` argument.
    format!(
        "container run --rm -v {host_wd}:{container_wd} -w {container_wd} {image} bash -c {user_command}",
    )
}

/// The built-in libkrun microVM tier.
pub struct BuiltInKrunEngine {
    store_root: PathBuf,
}

impl BuiltInKrunEngine {
    pub fn global() -> Self {
        Self {
            store_root: ImageStore::global_root(),
        }
    }

    pub fn with_store_root(store_root: PathBuf) -> Self {
        Self { store_root }
    }
}

impl ContainerEngine for BuiltInKrunEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::BuiltInKrun
    }

    fn prepare_exec(&self, request: ExecRequest) -> BoxFuture<'static, Result<PreparedExec>> {
        let store_root = self.store_root.clone();
        async move {
            let (tx, rx) = futures::channel::oneshot::channel();
            // Image pull + unpack + rootfs clone are blocking (ureq download,
            // tar extraction, filesystem clone), so they run on a dedicated
            // thread rather than a foreground executor.
            std::thread::Builder::new()
                .name("pb-container-prepare".into())
                .spawn(move || {
                    let result = prepare_builtin_exec(store_root, &request);
                    // The receiver being dropped just means the tool call was
                    // cancelled; nothing to clean up beyond what GC covers.
                    tx.send(result).ok();
                })
                .context("failed to spawn container prepare thread")?;
            rx.await
                .map_err(|_| anyhow!("container prepare thread died"))?
        }
        .boxed()
    }

    fn is_image_ready(&self, image: &str) -> bool {
        ImageStore::new(self.store_root.clone())
            .cached_rootfs(image)
            .is_some()
    }
}

fn prepare_builtin_exec(store_root: PathBuf, request: &ExecRequest) -> Result<PreparedExec> {
    let libkrun = paddleboard_sandbox_prereqs::find_libkrun()
        .context("libkrun is not installed (see script/setup-builtin-sandbox)")?;
    let libkrunfw = paddleboard_sandbox_prereqs::find_libkrunfw();
    let helper = paddleboard_sandbox_prereqs::find_krun_helper()
        .context("the paddleboard-krun-helper binary was not found next to the app")?;

    let store = ImageStore::new(store_root);
    store.remove_stale_ephemerals();
    let rootfs = store.ensure_image(&request.image)?;
    let ephemeral = store.create_ephemeral_rootfs(&rootfs)?;

    let command_file = ephemeral.join(GUEST_COMMAND_FILE.trim_start_matches('/'));
    std::fs::write(&command_file, format!("{}\n", request.command))
        .with_context(|| format!("failed to write guest command file {command_file:?}"))?;

    Ok(PreparedExec {
        shell_command: builtin_exec_command(
            &helper,
            &libkrun,
            libkrunfw.as_deref(),
            &ephemeral,
            request,
        ),
    })
}

fn builtin_exec_command(
    helper: &Path,
    libkrun: &Path,
    libkrunfw: Option<&Path>,
    ephemeral_rootfs: &Path,
    request: &ExecRequest,
) -> String {
    let helper = shell_single_quote(&helper.to_string_lossy());
    let libkrun = shell_single_quote(&libkrun.to_string_lossy());
    // libkrun dlopens libkrunfw (the guest kernel) by bare soname at VM
    // start, and dyld only honors search paths present when the helper is
    // exec'd — an inherited variable would already have been stripped by SIP
    // on its way through bash. So the assignment must be part of the command
    // itself. The stock fallback dirs are kept so nothing else regresses.
    let dyld_prefix = if cfg!(target_os = "macos") {
        libkrunfw
            .and_then(|path| path.parent())
            .map(|dir| {
                format!(
                    "DYLD_FALLBACK_LIBRARY_PATH={} ",
                    shell_single_quote(&format!(
                        "{}:/usr/local/lib:/usr/lib",
                        dir.to_string_lossy()
                    ))
                )
            })
            .unwrap_or_default()
    } else {
        String::new()
    };
    let root = shell_single_quote(&ephemeral_rootfs.to_string_lossy());
    let workspace = shell_single_quote(&request.host_workdir.to_string_lossy());
    let guest_workdir = shell_single_quote(CONTAINER_WORKDIR);
    let command_file = shell_single_quote(GUEST_COMMAND_FILE);
    // The per-run rootfs is removed after the microVM exits, mirroring
    // `podman run --rm`. The helper's exit status (the guest command's status)
    // is preserved across the cleanup.
    format!(
        "{dyld_prefix}{helper} --libkrun {libkrun} --root {root} --workspace {workspace} \
         --guest-workdir {guest_workdir} --cpus {KRUN_VCPUS} --mem-mib {KRUN_MEM_MIB} \
         --guest-command-file {command_file}; pb_status=$?; rm -rf {root}; exit $pb_status",
    )
}

/// POSIX shell single-quote escaping: wrap `s` in single quotes, and replace
/// every `'` in `s` with `'\''` (close quote, escaped literal quote, reopen
/// quote). The result is safe to pass through `bash -c` and through any POSIX
/// shell interpolation, regardless of the original contents of `s`.
pub fn shell_single_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> ExecRequest {
        ExecRequest {
            image: DEFAULT_SANDBOX_IMAGE.to_string(),
            host_workdir: PathBuf::from("/Users/jay/my project"),
            command: "echo \"it's alive\" && uname -a".to_string(),
        }
    }

    #[test]
    fn quotes_plain_strings() {
        assert_eq!(shell_single_quote("hello"), "'hello'");
        assert_eq!(shell_single_quote(""), "''");
    }

    #[test]
    fn escapes_embedded_single_quotes() {
        assert_eq!(shell_single_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_single_quote("'"), "''\\'''");
    }

    #[test]
    fn passes_through_shell_metacharacters_verbatim() {
        assert_eq!(
            shell_single_quote("rm -rf $HOME && echo pwned"),
            "'rm -rf $HOME && echo pwned'"
        );
    }

    #[test]
    fn podman_command_matches_the_historical_invocation() {
        // This string is load-bearing: it must stay identical to what
        // sandbox_tool issued before the engine abstraction existed.
        let command = podman_exec_command(&request());
        assert_eq!(
            command,
            "podman run --rm --runtime=runsc -v '/Users/jay/my project':'/workspace' \
             -w '/workspace' 'ubuntu:latest' bash -c 'echo \"it'\\''s alive\" && uname -a'"
        );
    }

    #[test]
    fn apple_container_command_matches_the_cli_contract() {
        // Pinned bit-for-bit like the Podman invocation: `container run --rm`
        // with the worktree bind-mounted at the fixed guest path and the user
        // command run through `bash -c`. `container` owns its own image pull, so
        // there is no prepare step — this is pure string assembly.
        let command = apple_container_exec_command(&request());
        assert_eq!(
            command,
            "container run --rm -v '/Users/jay/my project':'/workspace' \
             -w '/workspace' 'ubuntu:latest' bash -c 'echo \"it'\\''s alive\" && uname -a'"
        );
    }

    #[test]
    fn builtin_command_cleans_up_ephemeral_rootfs_and_preserves_status() {
        let command = builtin_exec_command(
            Path::new("/Applications/PaddleBoard.app/Contents/MacOS/paddleboard-krun-helper"),
            Path::new("/opt/homebrew/lib/libkrun.dylib"),
            Some(Path::new("/opt/homebrew/lib/libkrunfw.5.dylib")),
            Path::new("/data/containers/tmp/run-1-2"),
            &request(),
        );
        if cfg!(target_os = "macos") {
            assert!(command.starts_with(
                "DYLD_FALLBACK_LIBRARY_PATH='/opt/homebrew/lib:/usr/local/lib:/usr/lib' \
                 '/Applications/PaddleBoard.app/Contents/MacOS/paddleboard-krun-helper' \
                 --libkrun '/opt/homebrew/lib/libkrun.dylib'"
            ));
        } else {
            assert!(command.starts_with(
                "'/Applications/PaddleBoard.app/Contents/MacOS/paddleboard-krun-helper' \
                 --libkrun '/opt/homebrew/lib/libkrun.dylib'"
            ));
        }
        assert!(command.contains("--root '/data/containers/tmp/run-1-2'"));
        assert!(command.contains("--workspace '/Users/jay/my project'"));
        assert!(command.contains("--guest-workdir '/workspace'"));
        // The user command must NOT appear inline — it travels as a file in
        // the rootfs because libkrun's config transport is ASCII-only.
        assert!(command.contains("--guest-command-file '/.pb-sandbox-cmd.sh'"));
        assert!(!command.contains("uname"));
        assert!(command.ends_with(
            "; pb_status=$?; rm -rf '/data/containers/tmp/run-1-2'; exit $pb_status"
        ));
    }

    #[test]
    fn engine_constructor_returns_matching_kind() {
        assert_eq!(
            engine(EngineKind::PodmanGvisor).kind(),
            EngineKind::PodmanGvisor
        );
        assert_eq!(
            engine(EngineKind::BuiltInKrun).kind(),
            EngineKind::BuiltInKrun
        );
        assert_eq!(
            engine(EngineKind::AppleContainer).kind(),
            EngineKind::AppleContainer
        );
    }

    /// End-to-end proof that the built-in tier runs a real microVM: pulls the
    /// default image, boots libkrun, and checks that the command ran under a
    /// Linux kernel with the workspace mounted read-write.
    ///
    /// Requires libkrun installed, a signed helper (script/setup-builtin-sandbox),
    /// `PADDLEBOARD_KRUN_HELPER` pointing at it, and network access:
    ///
    /// ```sh
    /// PADDLEBOARD_KRUN_HELPER=$PWD/target/debug/paddleboard-krun-helper \
    ///   cargo test -p paddleboard_container_engine -- --ignored live_builtin
    /// ```
    #[test]
    #[ignore = "requires libkrun + helper + network"]
    // Blocking process use is the point of this test; it never runs on an
    // async executor.
    #[allow(clippy::disallowed_methods)]
    fn live_builtin_microvm_runs_linux_with_workspace_mounted() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("create workspace");
        std::fs::write(workspace.join("marker.txt"), "pb-live-marker").expect("write marker");

        let engine = BuiltInKrunEngine::with_store_root(temp.path().join("store"));
        let prepared = futures::executor::block_on(engine.prepare_exec(ExecRequest {
            image: DEFAULT_SANDBOX_IMAGE.to_string(),
            host_workdir: workspace.clone(),
            command: "uname -a && cat marker.txt && echo vm-wrote-this > from-vm.txt".to_string(),
        }))
        .expect("prepare built-in exec");

        let output = std::process::Command::new("bash")
            .arg("-c")
            .arg(&prepared.shell_command)
            .output()
            .expect("run helper");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "helper failed ({}):\nstdout: {stdout}\nstderr: {stderr}",
            output.status
        );
        assert!(stdout.contains("Linux"), "expected Linux uname, got: {stdout}");
        assert!(stdout.contains("pb-live-marker"), "workspace not mounted: {stdout}");
        assert_eq!(
            std::fs::read_to_string(workspace.join("from-vm.txt"))
                .expect("guest write visible on host")
                .trim(),
            "vm-wrote-this"
        );
    }
}
