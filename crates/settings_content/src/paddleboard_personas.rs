// PaddleBoard: settings schema for the persona system (PERSONA.md). Lives in
// settings_content so the field deserializes like any other Zed setting; the
// typed wrapper + init lives in `paddleboard_personas_settings` to keep this
// file's drift surface small.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings_macros::{MergeFrom, with_fallible_options};

#[with_fallible_options]
#[derive(Debug, Default, PartialEq, Clone, Serialize, Deserialize, JsonSchema, MergeFrom)]
pub struct PaddleboardPersonasContent {
    /// Whether the persona system is enabled. When `true`, PaddleBoard picks up
    /// a `PERSONA.md` at the project root as the default persona for new agent
    /// threads, discovers `*.persona.md` files in `.claude/personas/` (project
    /// and `~/.claude/personas/`), and shows the persona picker in the agent
    /// panel.
    ///
    /// Default: true
    pub enabled: Option<bool>,
}
