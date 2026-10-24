// PaddleBoard: settings schema for model tiers (routine work on a cheap model,
// escalation to a stronger one when a project check fails). Lives in
// settings_content so it deserializes like any other setting and can be set per
// project; the typed wrapper and the escalation logic live in the
// `paddleboard_tiers` crate to keep this file's drift surface small.
// Design: MODEL_TIERS.md at the repo root.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings_macros::{MergeFrom, with_fallible_options};

use crate::LanguageModelSelection;

#[with_fallible_options]
#[derive(Debug, Default, PartialEq, Clone, Serialize, Deserialize, JsonSchema, MergeFrom)]
pub struct PaddleboardTiersContent {
    /// Whether Spotter is active. When `false`, nothing runs after a turn
    /// and no model is ever switched.
    ///
    /// Default: false
    pub enabled: Option<bool>,

    /// The tiers, cheapest first. Each names a model PaddleBoard already knows
    /// how to call. The thread's current model decides which tier it is on.
    pub tiers: Option<Vec<PaddleboardTierContent>>,

    /// The command that decides whether the agent's work passed, for example
    /// `cargo test -p my_crate`. It runs in the agent's terminal, under the same
    /// sandbox and permissions, at the end of every agent turn. Usually set in
    /// the project's `.paddleboard/settings.json`.
    pub check: Option<PaddleboardTierCheckContent>,

    /// How many times the agent may try to fix a failed check on the same tier
    /// before it stops. `1` is the "two-strike" rule: one retry, then stop.
    ///
    /// Default: 1
    pub retries_per_tier: Option<u32>,

    /// What happens when a tier runs out of retries.
    ///
    /// - `"ask"` (default): stop and offer the next tier.
    /// - `"auto"`: move to the next tier without asking, up to `auto_ceiling`.
    /// - `"off"`: stop and report; never offer a stronger model.
    pub escalation: Option<PaddleboardTierEscalationContent>,

    /// The highest tier `"auto"` escalation may reach without asking, by name.
    /// Above it, PaddleBoard stops and asks. Unset means no ceiling.
    pub auto_ceiling: Option<String>,
}

#[with_fallible_options]
#[derive(Debug, Default, PartialEq, Clone, Serialize, Deserialize, JsonSchema, MergeFrom)]
pub struct PaddleboardTierContent {
    /// The name shown in the thread, for example "Local" or "Precise".
    pub name: Option<String>,
    /// The model this tier runs.
    pub model: Option<LanguageModelSelection>,
}

#[with_fallible_options]
#[derive(Debug, Default, PartialEq, Clone, Serialize, Deserialize, JsonSchema, MergeFrom)]
pub struct PaddleboardTierCheckContent {
    /// The command to run, as a one-liner.
    pub command: Option<String>,
    /// The longest the check may run before it counts as failed.
    ///
    /// Default: 300
    pub timeout_seconds: Option<u64>,
}

#[derive(
    Debug, Copy, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, MergeFrom,
)]
#[serde(rename_all = "snake_case")]
pub enum PaddleboardTierEscalationContent {
    #[default]
    Ask,
    Auto,
    Off,
}
