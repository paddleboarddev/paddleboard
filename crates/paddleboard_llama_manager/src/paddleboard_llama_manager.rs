//! PaddleBoard-managed local inference.
//!
//! This crate is the runtime *manager* that makes "Local Models" a zero-install
//! experience: it provisions a pinned `llama-server` binary, downloads a curated
//! GGUF model with progress, and supervises the server on an ephemeral loopback
//! port. The existing `llama.cpp` language-model provider is a pure HTTP client,
//! so once the managed server is listening the provider discovers the model with
//! no additional LLM code — the whole feature is assets plus process lifecycle.

mod download;
mod server;
pub mod ui;

use anyhow::{Context as _, Result};
use download::download_file_with_progress;
use futures::AsyncReadExt as _;
use gpui::{App, AppContext as _, AsyncApp, Context, Entity, Global, Task, WeakEntity};
use http_client::HttpClient;
use server::{READINESS_POLL_INTERVAL, READINESS_TIMEOUT, ServerHandle};
use sha2::{Digest as _, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The pinned llama.cpp release. Bundled binaries and the dev-mode install both
/// track this tag; see `script/fetch-llama-server`.
///
// TODO(local-models): phase 2 should drive this from a signed manifest fetched
// at runtime so security patches can ship without a full app release. Until then
// the bundled binary is patched by cutting a new app release (see PR notes on the
// llama.cpp security-advisory maintenance obligation).
const LLAMA_CPP_VERSION: &str = "b9874";

/// Context window the managed server is launched with. Modest by default so a
/// cold start stays fast and the KV cache fits comfortably in RAM; users who
/// point the provider at their own server are unaffected.
const DEFAULT_CONTEXT_SIZE: u32 = 8192;

/// Context window for the managed embedding server. EmbeddingGemma's maximum
/// input length is 2048 tokens; chunks are sized well below this anyway.
const EMBEDDING_CONTEXT_SIZE: u32 = 2048;

// ---------------------------------------------------------------------------
// Model catalog
// ---------------------------------------------------------------------------

/// A curated, openly-licensed model that PaddleBoard can download and run. Kept
/// deliberately tiny in phase 1; arbitrary Hugging Face models come later.
#[derive(Clone, Copy, Debug)]
pub struct CatalogModel {
    /// Stable id persisted in settings.
    pub id: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    /// Hugging Face `owner/repo` holding the GGUF.
    pub repo: &'static str,
    /// GGUF file name within the repo.
    pub file: &'static str,
    pub size_bytes: u64,
    pub sha256: &'static str,
}

impl CatalogModel {
    pub fn download_url(&self) -> String {
        format!(
            "https://huggingface.co/{}/resolve/main/{}",
            self.repo, self.file
        )
    }

    /// Approximate on-disk size in gigabytes, for display.
    pub fn approx_size_gb(&self) -> f32 {
        self.size_bytes as f32 / 1_000_000_000.0
    }
}

/// The default managed model: a capable general-purpose choice.
pub const DEFAULT_MODEL_ID: &str = "gemma-3-4b";

/// The curated catalog. Both entries are Gemma 3 (Q4_K_M) GGUFs from openly
/// redistributable, ungated mirrors, so no Hugging Face auth is required.
pub const CATALOG: &[CatalogModel] = &[
    CatalogModel {
        id: "gemma-3-4b",
        display_name: "Gemma 3 4B",
        description: "Google's Gemma 3 4B (4-bit). A capable general-purpose model. ~2.5 GB download.",
        repo: "unsloth/gemma-3-4b-it-GGUF",
        file: "gemma-3-4b-it-Q4_K_M.gguf",
        size_bytes: 2_489_894_016,
        sha256: "04a43a22e8d2003deda5acc262f68ec1005fa76c735a9962a8c77042a74a7d19",
    },
    CatalogModel {
        id: "gemma-3-1b",
        display_name: "Gemma 3 1B (tiny)",
        description: "Google's Gemma 3 1B (4-bit). Smallest and fastest; best for low-RAM machines. ~0.8 GB download.",
        repo: "unsloth/gemma-3-1b-it-GGUF",
        file: "gemma-3-1b-it-Q4_K_M.gguf",
        size_bytes: 806_058_272,
        sha256: "8270790f3ab69fdfe860b7b64008d9a19986d8df7e407bb018184caa08798ebd",
    },
];

pub fn catalog_model(id: &str) -> Option<&'static CatalogModel> {
    CATALOG.iter().find(|model| model.id == id)
}

/// The default managed embedding model.
pub const DEFAULT_EMBEDDING_MODEL_ID: &str = "embeddinggemma-300m";

/// Embedding models, kept separate from `CATALOG` so the chat-model picker
/// never offers a model that can't chat. Consumed by the semantic-search
/// indexer (paddleboard_rag), not by the Local Models UI.
pub const EMBEDDING_CATALOG: &[CatalogModel] = &[CatalogModel {
    id: "embeddinggemma-300m",
    display_name: "EmbeddingGemma 300M",
    description: "Google's EmbeddingGemma (QAT, 8-bit) for local semantic search. \
                  ~0.33 GB download.",
    repo: "ggml-org/embeddinggemma-300m-qat-q8_0-GGUF",
    file: "embeddinggemma-300m-qat-Q8_0.gguf",
    size_bytes: 328_577_056,
    sha256: "6fa0c02a9c302be6f977521d399b4de3a46310a4f2621ee0063747881b673f67",
}];

pub fn embedding_catalog_model(id: &str) -> Option<&'static CatalogModel> {
    EMBEDDING_CATALOG.iter().find(|model| model.id == id)
}

/// The default embedding model, tolerating a mistyped id like [`default_model`].
pub fn default_embedding_model() -> &'static CatalogModel {
    embedding_catalog_model(DEFAULT_EMBEDDING_MODEL_ID)
        .or(EMBEDDING_CATALOG.first())
        .expect("the embedding catalog is never empty")
}

/// The default model, falling back to the first catalog entry if the default id
/// is ever mistyped. The catalog is a non-empty compile-time constant.
pub fn default_model() -> &'static CatalogModel {
    catalog_model(DEFAULT_MODEL_ID)
        .or(CATALOG.first())
        .expect("the model catalog is never empty")
}

/// Directory holding downloaded GGUF files.
pub fn models_dir() -> PathBuf {
    paths::data_dir().join("llama_cpp").join("models")
}

fn model_path(model: &CatalogModel) -> PathBuf {
    models_dir().join(model.file)
}

/// Synchronous best-effort "already downloaded?" check for rendering. A file is
/// considered present only when its size matches, so a truncated transfer is not
/// mistaken for a complete one.
pub fn model_is_downloaded(model: &CatalogModel) -> bool {
    match std::fs::metadata(model_path(model)) {
        Ok(metadata) => metadata.len() == model.size_bytes,
        Err(_) => false,
    }
}

async fn model_present(path: &Path, expected_size: u64) -> bool {
    match smol::fs::metadata(path).await {
        Ok(metadata) => metadata.len() == expected_size,
        Err(_) => false,
    }
}

/// Whether a cached model file is present, the right size, and matches its
/// pinned SHA-256. A size-only check can be fooled by a truncated-then-padded or
/// otherwise corrupt file of the right length, so the digest is re-verified
/// before the file is handed to `llama-server`.
async fn verify_cached_model(path: &Path, model: &CatalogModel) -> Result<bool> {
    if !model_present(path, model.size_bytes).await {
        return Ok(false);
    }
    let digest = sha256_file(path).await?;
    Ok(digest.eq_ignore_ascii_case(model.sha256))
}

/// Streams `path` and returns its lowercase-hex SHA-256. Hashing a multi-GB GGUF
/// takes a couple of seconds — dwarfed by the model-load time — so verifying a
/// cached file before launch is cheap insurance against a corrupt cache.
async fn sha256_file(path: &Path) -> Result<String> {
    let mut file = smol::fs::File::open(path)
        .await
        .with_context(|| format!("opening {path:?} to verify its checksum"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 128 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .await
            .with_context(|| format!("reading {path:?} to verify its checksum"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

// ---------------------------------------------------------------------------
// Binary provisioning
// ---------------------------------------------------------------------------

struct ServerAsset {
    url: &'static str,
    sha256: &'static str,
}

fn server_bin_name() -> &'static str {
    if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    }
}

/// The pinned llama.cpp release asset for the current platform, or `None` where
/// Local Models are not supported yet (Windows and Intel macOS in phase 1).
fn current_target_asset() -> Option<ServerAsset> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return Some(ServerAsset {
        url: "https://github.com/ggml-org/llama.cpp/releases/download/b9874/llama-b9874-bin-macos-arm64.tar.gz",
        sha256: "6ad88c0f70c4731200e514132043b08894238beebf1a8e80e2b14a0ebecd1cb8",
    });
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return Some(ServerAsset {
        url: "https://github.com/ggml-org/llama.cpp/releases/download/b9874/llama-b9874-bin-ubuntu-x64.tar.gz",
        sha256: "5a3304b45428c12e8a81709b741d3770fa10d333d663c3c8039456fa9dd447bd",
    });
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    return Some(ServerAsset {
        url: "https://github.com/ggml-org/llama.cpp/releases/download/b9874/llama-b9874-bin-ubuntu-arm64.tar.gz",
        sha256: "33ad52ddaac26ffc965d41a4a485346ad57aa1a08c22916a47637dc273f007ec",
    });
    #[cfg(not(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
    )))]
    return None;
}

/// Whether the current platform can run Local Models in phase 1.
pub fn platform_supported() -> bool {
    current_target_asset().is_some()
}

/// Locate a bundled `llama-server`: an explicit override wins, then a `llama/`
/// directory beside the executable (macOS bundle + dev builds), then the Linux
/// bundle's `libexec/llama/`.
fn bundled_server_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PADDLEBOARD_LLAMA_SERVER") {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;
    let name = server_bin_name();

    let sibling = exe_dir.join("llama").join(name);
    if sibling.is_file() {
        return Some(sibling);
    }
    if let Some(app_root) = exe_dir.parent() {
        let libexec = app_root.join("libexec").join("llama").join(name);
        if libexec.is_file() {
            return Some(libexec);
        }
    }
    None
}

/// Resolve the `llama-server` binary, installing the pinned release into the data
/// directory when the app is not bundled (so `cargo run` works). The download is
/// SHA-256 verified against the pin before it is used.
pub async fn resolve_or_install_server(http_client: &dyn HttpClient) -> Result<PathBuf> {
    if let Some(path) = bundled_server_path() {
        return Ok(path);
    }
    let Some(asset) = current_target_asset() else {
        anyhow::bail!("Local Models are not supported on this platform yet");
    };

    let containing_dir = paths::data_dir().join("llama_cpp");
    let install_dir = containing_dir.join(LLAMA_CPP_VERSION);
    // The release archive unpacks into a `llama-<version>/` directory.
    let extracted_dir = install_dir.join(format!("llama-{LLAMA_CPP_VERSION}"));
    let server_path = extracted_dir.join(server_bin_name());
    if server_path.is_file() {
        return Ok(server_path);
    }

    smol::fs::create_dir_all(&containing_dir)
        .await
        .with_context(|| format!("creating {containing_dir:?}"))?;
    log::info!("installing llama.cpp {LLAMA_CPP_VERSION} into {install_dir:?}");
    http_client::github_download::download_server_binary(
        http_client,
        asset.url,
        Some(asset.sha256),
        &install_dir,
        http_client::github::AssetKind::TarGz,
    )
    .await
    .context("downloading the llama.cpp server release")?;

    anyhow::ensure!(
        server_path.is_file(),
        "llama-server missing after extracting {}",
        asset.url
    );
    util::fs::make_file_executable(&server_path)
        .await
        .with_context(|| format!("making {server_path:?} executable"))?;
    Ok(server_path)
}

// ---------------------------------------------------------------------------
// Manager entity
// ---------------------------------------------------------------------------

/// Observable lifecycle state of the managed local server, surfaced to the UI.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum ManagerStatus {
    /// Managed mode is off; nothing is running.
    #[default]
    Idle,
    /// Provisioning the `llama-server` binary.
    Preparing,
    /// Downloading the model weights.
    Downloading {
        model: String,
        received: u64,
        total: Option<u64>,
    },
    /// Server spawned; waiting for the model to finish loading.
    Starting { model: String },
    /// Server is up and serving on `port`.
    Ready { model: String, port: u16 },
    /// Something failed; `message` is user-facing.
    Error { message: String },
    /// This platform can't run Local Models in phase 1.
    Unsupported,
}

/// Which managed server a lifecycle operation targets. The chat and embedding
/// servers run independently: separate processes, ports, status machines, and
/// generations, sharing only the provisioning/download plumbing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ServerSlot {
    Chat,
    Embedding,
}

pub struct LlamaManager {
    http_client: Arc<dyn HttpClient>,
    status: ManagerStatus,
    /// The model id the user wants running, or `None` when managed mode is off.
    desired: Option<String>,
    /// The model the picker is set to, mirrored from settings even while managed
    /// mode is off, so the UI can show the selection independent of run state.
    selected_model: String,
    /// The running server. Dropping it kills the process (see `ServerHandle`).
    server: Option<ServerHandle>,
    /// The active provisioning/supervision task. Held so it is cancelled when a
    /// newer run supersedes it.
    _run_task: Option<Task<()>>,
    /// Bumped on every desired-state change so a superseded task can detect that
    /// it is stale and avoid clobbering newer state.
    generation: usize,
    /// The embedding server's independent lifecycle (feature-driven by the
    /// semantic-search indexer rather than by user settings).
    embedding_status: ManagerStatus,
    embedding_server: Option<ServerHandle>,
    _embedding_run_task: Option<Task<()>>,
    embedding_generation: usize,
    _quit_subscription: gpui::Subscription,
}

struct GlobalLlamaManager(Entity<LlamaManager>);
impl Global for GlobalLlamaManager {}

/// Installs the global `LlamaManager`. Idempotent; call once during startup
/// before the language-model providers are registered.
pub fn init(http_client: Arc<dyn HttpClient>, cx: &mut App) {
    if cx.has_global::<GlobalLlamaManager>() {
        return;
    }
    if platform_supported() {
        cx.background_spawn(async {
            reap_stray_servers();
        })
        .detach();
    }
    let manager = cx.new(|cx| LlamaManager::new(http_client, cx));
    cx.set_global(GlobalLlamaManager(manager));
}

/// Whether `command_line` is a managed `llama-server`: the executable is named
/// `llama-server` and some argument points into our `llama_cpp` data directory.
/// The `--model` argument always does — every managed model lives there — even
/// when the binary itself is the bundled one inside the app, so this matches
/// managed servers regardless of how the binary was provisioned, and never a
/// user's own llama.cpp install.
fn matches_managed_server(command_line: &[String], data_dir_marker: &str) -> bool {
    let Some(executable) = command_line.first() else {
        return false;
    };
    Path::new(executable)
        .file_name()
        .is_some_and(|name| name == server_bin_name())
        && command_line
            .iter()
            .any(|argument| argument.contains(data_dir_marker))
}

/// Kills managed `llama-server` processes orphaned by a previous app instance
/// that died without cleanup (crash or force-quit predating the stdin-pipe
/// watchdog, or any future watchdog failure), so stale servers are reaped on
/// launch instead of accumulating. A matching server is considered orphaned
/// when its parent is gone: reparented to init, or its recorded parent no
/// longer exists. Servers of a live instance keep that instance's watchdog
/// shell as their parent and are never touched.
fn reap_stray_servers() {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System, UpdateKind};

    let data_dir_marker = paths::data_dir()
        .join("llama_cpp")
        .to_string_lossy()
        .into_owned();
    let refresh = ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always);
    let mut system = System::new_with_specifics(RefreshKind::nothing().with_processes(refresh));
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh);

    for (pid, process) in system.processes() {
        let command_line: Vec<String> = process
            .cmd()
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect();
        if !matches_managed_server(&command_line, &data_dir_marker) {
            continue;
        }
        let orphaned = match process.parent() {
            None => true,
            Some(parent) => parent.as_u32() == 1 || system.process(parent).is_none(),
        };
        if !orphaned {
            continue;
        }
        if process.kill() {
            log::warn!(
                "killed a stray managed llama-server (pid {pid}) left behind by a previous session"
            );
        } else {
            log::warn!("failed to kill a stray managed llama-server (pid {pid})");
        }
    }
}

/// The global manager, if [`init`] has run.
pub fn manager(cx: &App) -> Option<Entity<LlamaManager>> {
    cx.try_global::<GlobalLlamaManager>()
        .map(|global| global.0.clone())
}

impl LlamaManager {
    fn new(http_client: Arc<dyn HttpClient>, cx: &mut Context<Self>) -> Self {
        // The stdin-pipe watchdog already covers every exit path (the pipe
        // closes even on SIGKILL), but killing explicitly on a graceful quit is
        // immediate rather than waiting on the watchdog's read to wake.
        let quit_subscription = cx.on_app_quit(|this, _cx| {
            this.server = None;
            this.embedding_server = None;
            async {}
        });
        Self {
            http_client,
            status: if platform_supported() {
                ManagerStatus::Idle
            } else {
                ManagerStatus::Unsupported
            },
            desired: None,
            selected_model: DEFAULT_MODEL_ID.to_string(),
            server: None,
            _run_task: None,
            generation: 0,
            embedding_status: if platform_supported() {
                ManagerStatus::Idle
            } else {
                ManagerStatus::Unsupported
            },
            embedding_server: None,
            _embedding_run_task: None,
            embedding_generation: 0,
            _quit_subscription: quit_subscription,
        }
    }

    pub fn status(&self) -> &ManagerStatus {
        &self.status
    }

    /// Whether managed mode is currently on.
    pub fn is_enabled(&self) -> bool {
        self.desired.is_some()
    }

    /// The model id the picker should show as selected.
    pub fn selected_model(&self) -> &str {
        &self.selected_model
    }

    /// The loopback port the managed server is serving on, once ready.
    pub fn ready_port(&self) -> Option<u16> {
        match &self.status {
            ManagerStatus::Ready { port, .. } => Some(*port),
            _ => None,
        }
    }

    /// Lifecycle state of the managed embedding server.
    pub fn embedding_status(&self) -> &ManagerStatus {
        &self.embedding_status
    }

    /// The loopback port serving `/v1/embeddings`, once ready.
    pub fn embedding_ready_port(&self) -> Option<u16> {
        match &self.embedding_status {
            ManagerStatus::Ready { port, .. } => Some(*port),
            _ => None,
        }
    }

    /// Starts (or restarts) the managed embedding server. Idempotent while the
    /// requested model is already starting or ready. Driven by features that
    /// need embeddings (the semantic-search indexer), not by user settings.
    pub fn ensure_embedding_running(&mut self, model_id: &str, cx: &mut Context<Self>) {
        match &self.embedding_status {
            ManagerStatus::Ready { model, .. }
            | ManagerStatus::Starting { model }
            | ManagerStatus::Downloading { model, .. }
                if model == model_id =>
            {
                return;
            }
            _ => {}
        }
        let Some(model) = embedding_catalog_model(model_id) else {
            self.embedding_status = ManagerStatus::Error {
                message: format!("Unknown local embedding model \"{model_id}\""),
            };
            cx.notify();
            return;
        };
        self.start_slot(ServerSlot::Embedding, *model, cx);
    }

    /// Stops the managed embedding server and returns the slot to `Idle`.
    pub fn stop_embedding(&mut self, cx: &mut Context<Self>) {
        self.embedding_generation += 1;
        self._embedding_run_task = None;
        self.embedding_server = None;
        self.embedding_status = ManagerStatus::Idle;
        cx.notify();
    }

    fn slot_generation(&self, slot: ServerSlot) -> usize {
        match slot {
            ServerSlot::Chat => self.generation,
            ServerSlot::Embedding => self.embedding_generation,
        }
    }

    fn set_slot_status(&mut self, slot: ServerSlot, status: ManagerStatus) {
        match slot {
            ServerSlot::Chat => self.status = status,
            ServerSlot::Embedding => self.embedding_status = status,
        }
    }

    fn store_slot_server(&mut self, slot: ServerSlot, server: Option<ServerHandle>) {
        match slot {
            ServerSlot::Chat => self.server = server,
            ServerSlot::Embedding => self.embedding_server = server,
        }
    }

    /// Applies the managed settings: records the selected model, and starts or
    /// stops the server so it matches `enabled`. Idempotent — an unchanged
    /// (enabled, model) pair does no work beyond mirroring the selection.
    pub fn set_managed(&mut self, enabled: bool, model_id: String, cx: &mut Context<Self>) {
        let desired = enabled.then(|| model_id.clone());
        let selection_changed = self.selected_model != model_id;
        self.selected_model = model_id;
        if self.desired != desired {
            self.desired = desired.clone();
            match desired {
                Some(id) => self.start(id, cx),
                None => self.stop(cx),
            }
        } else if selection_changed {
            cx.notify();
        }
    }

    /// Forces a (re)start of the given model, turning managed mode on. Used by the
    /// explicit "Download"/"Retry" action so it works even when the state is
    /// already `enabled` (e.g. retrying after an error).
    pub fn ensure_running(&mut self, model_id: String, cx: &mut Context<Self>) {
        self.selected_model = model_id.clone();
        self.desired = Some(model_id.clone());
        self.start(model_id, cx);
    }

    fn start(&mut self, model_id: String, cx: &mut Context<Self>) {
        let Some(model) = catalog_model(&model_id) else {
            self.status = ManagerStatus::Error {
                message: format!("Unknown local model \"{model_id}\""),
            };
            cx.notify();
            return;
        };
        self.start_slot(ServerSlot::Chat, *model, cx);
    }

    fn start_slot(&mut self, slot: ServerSlot, model: CatalogModel, cx: &mut Context<Self>) {
        if !platform_supported() {
            self.store_slot_server(slot, None);
            self.set_slot_status(slot, ManagerStatus::Unsupported);
            match slot {
                ServerSlot::Chat => self._run_task = None,
                ServerSlot::Embedding => self._embedding_run_task = None,
            }
            cx.notify();
            return;
        }

        let generation = match slot {
            ServerSlot::Chat => {
                self.generation += 1;
                self.generation
            }
            ServerSlot::Embedding => {
                self.embedding_generation += 1;
                self.embedding_generation
            }
        };
        // Kill any running server in this slot before switching models.
        self.store_slot_server(slot, None);
        self.set_slot_status(slot, ManagerStatus::Preparing);
        cx.notify();

        let http_client = self.http_client.clone();
        let task = cx.spawn(async move |this, cx| {
            if let Err(error) = run(this.clone(), http_client, model, slot, generation, cx).await {
                log::error!("Local Models manager failed ({slot:?}): {error:#}");
                this.update(cx, |this, cx| {
                    this.fail(slot, generation, format!("{error:#}"), cx)
                })
                .ok();
            }
        });
        match slot {
            ServerSlot::Chat => self._run_task = Some(task),
            ServerSlot::Embedding => self._embedding_run_task = Some(task),
        }
    }

    /// Records a failure for `generation` in `slot`: drops any server handle
    /// this run stored — killing an unhealthy `llama-server` (e.g. one that
    /// spawned but never became ready) rather than leaking its RAM/GPU/port —
    /// and surfaces `message` to the UI. Guarded by generation so a superseded
    /// run never clobbers newer state.
    fn fail(&mut self, slot: ServerSlot, generation: usize, message: String, cx: &mut Context<Self>) {
        if self.slot_generation(slot) != generation {
            return;
        }
        self.store_slot_server(slot, None);
        self.set_slot_status(slot, ManagerStatus::Error { message });
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        // Bump the generation so any in-flight run sees itself as stale.
        self.generation += 1;
        self._run_task = None;
        self.server = None;
        self.status = ManagerStatus::Idle;
        cx.notify();
    }
}

/// True while `generation` is still the current generation for `slot`.
fn is_current(
    this: &WeakEntity<LlamaManager>,
    slot: ServerSlot,
    generation: usize,
    cx: &mut AsyncApp,
) -> bool {
    this.update(cx, |this, _| this.slot_generation(slot) == generation)
        .unwrap_or(false)
}

/// Provisions the binary, ensures the model is present (downloading with
/// progress), spawns the server, and waits for it to become ready. Every state
/// write is guarded by `generation` so a superseded run never clobbers newer
/// state.
async fn run(
    this: WeakEntity<LlamaManager>,
    http_client: Arc<dyn HttpClient>,
    model: CatalogModel,
    slot: ServerSlot,
    generation: usize,
    cx: &mut AsyncApp,
) -> Result<()> {
    let binary = resolve_or_install_server(http_client.as_ref())
        .await
        .context("preparing the local model runtime")?;
    if !is_current(&this, slot, generation, cx) {
        return Ok(());
    }

    let model_path = model_path(&model);
    // Re-verify the pinned SHA-256 of any cached file before launching: a
    // size-only match can still be corrupt, and handing a bad file to
    // llama-server fails opaquely much later. On mismatch, delete and re-download.
    if !verify_cached_model(&model_path, &model).await? {
        // Remove a present-but-unverified file first, so a corrupt cache can
        // never be launched even if the re-download is later interrupted.
        if smol::fs::metadata(&model_path).await.is_ok() {
            log::warn!(
                "cached local model {} failed SHA-256 verification; re-downloading",
                model.id
            );
            if let Err(error) = smol::fs::remove_file(&model_path).await {
                log::warn!("failed to remove unverified cached model {model_path:?}: {error:?}");
            }
        }
        let mut progress_cx = cx.clone();
        let progress_this = this.clone();
        let mut last_reported: u64 = 0;
        download_file_with_progress(
            http_client.as_ref(),
            &model.download_url(),
            &model_path,
            Some(model.sha256),
            Some(model.size_bytes),
            move |progress| {
                // Throttle to ~every 4 MiB (plus the final tick) so a multi-GB
                // download doesn't drive thousands of re-renders.
                let is_final = Some(progress.received) == progress.total;
                if !is_final && progress.received.saturating_sub(last_reported) < 4 * 1024 * 1024 {
                    return;
                }
                last_reported = progress.received;
                progress_this
                    .update(&mut progress_cx, |this, cx| {
                        if this.slot_generation(slot) == generation {
                            this.set_slot_status(
                                slot,
                                ManagerStatus::Downloading {
                                    model: model.id.to_string(),
                                    received: progress.received,
                                    total: progress.total,
                                },
                            );
                            cx.notify();
                        }
                    })
                    .ok();
            },
        )
        .await
        .context("downloading the model")?;
    }
    if !is_current(&this, slot, generation, cx) {
        return Ok(());
    }

    let port = server::find_free_port()?;
    let handle = match slot {
        ServerSlot::Chat => {
            server::spawn_llama_server(&binary, &model_path, port, DEFAULT_CONTEXT_SIZE)?
        }
        ServerSlot::Embedding => server::spawn_llama_embedding_server(
            &binary,
            &model_path,
            port,
            EMBEDDING_CONTEXT_SIZE,
        )?,
    };

    // Store the handle only if this run is still current; otherwise let it drop
    // here, which kills the just-spawned process.
    let mut handle = Some(handle);
    let stored = this.update(cx, |this, cx| {
        if this.slot_generation(slot) != generation {
            return false;
        }
        let handle = handle.take();
        this.store_slot_server(slot, handle);
        this.set_slot_status(
            slot,
            ManagerStatus::Starting {
                model: model.id.to_string(),
            },
        );
        cx.notify();
        true
    })?;
    if !stored {
        return Ok(());
    }

    let health_client = http_client.clone();
    // Schedule the poll delay on the GPUI background executor (the project
    // disallows `smol::Timer::after`).
    let background_executor = cx.background_executor().clone();
    server::poll_until_ready(
        READINESS_TIMEOUT,
        || {
            let health_client = health_client.clone();
            async move { server::health_ready(health_client.as_ref(), port).await }
        },
        || background_executor.timer(READINESS_POLL_INTERVAL),
    )
    .await
    .context("waiting for the model to load")?;
    if !is_current(&this, slot, generation, cx) {
        return Ok(());
    }

    this.update(cx, |this, cx| {
        if this.slot_generation(slot) == generation {
            this.set_slot_status(
                slot,
                ManagerStatus::Ready {
                    model: model.id.to_string(),
                    port,
                },
            );
            cx.notify();
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_resolution() {
        assert_eq!(catalog_model("gemma-3-4b").unwrap().id, "gemma-3-4b");
        assert_eq!(catalog_model("gemma-3-1b").unwrap().id, "gemma-3-1b");
        assert!(catalog_model("does-not-exist").is_none());
        assert_eq!(default_model().id, DEFAULT_MODEL_ID);
    }

    #[test]
    fn embedding_catalog_is_separate_from_the_chat_catalog() {
        assert_eq!(
            default_embedding_model().id,
            DEFAULT_EMBEDDING_MODEL_ID,
            "the default embedding id resolves"
        );
        // The chat picker must never see embedding models, and vice versa.
        assert!(catalog_model(DEFAULT_EMBEDDING_MODEL_ID).is_none());
        for chat_model in CATALOG {
            assert!(embedding_catalog_model(chat_model.id).is_none());
        }
        let model = default_embedding_model();
        assert!(model.download_url().contains("/resolve/main/"));
        assert!(model.approx_size_gb() < 0.5, "embedding model stays tiny");
    }

    // Failing the embedding slot must not touch the chat slot's state (and the
    // other way around) — the two lifecycles are independent.
    #[gpui::test]
    async fn embedding_failure_does_not_clobber_the_chat_slot(cx: &mut gpui::TestAppContext) {
        let manager = cx.new(|cx| LlamaManager::new(Arc::new(UnusedHttpClient), cx));

        let (chat_generation, embedding_generation) = manager.update(cx, |manager, _| {
            manager.status = ManagerStatus::Ready {
                model: "gemma-3-4b".into(),
                port: 12345,
            };
            (manager.generation, manager.embedding_generation)
        });

        manager.update(cx, |manager, cx| {
            manager.fail(
                ServerSlot::Embedding,
                embedding_generation,
                "embedding model failed to load".to_string(),
                cx,
            );
        });

        manager.read_with(cx, |manager, _| {
            assert!(
                matches!(manager.status, ManagerStatus::Ready { port: 12345, .. }),
                "the chat slot must be untouched by an embedding failure"
            );
            assert!(matches!(
                manager.embedding_status,
                ManagerStatus::Error { .. }
            ));
            assert_eq!(manager.embedding_ready_port(), None);
            assert_eq!(manager.ready_port(), Some(12345));
        });

        // And a stale-generation chat failure must not clobber the embedding slot.
        manager.update(cx, |manager, cx| {
            manager.fail(
                ServerSlot::Chat,
                chat_generation + 1,
                "stale".to_string(),
                cx,
            );
        });
        manager.read_with(cx, |manager, _| {
            assert!(matches!(manager.status, ManagerStatus::Ready { .. }));
        });
    }

    #[test]
    fn stray_sweep_matches_only_managed_servers() {
        let marker = "/Users/jay/Library/Application Support/PaddleBoard/llama_cpp";
        let argument = |text: &str| text.to_string();

        // The exact shape of the observed orphans: data-dir binary + data-dir model.
        let orphan = vec![
            argument(&format!("{marker}/b9874/llama-b9874/llama-server")),
            argument("--host"),
            argument("127.0.0.1"),
            argument("--port"),
            argument("57791"),
            argument("--model"),
            argument(&format!("{marker}/models/embeddinggemma-300m-qat-Q8_0.gguf")),
            argument("--ctx-size"),
            argument("2048"),
            argument("--embeddings"),
        ];
        assert!(matches_managed_server(&orphan, marker));

        // A bundled binary still matches through its --model argument.
        let bundled = vec![
            argument("/Applications/PaddleBoard.app/Contents/MacOS/llama/llama-server"),
            argument("--model"),
            argument(&format!("{marker}/models/gemma-3-4b-it-Q4_K_M.gguf")),
        ];
        assert!(matches_managed_server(&bundled, marker));

        // A user's own llama.cpp install must never be touched.
        let unrelated = vec![
            argument("/opt/homebrew/bin/llama-server"),
            argument("--model"),
            argument("/Users/jay/models/own.gguf"),
        ];
        assert!(!matches_managed_server(&unrelated, marker));

        // The watchdog shell wrapper references the same paths but is not the
        // server (argv[0] is sh); killing it would orphan its child.
        let watchdog = vec![
            argument("/bin/sh"),
            argument("-c"),
            argument("\"$0\" \"$@\" &"),
            argument(&format!("{marker}/b9874/llama-b9874/llama-server")),
            argument("--model"),
            argument(&format!("{marker}/models/gemma-3-4b-it-Q4_K_M.gguf")),
        ];
        assert!(!matches_managed_server(&watchdog, marker));

        assert!(!matches_managed_server(&[], marker));
    }

    // A graceful quit must kill both managed servers even though the process
    // is about to exit anyway — deterministic teardown, not watchdog latency.
    #[cfg(unix)]
    #[gpui::test]
    async fn app_quit_kills_the_managed_servers(cx: &mut gpui::TestAppContext) {
        let manager = cx.new(|cx| LlamaManager::new(Arc::new(UnusedHttpClient), cx));

        let mut command = std::process::Command::new("sleep");
        command.arg("300");
        let handle = ServerHandle::spawn_for_test(command).unwrap();
        let pid = handle.pid();
        assert!(process_is_alive(pid));

        manager.update(cx, |manager, _| {
            manager.embedding_server = Some(handle);
        });

        cx.update(|cx| cx.shutdown());

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while process_is_alive(pid) {
            assert!(
                std::time::Instant::now() < deadline,
                "the managed server should be killed by the on_app_quit handler"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    #[test]
    fn download_url_targets_hugging_face_resolve() {
        let model = catalog_model("gemma-3-1b").unwrap();
        assert_eq!(
            model.download_url(),
            "https://huggingface.co/unsloth/gemma-3-1b-it-GGUF/resolve/main/gemma-3-1b-it-Q4_K_M.gguf"
        );
    }

    #[test]
    fn one_gigabyte_ish_reports_reasonable_size() {
        let model = catalog_model("gemma-3-4b").unwrap();
        assert!((model.approx_size_gb() - 2.49).abs() < 0.05);
    }

    // A size-matching but hash-mismatching cached file must be rejected (and so
    // re-fetched by `run`), never handed straight to llama-server.
    #[test]
    fn cached_model_verification_rejects_size_match_hash_mismatch() {
        futures::executor::block_on(async {
            let temp_dir = tempfile::tempdir().unwrap();
            let path = temp_dir.path().join("model.gguf");
            let bytes = b"not the genuine weights".to_vec();
            smol::fs::write(&path, &bytes).await.unwrap();

            let model = CatalogModel {
                id: "test",
                display_name: "Test",
                description: "",
                repo: "owner/repo",
                file: "model.gguf",
                // Length matches the bytes on disk, so the size-only check passes...
                size_bytes: bytes.len() as u64,
                // ...but this pin is for different bytes.
                sha256: "0000000000000000000000000000000000000000000000000000000000000000",
            };

            assert!(
                model_present(&path, model.size_bytes).await,
                "the size-only check should be fooled by the matching length"
            );
            assert!(
                !verify_cached_model(&path, &model).await.unwrap(),
                "hash re-verification must reject the corrupt cache"
            );

            // sha256_file reports the true digest of the bytes on disk, so a pin
            // equal to it would verify as valid.
            let actual = sha256_file(&path).await.unwrap();
            assert_eq!(actual, format!("{:x}", Sha256::digest(&bytes)));
        });
    }

    struct UnusedHttpClient;

    impl HttpClient for UnusedHttpClient {
        fn send(
            &self,
            _request: http_client::http::Request<http_client::AsyncBody>,
        ) -> futures::future::BoxFuture<
            'static,
            Result<http_client::Response<http_client::AsyncBody>>,
        > {
            Box::pin(async { anyhow::bail!("the http client is unused in this test") })
        }

        fn user_agent(&self) -> Option<&http_client::http::HeaderValue> {
            None
        }

        fn proxy(&self) -> Option<&url::Url> {
            None
        }
    }

    #[cfg(unix)]
    fn process_is_alive(pid: u32) -> bool {
        // `kill(pid, 0)` performs no signal delivery but reports (via ESRCH)
        // whether a process with that pid still exists.
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }

    // A run that stores its server handle and then fails (e.g. the readiness poll
    // times out) must clear `self.server`, so the unhealthy process is dropped
    // (and killed) instead of leaking its RAM/GPU/port.
    #[cfg(unix)]
    #[gpui::test]
    async fn readiness_failure_clears_and_kills_the_server(cx: &mut gpui::TestAppContext) {
        let manager = cx.new(|cx| LlamaManager::new(Arc::new(UnusedHttpClient), cx));

        // A cheap long-lived process stands in for a spawned-but-never-ready
        // llama-server, as if `run` had just stored its handle.
        let mut command = std::process::Command::new("sleep");
        command.arg("300");
        let handle = ServerHandle::spawn_for_test(command).unwrap();
        let pid = handle.pid();
        assert!(process_is_alive(pid), "the stand-in server should be running");

        let generation = manager.update(cx, |manager, _| {
            manager.server = Some(handle);
            manager.generation
        });

        manager.update(cx, |manager, cx| {
            manager.fail(
                ServerSlot::Chat,
                generation,
                "timed out waiting for the model".to_string(),
                cx,
            );
        });

        manager.read_with(cx, |manager, _| {
            assert!(
                manager.server.is_none(),
                "the leaked server handle must be cleared on failure"
            );
            assert!(matches!(manager.status, ManagerStatus::Error { .. }));
        });

        // Dropping the handle SIGKILLs the process group; wait for it to die.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while process_is_alive(pid) {
            assert!(
                std::time::Instant::now() < deadline,
                "the unhealthy llama-server process should be killed after clearing the handle"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}
