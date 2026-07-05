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

pub use paddleboard_sandbox_prereqs::{NativeBackend, SandboxStatus};
use paddleboard_sandbox_prereqs::{Os, resolve_native_backend};

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
    /// The Podman tier is missing but a "Native" tier is available; proceed
    /// sandboxed via `backend` (Apple `container` on macOS 26+, else the
    /// built-in libkrun microVM). The reason describes what was missing, for
    /// logging / debug surfaces.
    UseBuiltIn {
        reason: String,
        backend: NativeBackend,
    },
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
        Os::detect(),
    )
}

/// Pure core of [`decide_gate`], with the env override and host OS injected so
/// the policy matrix is testable without a real podman host, process
/// environment, or specific platform.
fn decide_gate_inner(
    prereqs: Option<&SandboxStatus>,
    builtin_capability: BuiltInCapability,
    settings: &SandboxSettings,
    force_builtin: bool,
    os: Os,
) -> SandboxGateDecision {
    // The concrete "Native" backend this host resolves to (Apple `container` on
    // macOS 26+, else libkrun; libkrun on Linux; none on Windows), or `None`
    // when this call site can't use the Native tier or no backend is available.
    let native_backend = |status: &SandboxStatus| -> Option<NativeBackend> {
        if builtin_capability != BuiltInCapability::Supported {
            return None;
        }
        resolve_native_backend(status, os)
    };

    // Dev/test override: force the Native tier even when Podman is healthy, so
    // the microVM / Apple-container path can be exercised without uninstalling
    // Podman.
    if force_builtin && let Some(status) = prereqs
        && let Some(backend) = native_backend(status)
    {
        return SandboxGateDecision::UseBuiltIn {
            reason: format!("{} is set", paddleboard_env_vars::SANDBOX_FORCE_BUILTIN_ENV_VAR),
            backend,
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

    // The middle rung: Podman tier missing, but the workload can run on a
    // Native tier (Apple `container` or libkrun) with no external installs.
    // Preferred over every missing-runtime policy because it keeps the sandbox
    // guarantee.
    if let Some(backend) = native_backend(status) {
        return SandboxGateDecision::UseBuiltIn { reason, backend };
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
    use paddleboard_sandbox_prereqs::{
        AppleContainerStatus, BuiltInStatus, GvisorStatus, PodmanStatus, SandboxStatus,
    };
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

    fn apple_unavailable() -> AppleContainerStatus {
        AppleContainerStatus::Unsupported {
            reason: "test platform",
        }
    }

    fn apple_available() -> AppleContainerStatus {
        AppleContainerStatus::Available {
            cli: PathBuf::from("/usr/local/bin/container"),
        }
    }

    fn happy() -> SandboxStatus {
        SandboxStatus {
            podman: PodmanStatus::Ready {
                version: "podman version 5.0.0".into(),
            },
            gvisor: GvisorStatus::Available,
            builtin: builtin_unavailable(),
            apple_container: apple_unavailable(),
        }
    }

    fn missing_podman() -> SandboxStatus {
        SandboxStatus {
            podman: PodmanStatus::Missing,
            gvisor: GvisorStatus::Unknown,
            builtin: builtin_unavailable(),
            apple_container: apple_unavailable(),
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
            apple_container: apple_unavailable(),
        }
    }

    /// The existing policy tests are platform-agnostic; run them as Linux so
    /// `builtin_available()` resolves to the libkrun Native backend. macOS-only
    /// Apple-container resolution is covered by dedicated tests below.
    fn gate(
        prereqs: Option<&SandboxStatus>,
        capability: BuiltInCapability,
        settings: &SandboxSettings,
    ) -> SandboxGateDecision {
        decide_gate_inner(prereqs, capability, settings, false, Os::Linux)
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
        let decision =
            decide_gate_inner(Some(&status), BuiltInCapability::Supported, &s, true, Os::Linux);
        assert!(matches!(decision, SandboxGateDecision::UseBuiltIn { .. }));
    }

    #[test]
    fn force_builtin_env_is_inert_when_builtin_unavailable() {
        let s = SandboxSettings::default();
        let decision = decide_gate_inner(
            Some(&missing_podman()),
            BuiltInCapability::Supported,
            &s,
            true,
            Os::Linux,
        );
        assert!(matches!(decision, SandboxGateDecision::Block { .. }));
    }

    #[test]
    fn mac_native_resolves_to_apple_container_when_available() {
        // macOS 26+ with the `container` CLI running: the Podman tier is missing,
        // so the gate routes to the Native tier and picks Apple's container.
        let s = SandboxSettings::default();
        let status = SandboxStatus {
            apple_container: apple_available(),
            ..missing_podman()
        };
        let decision = decide_gate_inner(
            Some(&status),
            BuiltInCapability::Supported,
            &s,
            false,
            Os::MacOs,
        );
        assert_eq!(
            decision,
            SandboxGateDecision::UseBuiltIn {
                reason: "podman is not installed on this host".to_string(),
                backend: NativeBackend::AppleContainer,
            }
        );
    }

    #[test]
    fn mac_native_falls_back_to_libkrun_without_apple_container() {
        // macOS 13–25 (or 26 without the CLI): Apple container unavailable but
        // libkrun present → the PR #43 built-in path.
        let s = SandboxSettings::default();
        let status = SandboxStatus {
            builtin: builtin_available(),
            apple_container: apple_unavailable(),
            ..missing_podman()
        };
        let decision = decide_gate_inner(
            Some(&status),
            BuiltInCapability::Supported,
            &s,
            false,
            Os::MacOs,
        );
        match decision {
            SandboxGateDecision::UseBuiltIn { backend, .. } => {
                assert_eq!(backend, NativeBackend::BuiltInKrun);
            }
            other => panic!("expected UseBuiltIn(BuiltInKrun), got {other:?}"),
        }
    }
}
