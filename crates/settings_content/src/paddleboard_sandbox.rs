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

    /// Which sandbox backend PaddleBoard should use, chosen at setup time.
    ///
    /// - `"native"`: the OS-native, zero-install tier — Apple `container` on
    ///   macOS 26+ (else the bundled libkrun microVM), or libkrun over KVM on
    ///   Linux. Not available on Windows.
    /// - `"podman"`: the Podman + gVisor (`runsc`) tier.
    ///
    /// The gate honors this explicitly: a machine that picks `"native"` uses
    /// the Native tier even when Podman is also installed, and one that picks
    /// `"podman"` is never silently rerouted to Native when Podman is missing —
    /// it applies `on_missing_runtime` instead.
    ///
    /// Default: `"native"` on macOS, `"podman"` on Linux and Windows.
    pub preferred_backend: Option<PaddleboardPreferredBackendContent>,
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

// No `Default` derive: there is no cross-platform default backend. Absence of
// the field (`None`) means "use the per-platform default", resolved in
// `paddleboard_sandbox_settings`.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, MergeFrom)]
#[serde(rename_all = "snake_case")]
pub enum PaddleboardPreferredBackendContent {
    Native,
    Podman,
}
