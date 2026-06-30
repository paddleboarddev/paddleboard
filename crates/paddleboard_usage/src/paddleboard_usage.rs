//! PaddleBoard: local, private LLM usage tracking.
//!
//! Records per-provider, per-model token counts to a flatfile on disk so a
//! user who mixes multiple providers (Anthropic, OpenAI, Gemini, Vertex,
//! Bedrock, local SLMs, …) can see where their usage goes over time. One JSON
//! file is written per day (`<dir>/YYYY-MM-DD.json`), which is intentionally a
//! text format so the directory can live inside the user's own (private) git
//! repository and produce clean diffs. Optionally PaddleBoard commits that
//! directory after each flush.
//!
//! All data stays on the user's machine — nothing is reported anywhere. This is
//! the persistent, multi-provider companion to the live status-bar context
//! gauge (`agent_ui::UsageStatusItem`).
//!
//! Recording is driven from a single choke point in the agent
//! (`Thread::accumulate_token_usage`), which is the one place every provider's
//! billed token delta — for both normal completions and context compaction —
//! flows through. [`record`] takes that delta, accumulates it in memory, and a
//! background task flushes to disk on an interval, so the hot path never blocks
//! on I/O.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use gpui::App;
use serde::{Deserialize, Serialize};
use settings::{RegisterSetting, Settings, SettingsStore};

/// Schema version written into each day file, so the format can evolve.
const FILE_SCHEMA_VERSION: u32 = 1;

/// How often the background task writes accumulated usage to disk.
const FLUSH_INTERVAL: Duration = Duration::from_secs(60);

/// Lower bound between two automatic git commits, so a busy session does not
/// produce a commit every flush.
const AUTO_COMMIT_MIN_INTERVAL: Duration = Duration::from_secs(300);

/// Identity used for automatic commits, so they do not depend on (or leak) the
/// user's configured git identity. Only used when `auto_commit` is enabled.
const COMMIT_NAME: &str = "PaddleBoard";
const COMMIT_EMAIL: &str = "usage@paddleboard.dev";

// ---------------------------------------------------------------------------
// Public token-count type
// ---------------------------------------------------------------------------

/// The four token counts PaddleBoard tracks, matching the provider-agnostic
/// `language_model::TokenUsage`. Kept as a local type so this crate does not
/// need to depend on the language-model crates; the agent hook constructs it
/// from the billed delta it already computes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenCounts {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
}

impl TokenCounts {
    pub fn is_zero(&self) -> bool {
        self.input_tokens == 0
            && self.output_tokens == 0
            && self.cache_creation_input_tokens == 0
            && self.cache_read_input_tokens == 0
    }

    /// Total tokens across all four categories — the headline "tokens used".
    pub fn total(&self) -> u64 {
        self.input_tokens
            .saturating_add(self.output_tokens)
            .saturating_add(self.cache_creation_input_tokens)
            .saturating_add(self.cache_read_input_tokens)
    }

    fn add(&mut self, other: TokenCounts) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.cache_creation_input_tokens = self
            .cache_creation_input_tokens
            .saturating_add(other.cache_creation_input_tokens);
        self.cache_read_input_tokens = self
            .cache_read_input_tokens
            .saturating_add(other.cache_read_input_tokens);
    }
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UsageGranularity {
    /// One rolled-up total per day, per provider, per model.
    #[default]
    Daily,
    /// Additionally break each day down by agent session.
    Session,
}

#[derive(Debug, Clone, PartialEq, RegisterSetting)]
pub struct UsageSettings {
    pub enabled: bool,
    pub granularity: UsageGranularity,
    /// `None` means the default location (`<data_dir>/usage`).
    pub directory: Option<PathBuf>,
    pub auto_commit: bool,
}

impl Default for UsageSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            granularity: UsageGranularity::Daily,
            directory: None,
            auto_commit: false,
        }
    }
}

impl Settings for UsageSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let defaults = Self::default();
        let Some(content) = content.paddleboard_usage.as_ref() else {
            return defaults;
        };
        Self {
            enabled: content.enabled.unwrap_or(defaults.enabled),
            granularity: content
                .granularity
                .map(|granularity| match granularity {
                    settings::PaddleboardUsageGranularityContent::Daily => UsageGranularity::Daily,
                    settings::PaddleboardUsageGranularityContent::Session => {
                        UsageGranularity::Session
                    }
                })
                .unwrap_or(defaults.granularity),
            directory: content
                .directory
                .as_ref()
                .map(|dir| expand_tilde(dir))
                .or(defaults.directory),
            auto_commit: content.auto_commit.unwrap_or(defaults.auto_commit),
        }
    }
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        paths::home_dir().join(rest)
    } else if path == "~" {
        paths::home_dir().clone()
    } else {
        PathBuf::from(path)
    }
}

/// The resolved usage directory for the given setting (applying the default).
fn resolve_directory(settings: &UsageSettings) -> PathBuf {
    settings
        .directory
        .clone()
        .unwrap_or_else(|| paths::data_dir().join("usage"))
}

// ---------------------------------------------------------------------------
// On-disk format
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct DayFile {
    schema: u32,
    date: String,
    entries: Vec<UsageEntry>,
}

/// One recorded line: usage for a (provider, model[, session]) bucket on a day.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageEntry {
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(flatten)]
    pub counts: TokenCounts,
}

/// A dated usage entry, as returned to the UI from [`read_totals`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DatedUsage {
    pub date: String,
    pub provider: String,
    pub model: String,
    pub session: Option<String>,
    pub counts: TokenCounts,
}

/// Per-(provider, model) totals across three time windows, for the UI.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProviderModelTotals {
    pub provider: String,
    pub model: String,
    pub today: TokenCounts,
    pub last_7_days: TokenCounts,
    pub all_time: TokenCounts,
}

/// A pre-aggregated view of all recorded usage, for the AI Dock Usage tab.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsageSummary {
    pub enabled: bool,
    pub directory: Option<PathBuf>,
    /// Rows sorted by all-time usage, descending.
    pub rows: Vec<ProviderModelTotals>,
    pub today: TokenCounts,
    pub last_7_days: TokenCounts,
    pub all_time: TokenCounts,
}

// ---------------------------------------------------------------------------
// Store (pure, testable accumulation + persistence)
// ---------------------------------------------------------------------------

/// In-memory accumulation keyed by `(provider, model, session)`. `BTreeMap`
/// keeps the on-disk entry order stable so git diffs stay minimal.
type EntryKey = (String, String, Option<String>);

struct Store {
    directory: PathBuf,
    days: BTreeMap<String, BTreeMap<EntryKey, TokenCounts>>,
    dirty: BTreeSet<String>,
}

impl Store {
    fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            days: BTreeMap::new(),
            dirty: BTreeSet::new(),
        }
    }

    fn record(
        &mut self,
        date: &str,
        provider: &str,
        model: &str,
        session: Option<String>,
        counts: TokenCounts,
    ) {
        if counts.is_zero() {
            return;
        }
        self.ensure_day_loaded(date);
        let day = self.days.entry(date.to_string()).or_default();
        let key = (provider.to_string(), model.to_string(), session);
        day.entry(key).or_default().add(counts);
        self.dirty.insert(date.to_string());
    }

    /// Merge any already-persisted data for `date` into memory once, so a flush
    /// rewrites the file with the running total rather than clobbering prior
    /// data (e.g. earlier today, before a restart).
    fn ensure_day_loaded(&mut self, date: &str) {
        if self.days.contains_key(date) {
            return;
        }
        let mut day: BTreeMap<EntryKey, TokenCounts> = BTreeMap::new();
        if let Some(file) = read_day_file(&self.day_path(date)) {
            for entry in file.entries {
                let key = (entry.provider, entry.model, entry.session);
                day.entry(key).or_default().add(entry.counts);
            }
        }
        self.days.insert(date.to_string(), day);
    }

    fn day_path(&self, date: &str) -> PathBuf {
        self.directory.join(format!("{date}.json"))
    }

    /// Serialize each dirty day to `(path, json)` and clear the dirty set. The
    /// caller performs the actual disk writes outside any lock.
    fn take_pending_writes(&mut self) -> Vec<(PathBuf, String)> {
        let mut writes = Vec::new();
        for date in std::mem::take(&mut self.dirty) {
            let Some(day) = self.days.get(&date) else {
                continue;
            };
            let entries = day
                .iter()
                .map(|((provider, model, session), counts)| UsageEntry {
                    provider: provider.clone(),
                    model: model.clone(),
                    session: session.clone(),
                    counts: *counts,
                })
                .collect();
            let file = DayFile {
                schema: FILE_SCHEMA_VERSION,
                date: date.clone(),
                entries,
            };
            match serde_json::to_string_pretty(&file) {
                Ok(json) => writes.push((self.day_path(&date), format!("{json}\n"))),
                Err(error) => log::error!("paddleboard_usage: failed to serialize {date}: {error}"),
            }
        }
        writes
    }
}

fn read_day_file(path: &Path) -> Option<DayFile> {
    let contents = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&contents) {
        Ok(file) => Some(file),
        Err(error) => {
            log::error!(
                "paddleboard_usage: ignoring unreadable usage file {}: {error}",
                path.display()
            );
            None
        }
    }
}

/// Atomically write `contents` to `path` (temp file + rename) so a crash mid
/// write can never corrupt an existing day file.
fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, contents)?;
    std::fs::rename(&temp, path)
}

// ---------------------------------------------------------------------------
// Global recorder
// ---------------------------------------------------------------------------

struct Config {
    enabled: bool,
    granularity: UsageGranularity,
    auto_commit: bool,
}

struct Recorder {
    config: Config,
    store: Store,
    last_commit: Option<Instant>,
    /// Set when files have been written but not yet auto-committed; cleared
    /// after a commit attempt. Avoids running git when nothing changed.
    pending_commit: bool,
}

static RECORDER: OnceLock<Mutex<Recorder>> = OnceLock::new();

fn with_recorder<R>(f: impl FnOnce(&mut Recorder) -> R) -> Option<R> {
    let recorder = RECORDER.get()?;
    match recorder.lock() {
        Ok(mut guard) => Some(f(&mut guard)),
        Err(_) => None,
    }
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Record a billed token delta for the current model. Called from the agent's
/// single usage choke point. Cheap and non-blocking: it only accumulates in
/// memory; the background task persists to disk.
pub fn record(provider: &str, model: &str, session_id: &str, counts: TokenCounts) {
    if counts.is_zero() {
        return;
    }
    let date = today();
    with_recorder(|recorder| {
        if !recorder.config.enabled {
            return;
        }
        let session = match recorder.config.granularity {
            UsageGranularity::Daily => None,
            UsageGranularity::Session => Some(session_id.to_string()),
        };
        recorder.store.record(&date, provider, model, session, counts);
    });
}

/// Persist any accumulated usage to disk now. Cheap and synchronous (no git),
/// so it is safe to call from the UI read path. Auto-commit is handled
/// separately on the background task; see [`maybe_auto_commit`].
pub fn flush_now() {
    let writes = match with_recorder(|recorder| {
        let writes = recorder.store.take_pending_writes();
        if !writes.is_empty() {
            recorder.pending_commit = true;
        }
        writes
    }) {
        Some(writes) => writes,
        None => return,
    };

    for (path, contents) in &writes {
        if let Err(error) = write_atomic(path, contents) {
            log::error!(
                "paddleboard_usage: failed to write {}: {error}",
                path.display()
            );
        }
    }
}

/// If auto-commit is enabled, files have changed, and the throttle window has
/// elapsed, commit the usage directory. Runs on the background task.
async fn maybe_auto_commit() {
    let directory = with_recorder(|recorder| {
        let due = recorder.config.auto_commit
            && recorder.pending_commit
            && recorder
                .last_commit
                .is_none_or(|last| last.elapsed() >= AUTO_COMMIT_MIN_INTERVAL);
        if !due {
            return None;
        }
        recorder.last_commit = Some(Instant::now());
        recorder.pending_commit = false;
        Some(recorder.store.directory.clone())
    })
    .flatten();
    let Some(directory) = directory else {
        return;
    };
    commit_directory(&directory).await;
}

/// Run `git add` + `git commit` in `directory`, using a fixed PaddleBoard
/// identity so commits do not depend on (or leak) the user's git config.
/// Best-effort: failures (not a repo, nothing to commit, no git) are logged,
/// never surfaced, and never block the app.
async fn commit_directory(directory: &Path) {
    if !run_git(directory, &["rev-parse", "--is-inside-work-tree"]).await {
        return;
    }
    if !run_git(directory, &["add", "--", "."]).await {
        log::error!(
            "paddleboard_usage: `git add` failed in {}",
            directory.display()
        );
        return;
    }
    let message = format!("Update usage stats ({})", today());
    let name_arg = format!("user.name={COMMIT_NAME}");
    let email_arg = format!("user.email={COMMIT_EMAIL}");
    // A non-zero exit from `commit` usually just means "nothing to commit", so
    // its result is intentionally not treated as an error here.
    run_git(
        directory,
        &[
            "-c",
            &name_arg,
            "-c",
            &email_arg,
            "commit",
            "--quiet",
            "--no-verify",
            "-m",
            &message,
        ],
    )
    .await;
}

/// Run `git -C <directory> <args>`, returning whether it exited successfully.
async fn run_git(directory: &Path, args: &[&str]) -> bool {
    match smol::process::Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .output()
        .await
    {
        Ok(output) => output.status.success(),
        Err(error) => {
            log::error!("paddleboard_usage: failed to run git: {error}");
            false
        }
    }
}

/// The resolved directory usage files are written to (for the UI's "open
/// folder" affordance and the settings display).
pub fn usage_directory() -> Option<PathBuf> {
    with_recorder(|recorder| recorder.store.directory.clone())
}

pub fn is_enabled() -> bool {
    with_recorder(|recorder| recorder.config.enabled).unwrap_or(false)
}

/// Read all persisted usage from disk, flushing in-memory state first so the
/// result includes the current session. Used by the AI Dock Usage tab.
pub fn read_totals() -> Vec<DatedUsage> {
    flush_now();
    let Some(directory) = usage_directory() else {
        return Vec::new();
    };
    read_totals_in(&directory)
}

fn read_totals_in(directory: &Path) -> Vec<DatedUsage> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(directory) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Some(file) = read_day_file(&path) else {
            continue;
        };
        for usage in file.entries {
            out.push(DatedUsage {
                date: file.date.clone(),
                provider: usage.provider,
                model: usage.model,
                session: usage.session,
                counts: usage.counts,
            });
        }
    }
    out
}

/// Aggregate all persisted usage into today / last-7-days / all-time totals,
/// grouped by provider and model (collapsing any per-session breakdown).
pub fn summary() -> UsageSummary {
    let directory = usage_directory();
    let enabled = is_enabled();
    let entries = read_totals();

    let today = chrono::Local::now().date_naive();
    let week_start = today - chrono::Duration::days(6);

    let mut rows: BTreeMap<(String, String), ProviderModelTotals> = BTreeMap::new();
    let mut total_today = TokenCounts::default();
    let mut total_week = TokenCounts::default();
    let mut total_all = TokenCounts::default();

    for entry in entries {
        let date = chrono::NaiveDate::parse_from_str(&entry.date, "%Y-%m-%d").ok();
        let in_today = date == Some(today);
        let in_week = date.is_some_and(|date| date >= week_start && date <= today);

        let row = rows
            .entry((entry.provider.clone(), entry.model.clone()))
            .or_insert_with(|| ProviderModelTotals {
                provider: entry.provider.clone(),
                model: entry.model.clone(),
                ..Default::default()
            });
        row.all_time.add(entry.counts);
        total_all.add(entry.counts);
        if in_week {
            row.last_7_days.add(entry.counts);
            total_week.add(entry.counts);
        }
        if in_today {
            row.today.add(entry.counts);
            total_today.add(entry.counts);
        }
    }

    let mut rows: Vec<ProviderModelTotals> = rows.into_values().collect();
    rows.sort_by(|a, b| {
        b.all_time
            .total()
            .cmp(&a.all_time.total())
            .then_with(|| a.provider.cmp(&b.provider))
            .then_with(|| a.model.cmp(&b.model))
    });

    UsageSummary {
        enabled,
        directory,
        rows,
        today: total_today,
        last_7_days: total_week,
        all_time: total_all,
    }
}

// ---------------------------------------------------------------------------
// Init / wiring
// ---------------------------------------------------------------------------

/// Force-link the crate early so the `RegisterSetting` inventory entry for
/// [`UsageSettings`] is present before `settings::init` collects registrations.
/// Mirrors the two-phase pattern used by the other PaddleBoard settings crates.
pub fn init_settings(_cx: &mut App) {}

/// Initialize the recorder: seed config from settings, observe changes, and
/// spawn the background flush loop. Must be called after `settings::init`.
pub fn init(cx: &mut App) {
    let settings = UsageSettings::get_global(cx).clone();
    let directory = resolve_directory(&settings);
    let recorder = Recorder {
        config: Config {
            enabled: settings.enabled,
            granularity: settings.granularity,
            auto_commit: settings.auto_commit,
        },
        store: Store::new(directory),
        last_commit: None,
        pending_commit: false,
    };
    // If init runs twice (e.g. in tests) the first recorder wins; that is fine.
    let _ = RECORDER.set(Mutex::new(recorder));

    cx.observe_global::<SettingsStore>(|cx| {
        let settings = UsageSettings::get_global(cx).clone();
        let directory = resolve_directory(&settings);
        with_recorder(|recorder| {
            recorder.config.enabled = settings.enabled;
            recorder.config.granularity = settings.granularity;
            recorder.config.auto_commit = settings.auto_commit;
            if recorder.store.directory != directory {
                // The target moved; flush what we have to the old location,
                // then start fresh against the new one.
                for (path, contents) in recorder.store.take_pending_writes() {
                    let _ = write_atomic(&path, &contents);
                }
                recorder.store = Store::new(directory);
            }
        });
    })
    .detach();

    let executor = cx.background_executor().clone();
    cx.background_executor()
        .spawn(async move {
            loop {
                executor.timer(FLUSH_INTERVAL).await;
                flush_now();
                maybe_auto_commit().await;
            }
        })
        .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(input: u64, output: u64) -> TokenCounts {
        TokenCounts {
            input_tokens: input,
            output_tokens: output,
            ..Default::default()
        }
    }

    #[test]
    fn accumulates_deltas_per_bucket() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::new(dir.path().to_path_buf());

        store.record("2026-06-25", "anthropic", "opus", None, counts(100, 10));
        store.record("2026-06-25", "anthropic", "opus", None, counts(50, 5));
        store.record("2026-06-25", "openai", "gpt", None, counts(7, 3));

        // Two providers => two buckets; the opus bucket summed both deltas.
        let writes = store.take_pending_writes();
        assert_eq!(writes.len(), 1, "one day file");
        for (path, contents) in &writes {
            write_atomic(path, contents).unwrap();
        }

        let totals = read_totals_in(dir.path());
        let opus = totals
            .iter()
            .find(|usage| usage.provider == "anthropic")
            .unwrap();
        assert_eq!(opus.counts.input_tokens, 150);
        assert_eq!(opus.counts.output_tokens, 15);
        assert_eq!(totals.len(), 2);
    }

    #[test]
    fn merges_with_existing_file_across_restart() {
        let dir = tempfile::tempdir().unwrap();

        {
            let mut store = Store::new(dir.path().to_path_buf());
            store.record("2026-06-25", "anthropic", "opus", None, counts(100, 10));
            for (path, contents) in store.take_pending_writes() {
                write_atomic(&path, &contents).unwrap();
            }
        }

        // Fresh store (simulating a restart) must extend, not clobber, the file.
        let mut store = Store::new(dir.path().to_path_buf());
        store.record("2026-06-25", "anthropic", "opus", None, counts(25, 5));
        for (path, contents) in store.take_pending_writes() {
            write_atomic(&path, &contents).unwrap();
        }

        let totals = read_totals_in(dir.path());
        assert_eq!(totals.len(), 1);
        assert_eq!(totals[0].counts.input_tokens, 125);
        assert_eq!(totals[0].counts.output_tokens, 15);
    }

    #[test]
    fn session_granularity_splits_buckets() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::new(dir.path().to_path_buf());

        store.record(
            "2026-06-25",
            "anthropic",
            "opus",
            Some("session-a".into()),
            counts(100, 10),
        );
        store.record(
            "2026-06-25",
            "anthropic",
            "opus",
            Some("session-b".into()),
            counts(40, 4),
        );

        let totals = read_totals_in_after_flush(&mut store, dir.path());
        assert_eq!(totals.len(), 2, "one bucket per session");
    }

    #[test]
    fn zero_deltas_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::new(dir.path().to_path_buf());
        store.record("2026-06-25", "anthropic", "opus", None, TokenCounts::default());
        assert!(store.take_pending_writes().is_empty());
    }

    fn read_totals_in_after_flush(store: &mut Store, dir: &Path) -> Vec<DatedUsage> {
        for (path, contents) in store.take_pending_writes() {
            write_atomic(&path, &contents).unwrap();
        }
        read_totals_in(dir)
    }
}
