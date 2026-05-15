// PaddleBoard: settings schema for the sandbox enforcement layer. Lives in
// settings_content so the field can be deserialized like any other Zed
// setting; the typed wrapper + policy logic lives in
// `paddleboard_sandbox_settings` to keep this file's drift surface small.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings_macros::{MergeFrom, with_fallible_options};

#[with_fallible_options]
#[derive(Debug, Default, PartialEq, Clone, Serialize, Deserialize, JsonSchema, MergeFrom)]
pub struct PaddleboardSandboxContent {
    /// Policy applied when a sandboxed tool tries to launch but the host
    /// prerequisites (Podman, gVisor `runsc`) are not satisfied.
    ///
    /// - `"block"` (default): refuse to launch and surface the install modal.
    /// - `"fall_back_to_host"`: run the command on the host without
    ///   sandboxing. Escape hatch for Windows or accepted-risk setups.
    /// - `"warn_once"`: proceed sandboxed and emit a one-shot notification.
    pub on_missing_runtime: Option<PaddleboardOnMissingRuntimeContent>,

    /// Whether PaddleBoard probes the host for sandbox prerequisites at all.
    /// When `false`, the gate always allows tools to proceed regardless of
    /// cached state — useful for users who find the probe slow or noisy.
    ///
    /// Default: true
    pub prereq_check_enabled: Option<bool>,
}

#[derive(
    Debug, Copy, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, MergeFrom,
)]
#[serde(rename_all = "snake_case")]
pub enum PaddleboardOnMissingRuntimeContent {
    #[default]
    Block,
    FallBackToHost,
    WarnOnce,
}
