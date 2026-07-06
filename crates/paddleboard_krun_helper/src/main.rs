//! `paddleboard-krun-helper` — boots a libkrun microVM around a single shell
//! command. Spawned by `paddleboard_container_engine`'s built-in tier; not
//! meant to be run by hand.
//!
//! A separate binary exists because `krun_start_enter` consumes the calling
//! process (it becomes the VMM and never returns), so the main PaddleBoard
//! process can't call libkrun directly — the same pattern krunvm uses.
//!
//! libkrun is loaded with `dlopen` rather than linked at build time so the
//! PaddleBoard workspace builds and runs on machines without libkrun; the
//! engine only routes work here after runtime detection finds the dylib. The
//! FFI signatures below are pinned to the stable libkrun 1.x ABI (libkrun.h,
//! v1.10) — 2.x is not supported.
//!
//! On macOS this binary must be signed with the `com.apple.security.hypervisor`
//! entitlement (see resources/paddleboard-krun-helper.entitlements and
//! script/setup-builtin-sandbox), or Hypervisor.framework refuses to create
//! the VM.

use anyhow::{Context as _, Result, anyhow, bail};
use clap::Parser;
use std::ffi::{CString, c_char};
use std::path::PathBuf;

/// Tag under which the host workspace directory is exposed to the guest as a
/// virtio-fs device; the guest-side init mounts it at `--guest-workdir`.
const WORKSPACE_TAG: &str = "pbworkspace";

/// Guest-side bootstrap, run by /bin/sh. Mounts the workspace virtio-fs
/// device (when present), then hands off to bash exactly like the Podman
/// tier does.
///
/// Constraints that shaped this: libkrun transports exec/env config over the
/// kernel command line, which is printable-ASCII-only — so this must be a
/// single ASCII line, and the user's (arbitrary, possibly multi-line/UTF-8)
/// command cannot ride in argv or the environment at all. Instead the engine
/// writes it into the ephemeral rootfs at [`GUEST_COMMAND_FILE`] and the
/// bootstrap executes that file.
const GUEST_INIT: &str = r#"if [ -n "$PB_WS_TAG" ]; then mkdir -p "$PB_GUEST_WORKDIR" && mount -t virtiofs "$PB_WS_TAG" "$PB_GUEST_WORKDIR" && cd "$PB_GUEST_WORKDIR" || exit 125; fi; exec /bin/bash "$PB_CMD_FILE""#;

#[derive(Parser)]
#[command(about = "Runs a command inside a libkrun microVM (internal PaddleBoard helper)")]
struct Args {
    /// Path to the libkrun dylib to load.
    #[arg(long)]
    libkrun: PathBuf,
    /// Host directory used as the guest's root filesystem (via virtio-fs).
    #[arg(long)]
    root: PathBuf,
    /// Host directory exposed to the guest at --guest-workdir.
    #[arg(long)]
    workspace: Option<PathBuf>,
    /// Guest mount point and working directory for --workspace.
    #[arg(long, default_value = "/workspace")]
    guest_workdir: String,
    /// Number of guest vCPUs.
    #[arg(long, default_value_t = 2)]
    cpus: u8,
    /// Guest RAM in MiB.
    #[arg(long, default_value_t = 2048)]
    mem_mib: u32,
    /// libkrun log level (0 = off .. 5 = trace).
    #[arg(long, default_value_t = 1)]
    krun_log_level: u32,
    /// Guest path of the shell script to run via bash. The caller must have
    /// written it into the rootfs; it cannot be passed inline because
    /// libkrun's config transport is ASCII-only (see GUEST_INIT).
    #[arg(long)]
    guest_command_file: String,
}

/// The libkrun 1.x entry points this helper needs, resolved from the dylib.
struct Krun<'lib> {
    set_log_level: libloading::Symbol<'lib, unsafe extern "C" fn(u32) -> i32>,
    create_ctx: libloading::Symbol<'lib, unsafe extern "C" fn() -> i32>,
    set_vm_config: libloading::Symbol<'lib, unsafe extern "C" fn(u32, u8, u32) -> i32>,
    set_root: libloading::Symbol<'lib, unsafe extern "C" fn(u32, *const c_char) -> i32>,
    add_virtiofs:
        libloading::Symbol<'lib, unsafe extern "C" fn(u32, *const c_char, *const c_char) -> i32>,
    set_workdir: libloading::Symbol<'lib, unsafe extern "C" fn(u32, *const c_char) -> i32>,
    set_exec: libloading::Symbol<
        'lib,
        unsafe extern "C" fn(
            u32,
            *const c_char,
            *const *const c_char,
            *const *const c_char,
        ) -> i32,
    >,
    start_enter: libloading::Symbol<'lib, unsafe extern "C" fn(u32) -> i32>,
}

impl<'lib> Krun<'lib> {
    fn load(library: &'lib libloading::Library) -> Result<Self> {
        // SAFETY: signatures match the libkrun 1.x C header (libkrun.h v1.10).
        unsafe {
            Ok(Self {
                set_log_level: library.get(b"krun_set_log_level\0")?,
                create_ctx: library.get(b"krun_create_ctx\0")?,
                set_vm_config: library.get(b"krun_set_vm_config\0")?,
                set_root: library.get(b"krun_set_root\0")?,
                add_virtiofs: library.get(b"krun_add_virtiofs\0")?,
                set_workdir: library.get(b"krun_set_workdir\0")?,
                set_exec: library.get(b"krun_set_exec\0")?,
                start_enter: library.get(b"krun_start_enter\0")?,
            })
        }
    }
}

fn main() {
    let args = Args::parse();
    // On success `run` never returns: krun_start_enter turns this process
    // into the VM, and the process exits with the guest command's status.
    let error = match run(args) {
        Ok(never) => match never {},
        Err(error) => error,
    };
    eprintln!("paddleboard-krun-helper: {error:#}");
    std::process::exit(70); // EX_SOFTWARE, distinguishable from guest statuses
}

enum Never {}

/// Turn a libkrun return code (0 or a negative errno) into a Result.
fn check(call: &str, code: i32) -> Result<i32> {
    if code < 0 {
        Err(anyhow!(
            "{call} failed: {}",
            std::io::Error::from_raw_os_error(-code)
        ))
    } else {
        Ok(code)
    }
}

fn cstring(value: impl Into<Vec<u8>>) -> Result<CString> {
    CString::new(value).context("argument contains an interior NUL byte")
}

fn run(args: Args) -> Result<Never> {
    if !args.root.is_dir() {
        bail!("--root {:?} is not a directory", args.root);
    }

    // Note for callers: libkrun dlopens libkrunfw (the guest kernel) by bare
    // soname at krun_start_enter, and dyld only honors search-path variables
    // set before process launch — so this helper must be spawned with
    // DYLD_FALLBACK_LIBRARY_PATH (macOS) covering libkrunfw's directory.
    // The engine's generated command does that.
    let library = unsafe { libloading::Library::new(&args.libkrun) }
        .with_context(|| format!("failed to load libkrun from {:?}", args.libkrun))?;
    let krun = Krun::load(&library)?;

    let root = cstring(args.root.to_string_lossy().into_owned())?;
    let workspace_tag = cstring(WORKSPACE_TAG)?;
    let guest_root_workdir = cstring("/")?;
    let exec_path = cstring("/bin/sh")?;

    let mut env: Vec<CString> = vec![
        cstring("PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin")?,
        cstring("HOME=/root")?,
        cstring("TERM=xterm-256color")?,
        cstring(format!("PB_CMD_FILE={}", args.guest_command_file))?,
        cstring(format!("PB_GUEST_WORKDIR={}", args.guest_workdir))?,
    ];

    // SAFETY: all pointers passed below are into CStrings (and arrays of
    // them) that live until krun_start_enter, which never returns.
    unsafe {
        check("krun_set_log_level", (krun.set_log_level)(args.krun_log_level))?;
        let ctx = check("krun_create_ctx", (krun.create_ctx)())? as u32;
        check(
            "krun_set_vm_config",
            (krun.set_vm_config)(ctx, args.cpus, args.mem_mib),
        )?;
        check("krun_set_root", (krun.set_root)(ctx, root.as_ptr()))?;

        if let Some(workspace) = &args.workspace {
            let workspace = cstring(workspace.to_string_lossy().into_owned())?;
            check(
                "krun_add_virtiofs",
                (krun.add_virtiofs)(ctx, workspace_tag.as_ptr(), workspace.as_ptr()),
            )?;
            env.push(cstring(format!("PB_WS_TAG={WORKSPACE_TAG}"))?);
        }

        // The guest init script cds into the workspace itself (the mount
        // point doesn't exist until it runs), so start at /.
        check(
            "krun_set_workdir",
            (krun.set_workdir)(ctx, guest_root_workdir.as_ptr()),
        )?;

        let argv_storage = [cstring("-c")?, cstring(GUEST_INIT)?];
        let mut argv: Vec<*const c_char> =
            argv_storage.iter().map(|arg| arg.as_ptr()).collect();
        argv.push(std::ptr::null());
        let mut envp: Vec<*const c_char> = env.iter().map(|var| var.as_ptr()).collect();
        envp.push(std::ptr::null());

        check(
            "krun_set_exec",
            (krun.set_exec)(ctx, exec_path.as_ptr(), argv.as_ptr(), envp.as_ptr()),
        )?;

        let code = (krun.start_enter)(ctx);
        // Reached only on failure — on success the process became the VM.
        check("krun_start_enter", code)?;
        bail!("krun_start_enter returned unexpectedly with {code}");
    }
}
