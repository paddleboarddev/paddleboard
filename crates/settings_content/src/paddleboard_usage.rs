// PaddleBoard: settings schema for the local LLM usage tracker. Lives in
// settings_content so the field can be deserialized like any other Zed
// setting; the typed wrapper + recording logic lives in the
// `paddleboard_usage` crate to keep this file's drift surface small.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings_macros::{MergeFrom, with_fallible_options};

#[with_fallible_options]
#[derive(Debug, Default, PartialEq, Clone, Serialize, Deserialize, JsonSchema, MergeFrom)]
pub struct PaddleboardUsageContent {
    /// Whether local LLM usage tracking is enabled. When `true`, PaddleBoard
    /// records per-provider, per-model token counts to a local flatfile so you
    /// can see how your usage is distributed across providers over time.
    ///
    /// All data stays on your machine — nothing is ever reported anywhere. The
    /// flatfile is yours to back up to your own (private) git repository.
    ///
    /// Default: true
    pub enabled: Option<bool>,

    /// How finely usage is recorded.
    ///
    /// - `"daily"` (default): one rolled-up total per day, per provider, per
    ///   model. Smallest file, cleanest git diffs.
    /// - `"session"`: additionally breaks each day down by agent session, so
    ///   you can see usage per conversation.
    pub granularity: Option<PaddleboardUsageGranularityContent>,

    /// Directory the usage flatfiles are written to. One JSON file is written
    /// per day (`YYYY-MM-DD.json`). Point this at a path inside your own git
    /// repository to back the history up.
    ///
    /// Supports a leading `~` for the home directory. Default: PaddleBoard's
    /// data directory (`<data_dir>/usage`).
    pub directory: Option<String>,

    /// When `true`, PaddleBoard runs `git add` + `git commit` in the usage
    /// directory after each flush (if it is inside a git repository), so the
    /// history is committed automatically. Off by default — when off, you
    /// commit and push the flatfiles yourself.
    ///
    /// Default: false
    pub auto_commit: Option<bool>,
}

#[derive(
    Debug, Copy, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, MergeFrom,
)]
#[serde(rename_all = "snake_case")]
pub enum PaddleboardUsageGranularityContent {
    #[default]
    Daily,
    Session,
}
