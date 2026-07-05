//! Detect whether PaddleBoard's sandbox prerequisites (Podman + gVisor's `runsc`)
//! are installed and reachable, and produce human-readable install guidance when
//! they aren't. Also detects the built-in libkrun microVM tier, which needs no
//! external installs. This is the data layer; UI surfaces (CLI flag, status
//! indicator, modal) live elsewhere.

use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxStatus {
    pub podman: PodmanStatus,
    pub gvisor: GvisorStatus,
    pub builtin: BuiltInStatus,
    /// Availability of Apple's `container` CLI, the macOS-native tier on
    /// macOS 26+ (Apple silicon). [`AppleContainerStatus::Unsupported`] on
    /// every other platform.
    pub apple_container: AppleContainerStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PodmanStatus {
    /// `podman` is not on `$PATH`.
    Missing,
    /// `podman --version` succeeds but `podman info` does not. On macOS this
    /// typically means the backing `podman machine` is not started.
    InstalledNotRunning { version: String },
    /// `podman info` succeeds — the daemon is reachable.
    Ready { version: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GvisorStatus {
    /// `runsc` appears in Podman's registered OCI runtimes.
    Available,
    /// Podman is reachable but `runsc` is not registered as a runtime.
    NotConfigured,
    /// gVisor cannot run on this platform (e.g. Windows).
    NotApplicable { reason: &'static str },
    /// We couldn't determine gVisor's status because Podman itself isn't ready.
    Unknown,
}

/// Availability of PaddleBoard's built-in container tier: libkrun microVMs
/// driven by the `paddleboard-krun-helper` binary. Unlike the Podman tier this
/// requires no user installs, but it does need hardware virtualization (HVF on
/// Apple silicon, KVM on Linux) and the libkrun dylib to be loadable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuiltInStatus {
    /// libkrun and the helper binary were both found; microVMs can run here.
    Available { libkrun: PathBuf, helper: PathBuf },
    /// The platform could run libkrun, but the dylib was not found.
    LibraryMissing,
    /// libkrun is present but the `paddleboard-krun-helper` binary is not
    /// alongside the app (or at `PADDLEBOARD_KRUN_HELPER`).
    HelperMissing { libkrun: PathBuf },
    /// This platform cannot run libkrun microVMs at all.
    Unsupported { reason: &'static str },
}

impl BuiltInStatus {
    pub fn is_available(&self) -> bool {
        matches!(self, BuiltInStatus::Available { .. })
    }

    /// Short human-readable explanation of why the built-in tier is not
    /// available, for gate errors and the prereqs modal.
    pub fn unavailable_reason(&self) -> Option<String> {
        match self {
            BuiltInStatus::Available { .. } => None,
            BuiltInStatus::LibraryMissing => Some(
                "libkrun is not installed (run script/setup-builtin-sandbox or `brew install libkrun`)"
                    .to_string(),
            ),
            BuiltInStatus::HelperMissing { .. } => {
                Some("the paddleboard-krun-helper binary was not found next to the app".to_string())
            }
            BuiltInStatus::Unsupported { reason } => Some((*reason).to_string()),
        }
    }
}

/// Availability of Apple's `container` CLI (github.com/apple/container), the
/// macOS-native container tier. Unlike libkrun this is a CLI shell-out that
/// pulls and runs OCI images itself, but it needs a running background service
/// (`container system start`) to reach its apiserver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppleContainerStatus {
    /// macOS 26+ on Apple silicon, the `container` CLI resolved, and its
    /// apiserver answered a health check — one-shot containers can run here.
    Available { cli: PathBuf },
    /// The CLI is installed but its background service did not answer
    /// (`container system start` has not been run, or the daemon is down).
    ServiceNotRunning { cli: PathBuf },
    /// The platform could run Apple's container tool but the `container` CLI
    /// was not found on `$PATH` or in a well-known install location.
    CliMissing,
    /// This host cannot run Apple's container tool at all: not macOS, macOS
    /// older than 26, or not Apple silicon.
    Unsupported { reason: &'static str },
}

impl AppleContainerStatus {
    pub fn is_available(&self) -> bool {
        matches!(self, AppleContainerStatus::Available { .. })
    }

    /// Short human-readable explanation of why the Apple container tier is not
    /// available, for gate errors and the prereqs modal.
    pub fn unavailable_reason(&self) -> Option<String> {
        match self {
            AppleContainerStatus::Available { .. } => None,
            AppleContainerStatus::ServiceNotRunning { .. } => Some(
                "Apple's container service is not running (run `container system start`)"
                    .to_string(),
            ),
            AppleContainerStatus::CliMissing => {
                Some("Apple's `container` CLI is not installed".to_string())
            }
            AppleContainerStatus::Unsupported { reason } => Some((*reason).to_string()),
        }
    }
}

/// The concrete non-Podman ("Native") backend that a host resolves to when the
/// preferred Podman + gVisor tier is unavailable. Both are strong VM-isolation
/// tiers, so choosing between them is a mechanical capability resolution, not a
/// security-relevant tier switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeBackend {
    /// Apple's `container` CLI (macOS 26+, Apple silicon).
    AppleContainer,
    /// PaddleBoard's bundled libkrun microVM helper.
    BuiltInKrun,
}

/// Resolve the "Native" tier to a concrete backend for `os`, given a probed
/// status. This is the Mac-Native (Option A) resolution: on macOS prefer Apple
/// `container` when available, else fall back to the bundled libkrun tier; on
/// Linux the only native tier is libkrun; Windows has none.
///
/// Pure and OS-parameterized so the resolution matrix is unit-testable on any
/// host without a real macOS-version or CLI probe.
pub fn resolve_native_backend(status: &SandboxStatus, os: Os) -> Option<NativeBackend> {
    match os {
        Os::MacOs => {
            if status.apple_container.is_available() {
                Some(NativeBackend::AppleContainer)
            } else if status.builtin.is_available() {
                Some(NativeBackend::BuiltInKrun)
            } else {
                None
            }
        }
        Os::Linux => status
            .builtin
            .is_available()
            .then_some(NativeBackend::BuiltInKrun),
        Os::Windows | Os::Other => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    MacOs,
    Linux,
    Windows,
    Other,
}

impl Os {
    pub fn detect() -> Os {
        match std::env::consts::OS {
            "macos" => Os::MacOs,
            "linux" => Os::Linux,
            "windows" => Os::Windows,
            _ => Os::Other,
        }
    }
}

impl SandboxStatus {
    /// True when both Podman is reachable and gVisor is registered (or
    /// inherently unavailable on this platform but Podman is fine).
    pub fn is_satisfied(&self) -> bool {
        matches!(self.podman, PodmanStatus::Ready { .. })
            && matches!(
                self.gvisor,
                GvisorStatus::Available | GvisorStatus::NotApplicable { .. }
            )
    }
}

const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// Probe the host for Podman + gVisor availability. Each probe is bounded by
/// a 2-second timeout so a stuck `podman machine` cannot stall the caller.
pub async fn check() -> SandboxStatus {
    let podman_bin = resolve_podman_bin();
    let podman = check_podman(&podman_bin).await;
    let gvisor = check_gvisor(&podman_bin, &podman).await;
    let builtin = check_builtin();
    let apple_container = check_apple_container().await;
    SandboxStatus {
        podman,
        gvisor,
        builtin,
        apple_container,
    }
}

/// Environment override for the libkrun dylib path, mainly for development
/// against a from-source libkrun build.
pub const LIBKRUN_ENV_VAR: &str = "PADDLEBOARD_LIBKRUN";
/// Environment override for the helper binary path.
pub const KRUN_HELPER_ENV_VAR: &str = "PADDLEBOARD_KRUN_HELPER";
pub const KRUN_HELPER_BIN_NAME: &str = "paddleboard-krun-helper";

/// Well-known locations for the libkrun dylib. Ordered: Homebrew (Apple
/// silicon), then generic /usr/local, then Linux distro lib dirs.
fn libkrun_candidate_paths() -> &'static [&'static str] {
    if cfg!(target_os = "macos") {
        &[
            "/opt/homebrew/lib/libkrun.dylib",
            "/usr/local/lib/libkrun.dylib",
        ]
    } else {
        &[
            "/usr/local/lib64/libkrun.so.1",
            "/usr/local/lib/libkrun.so.1",
            "/usr/lib64/libkrun.so.1",
            "/usr/lib/x86_64-linux-gnu/libkrun.so.1",
            "/usr/lib/aarch64-linux-gnu/libkrun.so.1",
            "/usr/lib/libkrun.so.1",
        ]
    }
}

/// Locate the libkrun dylib: `PADDLEBOARD_LIBKRUN` wins, then the first
/// existing well-known path.
pub fn find_libkrun() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(LIBKRUN_ENV_VAR) {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    libkrun_candidate_paths()
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
}

/// Locate libkrunfw (the packaged guest kernel). libkrun loads it by bare
/// soname at VM start, which fails when libkrun itself was dlopen'd by
/// absolute path — so the helper needs libkrunfw's absolute path too, to
/// pre-load it. On Linux the linker resolves it via ldconfig, but probing it
/// costs nothing and keeps detection honest.
pub fn find_libkrunfw() -> Option<PathBuf> {
    let candidates: &[&str] = if cfg!(target_os = "macos") {
        &[
            "/opt/homebrew/lib/libkrunfw.5.dylib",
            "/opt/homebrew/lib/libkrunfw.dylib",
            "/usr/local/lib/libkrunfw.5.dylib",
            "/usr/local/lib/libkrunfw.dylib",
        ]
    } else {
        &[
            "/usr/local/lib64/libkrunfw.so.5",
            "/usr/local/lib/libkrunfw.so.5",
            "/usr/lib64/libkrunfw.so.5",
            "/usr/lib/x86_64-linux-gnu/libkrunfw.so.5",
            "/usr/lib/aarch64-linux-gnu/libkrunfw.so.5",
            "/usr/lib/libkrunfw.so.5",
        ]
    };
    candidates
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
}

/// Locate the `paddleboard-krun-helper` binary: `PADDLEBOARD_KRUN_HELPER`
/// wins, then a sibling of the current executable — which covers both a cargo
/// target dir during development and `Contents/MacOS` inside a bundle.
pub fn find_krun_helper() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(KRUN_HELPER_ENV_VAR) {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    let sibling = std::env::current_exe()
        .ok()?
        .parent()?
        .join(KRUN_HELPER_BIN_NAME);
    sibling.is_file().then_some(sibling)
}

/// Whether this host can run libkrun microVMs at all, independent of what is
/// installed. HVF needs Apple silicon on macOS; Linux needs an accessible
/// /dev/kvm.
fn builtin_platform_support() -> Result<(), &'static str> {
    match Os::detect() {
        Os::MacOs => {
            if std::env::consts::ARCH == "aarch64" {
                Ok(())
            } else {
                Err("the built-in sandbox requires Apple silicon on macOS")
            }
        }
        Os::Linux => {
            // Opening /dev/kvm is the real capability test — it exists but is
            // often not accessible to the user (kvm group membership).
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open("/dev/kvm")
            {
                Ok(_) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    Err("the built-in sandbox requires KVM (/dev/kvm not present)")
                }
                Err(_) => Err("the built-in sandbox requires access to /dev/kvm \
                     (add your user to the kvm group)"),
            }
        }
        Os::Windows => Err("the built-in sandbox is not available on Windows"),
        Os::Other => Err("the built-in sandbox has not been ported to this platform"),
    }
}

/// Probe the built-in libkrun tier. Synchronous — it is only filesystem
/// checks, no subprocesses.
pub fn check_builtin() -> BuiltInStatus {
    if let Err(reason) = builtin_platform_support() {
        return BuiltInStatus::Unsupported { reason };
    }
    let Some(libkrun) = find_libkrun() else {
        return BuiltInStatus::LibraryMissing;
    };
    match find_krun_helper() {
        Some(helper) => BuiltInStatus::Available { libkrun, helper },
        None => BuiltInStatus::HelperMissing { libkrun },
    }
}

/// Environment override for the Apple `container` CLI path, mainly for testing
/// against a from-source build.
pub const CONTAINER_ENV_VAR: &str = "PADDLEBOARD_CONTAINER_CLI";
pub const CONTAINER_BIN_NAME: &str = "container";

/// Minimum macOS major version that ships a `container`-compatible
/// Virtualization stack. Apple's container tool reached v1.0 alongside macOS 26.
#[cfg(target_os = "macos")]
const APPLE_CONTAINER_MIN_MACOS: u32 = 26;

/// Well-known absolute locations for the `container` CLI when it isn't on the
/// resolved `$PATH`. The signed installer pkg lands it in `/usr/local/bin`.
const CONTAINER_FALLBACK_PATHS: &[&str] =
    &["/usr/local/bin/container", "/opt/homebrew/bin/container"];

/// Locate the Apple `container` CLI: `PADDLEBOARD_CONTAINER_CLI` wins, then the
/// bare name on `$PATH`, then the first existing well-known install location.
pub fn find_container_cli() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(CONTAINER_ENV_VAR) {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    if let Some(path) = std::env::var_os("PATH") {
        if let Some(found) = std::env::split_paths(&path)
            .map(|dir| dir.join(CONTAINER_BIN_NAME))
            .find(|candidate| candidate.is_file())
        {
            return Some(found);
        }
    }
    CONTAINER_FALLBACK_PATHS
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
}

/// Pure resolution of the Apple container tier from its three inputs, so the
/// matrix is testable without a real macOS-version or CLI probe. `platform` is
/// the arch/version gate result; `cli` is the resolved binary (if any);
/// `service_reachable` is whether the apiserver answered a health check.
fn resolve_apple_container_status(
    platform: Result<(), &'static str>,
    cli: Option<PathBuf>,
    service_reachable: bool,
) -> AppleContainerStatus {
    if let Err(reason) = platform {
        return AppleContainerStatus::Unsupported { reason };
    }
    let Some(cli) = cli else {
        return AppleContainerStatus::CliMissing;
    };
    if service_reachable {
        AppleContainerStatus::Available { cli }
    } else {
        AppleContainerStatus::ServiceNotRunning { cli }
    }
}

/// Probe availability of Apple's `container` tier. Off macOS this is a synchronous
/// `Unsupported`; on macOS it checks arch + version, resolves the CLI, and health-
/// checks the background service — each subprocess bounded by [`PROBE_TIMEOUT`] so a
/// stopped service cannot stall the caller.
pub async fn check_apple_container() -> AppleContainerStatus {
    #[cfg(target_os = "macos")]
    {
        let platform = apple_container_platform_support().await;
        if platform.is_err() {
            return resolve_apple_container_status(platform, None, false);
        }
        let cli = find_container_cli();
        let service_reachable = match &cli {
            Some(bin) => probe_container_service(bin).await,
            None => false,
        };
        resolve_apple_container_status(platform, cli, service_reachable)
    }
    #[cfg(not(target_os = "macos"))]
    {
        resolve_apple_container_status(
            Err("Apple's container tool is only available on macOS"),
            None,
            false,
        )
    }
}

/// Whether this host can run Apple's container tool at all: macOS 26+ on Apple
/// silicon. Reads the product version via `sw_vers` rather than a compile-time
/// constant so it reflects the OS the app is actually running on.
#[cfg(target_os = "macos")]
async fn apple_container_platform_support() -> Result<(), &'static str> {
    if std::env::consts::ARCH != "aarch64" {
        return Err("Apple's container tool requires Apple silicon");
    }
    match macos_major_version().await {
        Some(major) if major >= APPLE_CONTAINER_MIN_MACOS => Ok(()),
        Some(_) => Err("Apple's container tool requires macOS 26 or newer"),
        None => Err("could not determine the macOS version"),
    }
}

/// Parse the macOS major version from `sw_vers -productVersion` (e.g. `26.5.1`
/// → `26`). Uses the absolute path since a GUI-launched app inherits a minimal
/// `$PATH` that may omit `/usr/bin`.
#[cfg(target_os = "macos")]
async fn macos_major_version() -> Option<u32> {
    let out = run_probe("/usr/bin/sw_vers", &["-productVersion"]).await?;
    out.trim().split('.').next()?.parse().ok()
}

/// Health-check the `container` background service. `container system status`
/// pings the apiserver; a stopped service makes it fail (or hang), and the
/// [`PROBE_TIMEOUT`] wrapper in [`run_probe`] bounds the hang.
#[cfg(target_os = "macos")]
async fn probe_container_service(cli: &std::path::Path) -> bool {
    run_probe(&cli.to_string_lossy(), &["system", "status"])
        .await
        .is_some()
}

async fn check_podman(podman_bin: &str) -> PodmanStatus {
    let version = match run_probe(podman_bin, &["--version"]).await {
        Some(stdout) => stdout.trim().to_string(),
        None => return PodmanStatus::Missing,
    };

    // `podman info` reaches the daemon. On macOS the CLI can be installed while
    // the backing machine is stopped, which is why we treat that case separately.
    match run_probe(podman_bin, &["info", "--format", "json"]).await {
        Some(_) => PodmanStatus::Ready { version },
        None => PodmanStatus::InstalledNotRunning { version },
    }
}

async fn check_gvisor(podman_bin: &str, podman: &PodmanStatus) -> GvisorStatus {
    if Os::detect() == Os::Windows {
        return GvisorStatus::NotApplicable {
            reason: "gVisor only runs on Linux; it is not available on Windows.",
        };
    }

    let info_json = match podman {
        PodmanStatus::Ready { .. } => {
            match run_probe(podman_bin, &["info", "--format", "json"]).await {
                Some(out) => out,
                None => return GvisorStatus::Unknown,
            }
        }
        _ => return GvisorStatus::Unknown,
    };

    // Use untyped JSON navigation: Podman's info schema has shifted across
    // versions and field names like `ociRuntimes` may not survive a future
    // bump. Looking up by string key keeps us resilient to renames of
    // siblings.
    let value: serde_json::Value = match serde_json::from_str(&info_json) {
        Ok(v) => v,
        Err(_) => return GvisorStatus::NotConfigured,
    };
    let has_runsc_in_info = value
        .get("host")
        .and_then(|h| h.get("ociRuntimes"))
        .and_then(|r| r.as_object())
        .is_some_and(|map| map.contains_key("runsc"));

    if has_runsc_in_info {
        return GvisorStatus::Available;
    }

    // PaddleBoard: on macOS, `podman info` runs on the client and doesn't
    // reflect runtimes registered inside the Podman machine VM. Fall back to
    // probing runsc directly inside the VM.
    if run_probe(podman_bin, &["machine", "ssh", "--", "runsc", "--version"])
        .await
        .is_some_and(|out| out.contains("runsc version"))
    {
        return GvisorStatus::Available;
    }

    GvisorStatus::NotConfigured
}

async fn run_probe(cmd: &str, args: &[&str]) -> Option<String> {
    let fut = tokio::process::Command::new(cmd).args(args).output();
    let output = tokio::time::timeout(PROBE_TIMEOUT, fut).await.ok()?.ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

// PaddleBoard: well-known absolute locations where `podman` lands when it isn't
// on the resolved PATH. GUI apps on macOS inherit a minimal PATH, and even after
// `load_login_shell_environment()` the user's login shell may not export
// Podman's dir (the official installer uses /opt/podman/bin; Homebrew uses
// /opt/homebrew/bin on Apple Silicon). Probing these keeps "is Podman installed?"
// from producing a false negative that pushes a user to reinstall what they have.
const PODMAN_FALLBACK_PATHS: &[&str] = &[
    "/opt/podman/bin/podman",   // official Podman macOS installer (podman.io pkg)
    "/opt/homebrew/bin/podman", // Homebrew, Apple Silicon
    "/usr/local/bin/podman",    // Homebrew on Intel / common Linux prefix
    "/usr/bin/podman",          // Linux distro package
];

/// Decide which `podman` invocation to probe with: prefer the bare name when it
/// resolves on `$PATH` (respecting the user's setup), otherwise the first
/// existing well-known absolute path, else fall back to the bare name so the
/// probe still runs and reports `Missing` honestly.
fn choose_podman_bin(on_path: bool, exists: impl Fn(&str) -> bool) -> String {
    if on_path {
        return "podman".to_string();
    }
    for candidate in PODMAN_FALLBACK_PATHS {
        if exists(candidate) {
            return (*candidate).to_string();
        }
    }
    "podman".to_string()
}

fn podman_binary_name() -> &'static str {
    if cfg!(windows) { "podman.exe" } else { "podman" }
}

fn podman_on_path() -> bool {
    std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path).any(|dir| dir.join(podman_binary_name()).is_file())
        })
        .unwrap_or(false)
}

fn resolve_podman_bin() -> String {
    choose_podman_bin(podman_on_path(), |candidate| {
        std::path::Path::new(candidate).is_file()
    })
}

/// PaddleBoard: ensure Podman's install directory is on `$PATH` for this process,
/// so every `podman` invocation finds it — the prereqs probe here, plus the
/// sandbox tool and REPL kernel in other crates that spawn `podman` by bare name.
///
/// GUI apps on macOS inherit a minimal `$PATH`, and even after the login-shell
/// import the user's shell may not export Podman's dir (the official installer
/// uses `/opt/podman/bin`). When `podman` already resolves on `$PATH`, this is a
/// no-op; otherwise it prepends the first well-known install dir that exists.
///
/// Mutates the process environment, so call it exactly once during early startup
/// — right after `load_login_shell_environment` — before any worker that reads
/// `$PATH` is spawned.
pub fn ensure_podman_on_path() {
    if podman_on_path() {
        return;
    }
    let Some(dir) = PODMAN_FALLBACK_PATHS
        .iter()
        .map(std::path::Path::new)
        .find(|path| path.is_file())
        .and_then(|path| path.parent())
    else {
        return;
    };

    let mut entries: Vec<std::path::PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    if entries.iter().any(|entry| entry == dir) {
        return;
    }
    entries.insert(0, dir.to_path_buf());

    if let Ok(joined) = std::env::join_paths(entries) {
        // SAFETY: matches the existing `set_var` in `load_login_shell_environment`
        // — invoked once at early startup, in the same background task, before
        // PATH-dependent workers spawn.
        unsafe { std::env::set_var("PATH", joined) };
    }
}

#[derive(Debug, Clone)]
pub struct InstallInstructions {
    pub title: String,
    pub steps: Vec<InstallStep>,
    pub doc_url: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct InstallStep {
    pub description: String,
    pub command: Option<String>,
    pub command_kind: CommandKind,
}

/// Distinguishes runnable shell commands from text snippets that the user
/// needs to paste into a config file. The UI uses this to decide whether to
/// expose a "Run in Terminal" affordance — pasting a TOML fragment into a
/// shell would just produce errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CommandKind {
    #[default]
    Shell,
    Snippet,
}

impl InstallStep {
    fn note(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            command: None,
            command_kind: CommandKind::Shell,
        }
    }

    fn shell(description: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            command: Some(command.into()),
            command_kind: CommandKind::Shell,
        }
    }

    fn snippet(description: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            command: Some(command.into()),
            command_kind: CommandKind::Snippet,
        }
    }
}

/// Hand-curated guidance for getting the sandbox stack to a working state.
/// `os` is taken as a parameter so tests can exercise each branch deterministically.
pub fn install_instructions(status: &SandboxStatus, os: Os) -> InstallInstructions {
    if status.is_satisfied() {
        return InstallInstructions {
            title: "Sandbox prerequisites are satisfied".to_string(),
            steps: vec![InstallStep::note(
                "Podman is installed and reachable, and gVisor (runsc) is registered. PaddleBoard's sandboxed tools are ready to use.",
            )],
            doc_url: None,
        };
    }

    let mut steps = Vec::new();
    let mut title = "Install sandbox prerequisites".to_string();

    if status.builtin.is_available() {
        steps.push(InstallStep::note(
            "PaddleBoard's built-in microVM sandbox is active on this machine, so sandboxed tools already work with no installs. The steps below set up Podman + gVisor — the preferred, strongest tier.",
        ));
    }

    match (&status.podman, os) {
        (PodmanStatus::Missing, Os::MacOs) => {
            title = "Install Podman on macOS".to_string();
            steps.push(InstallStep::note(
                "PaddleBoard runs sandboxed tools (like the Python REPL and the agent's shell) inside Podman containers. On macOS, Podman runs containers inside a small Linux VM, called a \"machine\".",
            ));
            steps.push(InstallStep::note(
                "If you don't have Homebrew yet, install it first from https://brew.sh — then come back to this dialog.",
            ));
            steps.push(InstallStep::shell(
                "Install the Podman CLI via Homebrew (about 30 seconds).",
                "brew install podman",
            ));
            steps.push(InstallStep::shell(
                "Create the Linux VM that Podman will use. The defaults are fine; the VM uses roughly 2 GB of disk.",
                "podman machine init",
            ));
            steps.push(InstallStep::shell(
                "Start the VM. The first boot takes 15–30 seconds.",
                "podman machine start",
            ));
            steps.push(InstallStep::note(
                "Click Refresh below to confirm PaddleBoard now sees the running Podman daemon.",
            ));
        }
        (PodmanStatus::Missing, Os::Linux) => {
            title = "Install Podman on Linux".to_string();
            steps.push(InstallStep::note(
                "PaddleBoard runs sandboxed tools (like the Python REPL and the agent's shell) inside Podman containers.",
            ));
            steps.push(InstallStep::shell(
                "Install Podman via your distribution's package manager. PaddleBoard picked the command below based on your /etc/os-release.",
                linux_podman_install_command(),
            ));
            steps.push(InstallStep::shell(
                "Confirm the daemon responds. Most distributions need no extra setup — Podman runs rootless out of the box.",
                "podman info",
            ));
            steps.push(InstallStep::note(
                "Click Refresh below to verify PaddleBoard sees Podman.",
            ));
        }
        (PodmanStatus::Missing, Os::Windows) => {
            title = "Install Podman on Windows".to_string();
            steps.push(InstallStep::note(
                "Download and install Podman Desktop from https://podman-desktop.io. After installation, start Podman Desktop at least once so it provisions its WSL backend.",
            ));
            steps.push(InstallStep::note(
                "Note: gVisor is Linux-only, so the strongest sandboxing tier is not available on Windows even with Podman installed. PaddleBoard will fall back to Podman's default runtime.",
            ));
            steps.push(InstallStep::note(
                "Click Refresh below to verify.",
            ));
        }
        (PodmanStatus::Missing, Os::Other) => {
            title = "Install Podman".to_string();
            steps.push(InstallStep::note(
                "PaddleBoard's sandbox stack hasn't been tested on this platform. See https://podman.io/docs/installation for installation guidance for your OS.",
            ));
        }
        (PodmanStatus::InstalledNotRunning { .. }, Os::MacOs) => {
            title = "Start the Podman machine".to_string();
            steps.push(InstallStep::note(
                "Podman is installed but its backing Linux VM (the \"machine\") isn't running. The CLI is reachable; the daemon inside the VM is not.",
            ));
            steps.push(InstallStep::shell(
                "Start the machine. This takes 5–15 seconds.",
                "podman machine start",
            ));
            steps.push(InstallStep::note(
                "If the command above prints \"no machines found\", you need to create one first:",
            ));
            steps.push(InstallStep::shell(
                "Create the Linux VM that Podman will use.",
                "podman machine init",
            ));
            steps.push(InstallStep::note(
                "Click Refresh below once the machine reports as running.",
            ));
        }
        (PodmanStatus::InstalledNotRunning { .. }, Os::Linux) => {
            title = "Start the Podman service".to_string();
            steps.push(InstallStep::note(
                "Podman is installed but the daemon is unreachable. On Linux this usually means the user's Podman socket isn't running.",
            ));
            steps.push(InstallStep::shell(
                "Start (and enable) the rootless Podman socket via systemd.",
                "systemctl --user enable --now podman.socket",
            ));
            steps.push(InstallStep::shell(
                "Verify the daemon is reachable.",
                "podman info",
            ));
            steps.push(InstallStep::note(
                "Click Refresh below to verify.",
            ));
        }
        (PodmanStatus::InstalledNotRunning { .. }, _) => {
            title = "Start the Podman service".to_string();
            steps.push(InstallStep::note(
                "Podman is installed but not reachable. Verify the Podman service is running and your user has permission to talk to it, then click Refresh below.",
            ));
        }
        (PodmanStatus::Ready { .. }, _) => {}
    }

    // Only surface gVisor steps once Podman itself is ready — running them
    // earlier would just confuse a user who hasn't even installed Podman.
    if matches!(status.podman, PodmanStatus::Ready { .. })
        && matches!(status.gvisor, GvisorStatus::NotConfigured)
    {
        title = "Install gVisor (runsc) and register it with Podman".to_string();
        match os {
            Os::Linux => {
                steps.push(InstallStep::note(
                    "gVisor is a user-space kernel that provides stronger isolation than Podman's default runtime. PaddleBoard prefers it when available, and falls back to the default runtime when it isn't.",
                ));
                steps.push(InstallStep::shell(
                    "Download the latest runsc binary for your CPU architecture and install it to /usr/local/bin. Requires sudo.",
                    "curl -fsSL https://storage.googleapis.com/gvisor/releases/release/latest/$(uname -m)/runsc -o runsc \\\n  && chmod +x runsc \\\n  && sudo mv runsc /usr/local/bin/runsc",
                ));
                steps.push(InstallStep::snippet(
                    "Register runsc as a Podman runtime. Add the snippet below to ~/.config/containers/containers.conf (create the file if it doesn't exist):",
                    "[engine.runtimes]\nrunsc = [\"/usr/local/bin/runsc\"]",
                ));
                steps.push(InstallStep::note(
                    "Click Refresh below to verify runsc is registered.",
                ));
            }
            Os::MacOs => {
                steps.push(InstallStep::note(
                    "On macOS, containers run inside the Podman machine VM, so gVisor must be installed inside that VM — not on macOS itself. The next steps move you into the VM.",
                ));
                steps.push(InstallStep::shell(
                    "SSH into the Podman machine. This opens an interactive shell inside the VM.",
                    "podman machine ssh",
                ));
                steps.push(InstallStep::snippet(
                    "Inside the VM, download runsc to /usr/local/bin. (The VM is Fedora-based and requires sudo. Paste this into the SSH session.)",
                    "sudo curl -fsSL https://storage.googleapis.com/gvisor/releases/release/latest/$(uname -m)/runsc -o /usr/local/bin/runsc \\\n  && sudo chmod +x /usr/local/bin/runsc",
                ));
                steps.push(InstallStep::snippet(
                    "Still inside the VM, register runsc as a Podman runtime. Append the snippet below to /etc/containers/containers.conf (use sudo):",
                    "[engine.runtimes]\nrunsc = [\"/usr/local/bin/runsc\"]",
                ));
                steps.push(InstallStep::note(
                    "Type `exit` to leave the VM, then click Refresh below to verify.",
                ));
            }
            _ => {}
        }
    }

    InstallInstructions {
        title,
        steps,
        doc_url: Some("https://gvisor.dev/docs/user_guide/install/"),
    }
}

fn linux_podman_install_command() -> String {
    let id = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|contents| {
            contents.lines().find_map(|line| {
                line.strip_prefix("ID=")
                    .map(|value| value.trim_matches('"').to_string())
            })
        });
    match id.as_deref() {
        Some("ubuntu") | Some("debian") | Some("pop") | Some("linuxmint") => {
            "sudo apt update && sudo apt install -y podman".to_string()
        }
        Some("fedora") | Some("rhel") | Some("centos") | Some("rocky") | Some("alma") => {
            "sudo dnf install -y podman".to_string()
        }
        Some("arch") | Some("manjaro") | Some("endeavouros") => {
            "sudo pacman -S --noconfirm podman".to_string()
        }
        Some("opensuse-leap") | Some("opensuse-tumbleweed") | Some("sles") => {
            "sudo zypper install -y podman".to_string()
        }
        _ => "Use your distribution's package manager to install podman.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_builtin() -> BuiltInStatus {
        BuiltInStatus::Unsupported {
            reason: "test platform",
        }
    }

    fn no_apple_container() -> AppleContainerStatus {
        AppleContainerStatus::Unsupported {
            reason: "test platform",
        }
    }

    #[test]
    fn is_satisfied_requires_both_podman_and_gvisor() {
        let happy = SandboxStatus {
            podman: PodmanStatus::Ready {
                version: "podman version 5.0.0".into(),
            },
            gvisor: GvisorStatus::Available,
            builtin: no_builtin(),
            apple_container: no_apple_container(),
        };
        assert!(happy.is_satisfied());

        let no_podman = SandboxStatus {
            podman: PodmanStatus::Missing,
            gvisor: GvisorStatus::Available,
            builtin: no_builtin(),
            apple_container: no_apple_container(),
        };
        assert!(!no_podman.is_satisfied());

        let no_gvisor = SandboxStatus {
            podman: PodmanStatus::Ready {
                version: "podman version 5.0.0".into(),
            },
            gvisor: GvisorStatus::NotConfigured,
            builtin: no_builtin(),
            apple_container: no_apple_container(),
        };
        assert!(!no_gvisor.is_satisfied());

        // Windows: NotApplicable still counts as satisfied if podman is ready,
        // because the user has done everything they can on this platform.
        let windows_ready = SandboxStatus {
            podman: PodmanStatus::Ready {
                version: "podman version 5.0.0".into(),
            },
            gvisor: GvisorStatus::NotApplicable {
                reason: "gVisor only runs on Linux; it is not available on Windows.",
            },
            builtin: no_builtin(),
            apple_container: no_apple_container(),
        };
        assert!(windows_ready.is_satisfied());
    }

    #[test]
    fn missing_podman_gives_install_steps() {
        let status = SandboxStatus {
            podman: PodmanStatus::Missing,
            gvisor: GvisorStatus::Unknown,
            builtin: no_builtin(),
            apple_container: no_apple_container(),
        };
        let instructions = install_instructions(&status, Os::MacOs);
        assert!(instructions.title.contains("Podman"));
        assert!(instructions.steps.iter().any(|step| step
            .command
            .as_deref()
            .is_some_and(|cmd| cmd.contains("brew install podman"))));
    }

    #[test]
    fn builtin_available_adds_no_install_needed_note() {
        let status = SandboxStatus {
            podman: PodmanStatus::Missing,
            gvisor: GvisorStatus::Unknown,
            builtin: BuiltInStatus::Available {
                libkrun: PathBuf::from("/opt/homebrew/lib/libkrun.dylib"),
                helper: PathBuf::from("/tmp/paddleboard-krun-helper"),
            },
            apple_container: no_apple_container(),
        };
        let instructions = install_instructions(&status, Os::MacOs);
        assert!(
            instructions
                .steps
                .first()
                .is_some_and(|step| step.description.contains("built-in microVM sandbox"))
        );
        // Podman steps still follow — it remains the preferred tier.
        assert!(instructions.steps.iter().any(|step| step
            .command
            .as_deref()
            .is_some_and(|cmd| cmd.contains("brew install podman"))));
    }

    #[test]
    fn builtin_unavailable_reasons_are_present_for_all_gap_states() {
        assert!(
            BuiltInStatus::Available {
                libkrun: PathBuf::from("/l"),
                helper: PathBuf::from("/h"),
            }
            .unavailable_reason()
            .is_none()
        );
        assert!(BuiltInStatus::LibraryMissing.unavailable_reason().is_some());
        assert!(
            BuiltInStatus::HelperMissing {
                libkrun: PathBuf::from("/l")
            }
            .unavailable_reason()
            .is_some()
        );
        assert!(
            BuiltInStatus::Unsupported { reason: "nope" }
                .unavailable_reason()
                .is_some()
        );
    }

    #[test]
    fn podman_on_path_uses_bare_name() {
        // When podman resolves on PATH we must not second-guess it with absolute
        // fallbacks — respect the user's environment.
        let bin = choose_podman_bin(true, |_| panic!("should not probe fallbacks"));
        assert_eq!(bin, "podman");
    }

    #[test]
    fn podman_off_path_falls_back_to_install_location() {
        // The macOS installer case that motivated this: not on PATH, but present
        // at /opt/podman/bin/podman.
        let bin = choose_podman_bin(false, |candidate| candidate == "/opt/podman/bin/podman");
        assert_eq!(bin, "/opt/podman/bin/podman");
    }

    #[test]
    fn podman_off_path_prefers_earliest_existing_fallback() {
        // Both brew and the installer present → installer path wins by ordering.
        let bin = choose_podman_bin(false, |candidate| {
            candidate == "/opt/podman/bin/podman" || candidate == "/opt/homebrew/bin/podman"
        });
        assert_eq!(bin, "/opt/podman/bin/podman");
    }

    #[test]
    fn podman_truly_absent_falls_back_to_bare_name() {
        // Nothing anywhere → bare name so the probe still runs and reports Missing.
        let bin = choose_podman_bin(false, |_| false);
        assert_eq!(bin, "podman");
    }

    #[test]
    fn satisfied_status_returns_short_confirmation() {
        let status = SandboxStatus {
            podman: PodmanStatus::Ready {
                version: "podman version 5.0.0".into(),
            },
            gvisor: GvisorStatus::Available,
            builtin: no_builtin(),
            apple_container: no_apple_container(),
        };
        let instructions = install_instructions(&status, Os::Linux);
        assert_eq!(instructions.steps.len(), 1);
        assert!(instructions.title.contains("satisfied"));
    }

    fn status_with(builtin: BuiltInStatus, apple: AppleContainerStatus) -> SandboxStatus {
        SandboxStatus {
            podman: PodmanStatus::Missing,
            gvisor: GvisorStatus::Unknown,
            builtin,
            apple_container: apple,
        }
    }

    fn apple_available() -> AppleContainerStatus {
        AppleContainerStatus::Available {
            cli: PathBuf::from("/usr/local/bin/container"),
        }
    }

    fn krun_available() -> BuiltInStatus {
        BuiltInStatus::Available {
            libkrun: PathBuf::from("/opt/homebrew/lib/libkrun.dylib"),
            helper: PathBuf::from("/tmp/paddleboard-krun-helper"),
        }
    }

    #[test]
    fn mac_native_prefers_apple_container_when_available() {
        let status = status_with(krun_available(), apple_available());
        assert_eq!(
            resolve_native_backend(&status, Os::MacOs),
            Some(NativeBackend::AppleContainer)
        );
    }

    #[test]
    fn mac_native_falls_back_to_libkrun_without_apple_container() {
        // macOS 13–25 (or macOS 26 without the CLI): Apple container unavailable,
        // libkrun present → the older-macOS path from PR #43.
        let status = status_with(krun_available(), no_apple_container());
        assert_eq!(
            resolve_native_backend(&status, Os::MacOs),
            Some(NativeBackend::BuiltInKrun)
        );
    }

    #[test]
    fn mac_native_is_none_when_neither_backend_is_available() {
        let status = status_with(no_builtin(), no_apple_container());
        assert_eq!(resolve_native_backend(&status, Os::MacOs), None);
    }

    #[test]
    fn linux_native_is_libkrun_and_ignores_apple_container() {
        // Apple container never resolves off macOS even if its status somehow
        // claimed availability; Linux "Native" is libkrun only.
        let status = status_with(krun_available(), apple_available());
        assert_eq!(
            resolve_native_backend(&status, Os::Linux),
            Some(NativeBackend::BuiltInKrun)
        );
        let status = status_with(no_builtin(), apple_available());
        assert_eq!(resolve_native_backend(&status, Os::Linux), None);
    }

    #[test]
    fn windows_and_other_have_no_native_backend() {
        let status = status_with(krun_available(), apple_available());
        assert_eq!(resolve_native_backend(&status, Os::Windows), None);
        assert_eq!(resolve_native_backend(&status, Os::Other), None);
    }

    #[test]
    fn apple_container_status_resolves_from_probe_inputs() {
        // Unsupported platform short-circuits before the CLI is even consulted.
        assert!(matches!(
            resolve_apple_container_status(Err("old macOS"), None, false),
            AppleContainerStatus::Unsupported { .. }
        ));
        // Supported platform, no CLI → CliMissing.
        assert_eq!(
            resolve_apple_container_status(Ok(()), None, false),
            AppleContainerStatus::CliMissing
        );
        // CLI present but the apiserver did not answer → ServiceNotRunning.
        let cli = PathBuf::from("/usr/local/bin/container");
        assert_eq!(
            resolve_apple_container_status(Ok(()), Some(cli.clone()), false),
            AppleContainerStatus::ServiceNotRunning { cli: cli.clone() }
        );
        // CLI present and service reachable → Available.
        assert_eq!(
            resolve_apple_container_status(Ok(()), Some(cli.clone()), true),
            AppleContainerStatus::Available { cli }
        );
    }

    #[test]
    fn apple_container_unavailable_reasons_present_for_all_gap_states() {
        assert!(apple_available().unavailable_reason().is_none());
        assert!(
            AppleContainerStatus::ServiceNotRunning {
                cli: PathBuf::from("/usr/local/bin/container")
            }
            .unavailable_reason()
            .is_some()
        );
        assert!(
            AppleContainerStatus::CliMissing
                .unavailable_reason()
                .is_some()
        );
        assert!(
            AppleContainerStatus::Unsupported { reason: "nope" }
                .unavailable_reason()
                .is_some()
        );
    }
}
