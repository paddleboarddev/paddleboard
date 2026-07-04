//! Settings and policy logic for PaddleBoard's sandbox enforcement.
//!
//! When a sandboxed tool (one-shot exec, long-lived service, MCP stdio
//! transport) is about to spawn a podman container, it consults
//! [`decide_gate`] with the cached sandbox-prereqs status and the user's
//! [`SandboxSettings`]. The returned [`SandboxGateDecision`] tells the call
//! site whether to proceed sandboxed, fall back to running on the host, surface
//! a one-shot warning, or block entirely (so the agent gets a clear error and
//! the user sees the install modal).

use std::sync::atomic::{AtomicBool, Ordering};

use gpui::App;
use serde::Deserialize;
use settings::{RegisterSetting, Settings};

pub use paddleboard_sandbox_prereqs::SandboxStatus;

/// Force-link the crate so the `RegisterSetting` inventory entry for
/// [`SandboxSettings`] is reachable. The derive emits an `inventory::submit!`
/// block, but the linker can drop a crate that has no externally-visible call
/// sites — calling this function from PaddleBoard's main keeps the symbol
/// alive.
pub fn init(_cx: &mut App) {}

static WARN_ONCE_SHOWN: AtomicBool = AtomicBool::new(false);

/// Returns `true` the first time it is called in a process, and `false`
/// thereafter. Used by the `warn_once` policy to ensure callers only emit a
/// notification once per session.
pub fn claim_warn_once_slot() -> bool {
    !WARN_ONCE_SHOWN.swap(true, Ordering::SeqCst)
}


/// Policy for what to do when [`SandboxStatus::is_satisfied`] is false at the
/// point a sandboxed tool tries to spawn.
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnMissingRuntime {
    /// Refuse to launch and surface the install modal. Keeps the sandbox
    /// guarantee honest; agent receives a clear error.
    #[default]
    Block,
    /// Bypass the container and run the command directly on the host. Escape
    /// hatch for environments where the sandbox stack is genuinely unavailable
    /// (Windows, CI) or the user has accepted the risk.
    FallBackToHost,
    /// Proceed sandboxed but emit a one-shot notification with install
    /// guidance. Useful while users are still onboarding.
    WarnOnce,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, RegisterSetting)]
pub struct SandboxSettings {
    /// Policy applied when prereqs are missing at tool-launch time.
    pub on_missing_runtime: OnMissingRuntime,
    /// Whether to probe the host for sandbox prereqs at all. When `false`,
    /// `decide_gate` always returns [`SandboxGateDecision::Allow`], regardless
    /// of cached state.
    pub prereq_check_enabled: bool,
}

impl Default for SandboxSettings {
    fn default() -> Self {
        Self {
            on_missing_runtime: OnMissingRuntime::Block,
            prereq_check_enabled: true,
        }
    }
}

impl Settings for SandboxSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let Some(content) = content.paddleboard_sandbox.as_ref() else {
            return Self::default();
        };
        Self {
            on_missing_runtime: content
                .on_missing_runtime
                .as_ref()
                .map(|v| (*v).into())
                .unwrap_or_default(),
            prereq_check_enabled: content.prereq_check_enabled.unwrap_or(true),
        }
    }
}

impl From<settings::PaddleboardOnMissingRuntimeContent> for OnMissingRuntime {
    fn from(value: settings::PaddleboardOnMissingRuntimeContent) -> Self {
        match value {
            settings::PaddleboardOnMissingRuntimeContent::Block => OnMissingRuntime::Block,
            settings::PaddleboardOnMissingRuntimeContent::FallBackToHost => {
                OnMissingRuntime::FallBackToHost
            }
            settings::PaddleboardOnMissingRuntimeContent::WarnOnce => OnMissingRuntime::WarnOnce,
        }
    }
}

/// What a sandbox-aware call site should do given the current prereqs status
/// and user policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxGateDecision {
    /// Proceed sandboxed via the Podman + gVisor tier.
    Allow,
    /// The Podman tier is missing but the built-in libkrun microVM tier is
    /// available; proceed sandboxed via a microVM. The reason describes what
    /// was missing, for logging / debug surfaces.
    UseBuiltIn { reason: String },
    /// Prereqs are missing and the user has opted to run on the host instead.
    /// The reason is included for logging / debug surfaces.
    FallBackToHost { reason: String },
    /// Prereqs are missing and policy is `block`. The call site should
    /// surface the install modal and return this error to its caller.
    Block { reason: String },
    /// Prereqs are missing but policy is `warn_once`. The call site should
    /// emit a notification with this message and then proceed sandboxed.
    WarnOnce { reason: String },
}

/// Whether a call site can run its workload on the built-in libkrun microVM
/// tier. Phase 1: only the one-shot `sandbox_tool` can; the service tool, MCP
/// transport, and REPL kernels stay Podman-only, so they pass `Unsupported`
/// and see exactly the pre-built-in policy behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltInCapability {
    Supported,
    Unsupported,
}

/// Decide what to do given the cached probe result and user policy.
pub fn decide_gate(
    prereqs: Option<&SandboxStatus>,
    builtin_capability: BuiltInCapability,
    settings: &SandboxSettings,
) -> SandboxGateDecision {
    decide_gate_inner(
        prereqs,
        builtin_capability,
        settings,
        *paddleboard_env_vars::PADDLEBOARD_SANDBOX_FORCE_BUILTIN,
    )
}

/// Pure core of [`decide_gate`], with the env override injected so the policy
/// matrix is testable without a real podman host or process environment.
fn decide_gate_inner(
    prereqs: Option<&SandboxStatus>,
    builtin_capability: BuiltInCapability,
    settings: &SandboxSettings,
    force_builtin: bool,
) -> SandboxGateDecision {
    let builtin_viable = |status: &SandboxStatus| {
        builtin_capability == BuiltInCapability::Supported && status.builtin.is_available()
    };

    // Dev/test override: force the built-in tier even when Podman is healthy,
    // so the microVM path can be exercised without uninstalling Podman.
    if force_builtin && let Some(status) = prereqs
        && builtin_viable(status)
    {
        return SandboxGateDecision::UseBuiltIn {
            reason: format!("{} is set", paddleboard_env_vars::SANDBOX_FORCE_BUILTIN_ENV_VAR),
        };
    }

    if !settings.prereq_check_enabled {
        return SandboxGateDecision::Allow;
    }

    // If we have no cached status yet (probe still in flight, or running on a
    // platform where the probe failed entirely), don't block — the underlying
    // podman invocation will surface its own error if it's truly broken. The
    // gate exists to *improve* UX, not to invent new failure modes.
    let Some(status) = prereqs else {
        return SandboxGateDecision::Allow;
    };

    if status.is_satisfied() {
        return SandboxGateDecision::Allow;
    }

    let reason = describe_missing(status);

    // The middle rung: Podman tier missing, but the workload can run on the
    // built-in microVM tier with no installs. Preferred over every
    // missing-runtime policy because it keeps the sandbox guarantee.
    if builtin_viable(status) {
        return SandboxGateDecision::UseBuiltIn { reason };
    }

    match settings.on_missing_runtime {
        OnMissingRuntime::Block => SandboxGateDecision::Block { reason },
        OnMissingRuntime::FallBackToHost => SandboxGateDecision::FallBackToHost { reason },
        OnMissingRuntime::WarnOnce => SandboxGateDecision::WarnOnce { reason },
    }
}

fn describe_missing(status: &SandboxStatus) -> String {
    use paddleboard_sandbox_prereqs::{GvisorStatus, PodmanStatus};
    match (&status.podman, &status.gvisor) {
        (PodmanStatus::Missing, _) => "podman is not installed on this host".to_string(),
        (PodmanStatus::InstalledNotRunning { .. }, _) => {
            "podman is installed but its daemon (machine) is not reachable".to_string()
        }
        (PodmanStatus::Ready { .. }, GvisorStatus::NotConfigured) => {
            "gVisor runtime (runsc) is not registered with podman".to_string()
        }
        (PodmanStatus::Ready { .. }, GvisorStatus::Unknown) => {
            "sandbox runtime status could not be determined".to_string()
        }
        _ => "sandbox prerequisites are not satisfied".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddleboard_sandbox_prereqs::{BuiltInStatus, GvisorStatus, PodmanStatus, SandboxStatus};
    use std::path::PathBuf;

    fn builtin_available() -> BuiltInStatus {
        BuiltInStatus::Available {
            libkrun: PathBuf::from("/opt/homebrew/lib/libkrun.dylib"),
            helper: PathBuf::from("/tmp/paddleboard-krun-helper"),
        }
    }

    fn builtin_unavailable() -> BuiltInStatus {
        BuiltInStatus::Unsupported {
            reason: "test platform",
        }
    }

    fn happy() -> SandboxStatus {
        SandboxStatus {
            podman: PodmanStatus::Ready {
                version: "podman version 5.0.0".into(),
            },
            gvisor: GvisorStatus::Available,
            builtin: builtin_unavailable(),
        }
    }

    fn missing_podman() -> SandboxStatus {
        SandboxStatus {
            podman: PodmanStatus::Missing,
            gvisor: GvisorStatus::Unknown,
            builtin: builtin_unavailable(),
        }
    }

    fn missing_podman_with_builtin() -> SandboxStatus {
        SandboxStatus {
            builtin: builtin_available(),
            ..missing_podman()
        }
    }

    fn missing_gvisor() -> SandboxStatus {
        SandboxStatus {
            podman: PodmanStatus::Ready {
                version: "podman version 5.0.0".into(),
            },
            gvisor: GvisorStatus::NotConfigured,
            builtin: builtin_unavailable(),
        }
    }

    fn gate(
        prereqs: Option<&SandboxStatus>,
        capability: BuiltInCapability,
        settings: &SandboxSettings,
    ) -> SandboxGateDecision {
        decide_gate_inner(prereqs, capability, settings, false)
    }

    #[test]
    fn allow_when_prereqs_are_satisfied() {
        let s = SandboxSettings::default();
        assert_eq!(
            gate(Some(&happy()), BuiltInCapability::Supported, &s),
            SandboxGateDecision::Allow
        );
    }

    #[test]
    fn allow_when_status_not_yet_probed() {
        let s = SandboxSettings::default();
        assert_eq!(
            gate(None, BuiltInCapability::Supported, &s),
            SandboxGateDecision::Allow
        );
    }

    #[test]
    fn allow_when_prereq_check_disabled() {
        let s = SandboxSettings {
            prereq_check_enabled: false,
            on_missing_runtime: OnMissingRuntime::Block,
        };
        assert_eq!(
            gate(
                Some(&missing_podman()),
                BuiltInCapability::Supported,
                &s
            ),
            SandboxGateDecision::Allow
        );
    }

    #[test]
    fn block_is_default_when_podman_missing() {
        let s = SandboxSettings::default();
        let decision = gate(Some(&missing_podman()), BuiltInCapability::Supported, &s);
        assert!(matches!(decision, SandboxGateDecision::Block { .. }));
    }

    #[test]
    fn block_when_gvisor_missing_with_default_policy() {
        let s = SandboxSettings::default();
        let decision = gate(Some(&missing_gvisor()), BuiltInCapability::Supported, &s);
        match decision {
            SandboxGateDecision::Block { reason } => {
                assert!(reason.contains("gVisor"));
            }
            other => panic!("expected Block, got {other:?}"),
        }
    }

    #[test]
    fn fall_back_to_host_policy_returns_fallback() {
        let s = SandboxSettings {
            prereq_check_enabled: true,
            on_missing_runtime: OnMissingRuntime::FallBackToHost,
        };
        let decision = gate(Some(&missing_podman()), BuiltInCapability::Supported, &s);
        assert!(matches!(decision, SandboxGateDecision::FallBackToHost { .. }));
    }

    #[test]
    fn warn_once_policy_returns_warning() {
        let s = SandboxSettings {
            prereq_check_enabled: true,
            on_missing_runtime: OnMissingRuntime::WarnOnce,
        };
        let decision = gate(Some(&missing_gvisor()), BuiltInCapability::Supported, &s);
        assert!(matches!(decision, SandboxGateDecision::WarnOnce { .. }));
    }

    #[test]
    fn builtin_outranks_every_missing_runtime_policy() {
        for policy in [
            OnMissingRuntime::Block,
            OnMissingRuntime::FallBackToHost,
            OnMissingRuntime::WarnOnce,
        ] {
            let s = SandboxSettings {
                prereq_check_enabled: true,
                on_missing_runtime: policy,
            };
            let decision = gate(
                Some(&missing_podman_with_builtin()),
                BuiltInCapability::Supported,
                &s,
            );
            assert!(
                matches!(decision, SandboxGateDecision::UseBuiltIn { .. }),
                "policy {policy:?} should be outranked by the built-in tier, got {decision:?}"
            );
        }
    }

    #[test]
    fn builtin_not_used_when_podman_tier_is_healthy() {
        let s = SandboxSettings::default();
        let status = SandboxStatus {
            builtin: builtin_available(),
            ..happy()
        };
        assert_eq!(
            gate(Some(&status), BuiltInCapability::Supported, &s),
            SandboxGateDecision::Allow
        );
    }

    #[test]
    fn unsupported_call_sites_keep_policy_behavior() {
        // The service tool / MCP transport can't run on the built-in tier in
        // phase 1; with `Unsupported` they must see exactly the old policy
        // behavior even when the built-in tier is available.
        let s = SandboxSettings::default();
        let decision = gate(
            Some(&missing_podman_with_builtin()),
            BuiltInCapability::Unsupported,
            &s,
        );
        assert!(matches!(decision, SandboxGateDecision::Block { .. }));
    }

    #[test]
    fn force_builtin_env_overrides_healthy_podman() {
        let s = SandboxSettings::default();
        let status = SandboxStatus {
            builtin: builtin_available(),
            ..happy()
        };
        let decision = decide_gate_inner(Some(&status), BuiltInCapability::Supported, &s, true);
        assert!(matches!(decision, SandboxGateDecision::UseBuiltIn { .. }));
    }

    #[test]
    fn force_builtin_env_is_inert_when_builtin_unavailable() {
        let s = SandboxSettings::default();
        let decision =
            decide_gate_inner(Some(&missing_podman()), BuiltInCapability::Supported, &s, true);
        assert!(matches!(decision, SandboxGateDecision::Block { .. }));
    }
}
