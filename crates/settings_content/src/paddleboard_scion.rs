// PaddleBoard: settings schema for the Scion multi-agent integration. Lives in
// settings_content so the field deserializes like any other Zed setting; the
// typed wrapper + init lives in `paddleboard_scion_settings` to keep this
// file's drift surface small.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings_macros::{MergeFrom, with_fallible_options};

#[with_fallible_options]
#[derive(Debug, Default, PartialEq, Clone, Serialize, Deserialize, JsonSchema, MergeFrom)]
pub struct PaddleboardScionContent {
    /// Whether the Scion integration is enabled. When `true` (and the `scion`
    /// CLI is installed on `PATH`), PaddleBoard polls the local Scion daemon,
    /// shows the Scion section in the orchestration panel, and exposes the
    /// `spawn_scion_agent` tool to agents.
    ///
    /// Default: false
    pub enabled: Option<bool>,
}
