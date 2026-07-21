// PaddleBoard: settings schema for PaddleBoard-added chrome visibility (dock
// panel buttons and status bar items). Lives in settings_content so the field
// deserializes like any other Zed setting; the typed wrapper + init lives in
// `paddleboard_ui` to keep this file's drift surface small.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings_macros::{MergeFrom, with_fallible_options};

#[with_fallible_options]
#[derive(Debug, Default, PartialEq, Clone, Serialize, Deserialize, JsonSchema, MergeFrom)]
pub struct PaddleboardUiContent {
    /// Whether to show the Browser panel button in the dock.
    ///
    /// Default: true
    pub browser_button: Option<bool>,
    /// Whether to show the AI Provider picker panel button in the dock.
    ///
    /// Default: true
    pub llm_picker_button: Option<bool>,
    /// Whether to show the Orchestration panel button in the dock.
    ///
    /// Default: true
    pub orchestration_button: Option<bool>,
    /// Whether to show the Manifest panel button in the dock.
    ///
    /// Default: true
    pub manifest_button: Option<bool>,
    /// Whether to show the sandbox backend status item in the status bar.
    ///
    /// Default: true
    pub sandbox_status: Option<bool>,
    /// Whether to show the MCP servers status item in the status bar.
    ///
    /// Default: true
    pub mcp_status: Option<bool>,
    /// Whether to show the agent token usage status item in the status bar.
    ///
    /// Default: true
    pub usage_status: Option<bool>,
    /// Whether to show the Set Sail deploy status item in the status bar.
    ///
    /// Default: true
    pub set_sail_status: Option<bool>,
}
