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

pub use paddleboard_sandbox_prereqs::{NativeBackend, PreferredBackend, SandboxStatus};
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
    /// The backend the user picked at setup time. The gate honors this
    /// explicitly (see [`decide_gate`]). Absent settings resolve to the
    /// per-platform default via [`PreferredBackend::platform_default`].
    pub preferred_backend: PreferredBackend,
}

impl Default for SandboxSettings {
    fn default() -> Self {
        Self {
            on_missing_runtime: OnMissingRuntime::Block,
            prereq_check_enabled: true,
            preferred_backend: PreferredBackend::platform_default(Os::detect()),
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
            // `PreferredBackend` is defined in `paddleboard_sandbox_prereqs`, so
            // the orphan rule rules out a `From` impl here — map inline instead.
            preferred_backend: content
                .preferred_backend
                .as_ref()
                .map(|v| match v {
                    settings::PaddleboardPreferredBackendContent::Native => {
                        PreferredBackend::Native
                    }
                    settings::PaddleboardPreferredBackendContent::Podman => {
                        PreferredBackend::Podman
                    }
                })
                .unwrap_or_else(|| PreferredBackend::platform_default(Os::detect())),
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
///
/// Precedence, honoring the user's explicit [`SandboxSettings::preferred_backend`]
/// (this is what resolves review finding #2 — no silent tier rerouting):
///
/// 1. `PADDLEBOARD_SANDBOX_FORCE_BUILTIN` dev override → Native, if resolvable.
/// 2. `prereq_check_enabled == false`, or no cached probe yet → `Allow`.
/// 3. **Preference = Native**: use the Native tier whenever it resolves for
///    this call site — *even if Podman is healthy*. If Native can't be used
///    here (unsupported call site, or no backend on this host), fall back to
///    Podman when it's satisfied, else apply `on_missing_runtime`.
/// 4. **Preference = Podman**: use Podman when satisfied. When it isn't, apply
///    `on_missing_runtime` — we do *not* silently reroute to Native, because
///    the user asked for Podman.
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

    match settings.preferred_backend {
        PreferredBackend::Native => {
            // Honor the choice: prefer Native wherever it resolves, even when
            // Podman is also healthy.
            if let Some(backend) = native_backend(status) {
                let reason = if status.is_satisfied() {
                    "the Native sandbox backend is preferred (paddleboard_sandbox.preferred_backend = \"native\")".to_string()
                } else {
                    describe_missing(status)
                };
                return SandboxGateDecision::UseBuiltIn { reason, backend };
            }
            // Native isn't usable here (unsupported call site, or no backend on
            // this host). Fall back to Podman when it's satisfied, else policy.
            if status.is_satisfied() {
                return SandboxGateDecision::Allow;
            }
            on_missing(settings, describe_missing(status))
        }
        PreferredBackend::Podman => {
            if status.is_satisfied() {
                return SandboxGateDecision::Allow;
            }
            // Podman was chosen but isn't satisfied — apply policy rather than
            // rerouting to Native behind the user's back.
            on_missing(settings, describe_missing(status))
        }
    }
}

fn on_missing(settings: &SandboxSettings, reason: String) -> SandboxGateDecision {
    match settings.on_missing_runtime {
        OnMissingRuntime::Block => SandboxGateDecision::Block { reason },
        OnMissingRuntime::FallBackToHost => SandboxGateDecision::FallBackToHost { reason },
        OnMissingRuntime::WarnOnce => SandboxGateDecision::WarnOnce { reason },
    }
}

/// The active sandbox tier for the representative one-shot `sandbox_tool` path,
/// derived from the *same* [`decide_gate`] logic the tool itself uses — so the
/// status shield and the gate can never disagree (review finding #6). Callers
/// that only need the shield's view use [`active_tier`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveTier {
    pub kind: ActiveTierKind,
    /// True when this tier only sandboxes one-shot commands. In phase 1 the
    /// Native tiers are one-shot-only: the service tool, MCP transport, and
    /// REPL kernels still require Podman + gVisor.
    pub one_shot_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveTierKind {
    /// The probe hasn't completed yet.
    Unknown,
    /// Podman + gVisor will run the workload.
    Podman,
    /// Apple's `container` tier will run one-shot commands.
    AppleContainer,
    /// The bundled libkrun microVM tier will run one-shot commands.
    BuiltInKrun,
    /// No sandbox — the command runs on the host (`fall_back_to_host`).
    Host,
    /// Nothing can run the workload; the tool would be blocked.
    Unavailable,
}

/// Shield-facing derivation of the currently active tier. Runs the real
/// [`decide_gate`] for the one-shot `sandbox_tool` capability and maps the
/// decision to a tier, guaranteeing the shield reflects what the gate would
/// actually do.
pub fn active_tier(prereqs: Option<&SandboxStatus>, settings: &SandboxSettings) -> ActiveTier {
    active_tier_inner(
        prereqs,
        settings,
        *paddleboard_env_vars::PADDLEBOARD_SANDBOX_FORCE_BUILTIN,
        Os::detect(),
    )
}

fn active_tier_inner(
    prereqs: Option<&SandboxStatus>,
    settings: &SandboxSettings,
    force_builtin: bool,
    os: Os,
) -> ActiveTier {
    // "Checking…" while the first probe is in flight, but only when the check
    // is enabled — a disabled check means the gate always allows, so the tier
    // is genuinely Podman rather than unknown.
    if prereqs.is_none() && settings.prereq_check_enabled {
        return ActiveTier {
            kind: ActiveTierKind::Unknown,
            one_shot_only: false,
        };
    }
    match decide_gate_inner(
        prereqs,
        BuiltInCapability::Supported,
        settings,
        force_builtin,
        os,
    ) {
        SandboxGateDecision::Allow | SandboxGateDecision::WarnOnce { .. } => ActiveTier {
            kind: ActiveTierKind::Podman,
            one_shot_only: false,
        },
        SandboxGateDecision::UseBuiltIn { backend, .. } => {
            let kind = match backend {
                NativeBackend::AppleContainer => ActiveTierKind::AppleContainer,
                NativeBackend::BuiltInKrun => ActiveTierKind::BuiltInKrun,
            };
            ActiveTier {
                kind,
                one_shot_only: true,
            }
        }
        SandboxGateDecision::FallBackToHost { .. } => ActiveTier {
            kind: ActiveTierKind::Host,
            one_shot_only: false,
        },
        SandboxGateDecision::Block { .. } => ActiveTier {
            kind: ActiveTierKind::Unavailable,
            one_shot_only: false,
        },
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

    fn settings(preferred: PreferredBackend, policy: OnMissingRuntime) -> SandboxSettings {
        SandboxSettings {
            on_missing_runtime: policy,
            prereq_check_enabled: true,
            preferred_backend: preferred,
        }
    }

    /// The policy tests run as Linux with Podman preferred so `builtin_available()`
    /// resolves to the libkrun Native backend and the missing-runtime policies are
    /// exercised. macOS Apple-container resolution and the Native preference are
    /// covered by dedicated tests below.
    fn gate(
        prereqs: Option<&SandboxStatus>,
        capability: BuiltInCapability,
        settings: &SandboxSettings,
    ) -> SandboxGateDecision {
        decide_gate_inner(prereqs, capability, settings, false, Os::Linux)
    }

    #[test]
    fn allow_when_prereqs_are_satisfied() {
        let s = settings(PreferredBackend::Podman, OnMissingRuntime::Block);
        assert_eq!(
            gate(Some(&happy()), BuiltInCapability::Supported, &s),
            SandboxGateDecision::Allow
        );
    }

    #[test]
    fn allow_when_status_not_yet_probed() {
        let s = settings(PreferredBackend::Podman, OnMissingRuntime::Block);
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
            preferred_backend: PreferredBackend::Podman,
        };
        assert_eq!(
            gate(Some(&missing_podman()), BuiltInCapability::Supported, &s),
            SandboxGateDecision::Allow
        );
    }

    #[test]
    fn block_is_default_when_podman_missing() {
        let s = settings(PreferredBackend::Podman, OnMissingRuntime::Block);
        let decision = gate(Some(&missing_podman()), BuiltInCapability::Supported, &s);
        assert!(matches!(decision, SandboxGateDecision::Block { .. }));
    }

    #[test]
    fn block_when_gvisor_missing_with_default_policy() {
        let s = settings(PreferredBackend::Podman, OnMissingRuntime::Block);
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
        let s = settings(PreferredBackend::Podman, OnMissingRuntime::FallBackToHost);
        let decision = gate(Some(&missing_podman()), BuiltInCapability::Supported, &s);
        assert!(matches!(decision, SandboxGateDecision::FallBackToHost { .. }));
    }

    #[test]
    fn warn_once_policy_returns_warning() {
        let s = settings(PreferredBackend::Podman, OnMissingRuntime::WarnOnce);
        let decision = gate(Some(&missing_gvisor()), BuiltInCapability::Supported, &s);
        assert!(matches!(decision, SandboxGateDecision::WarnOnce { .. }));
    }

    #[test]
    fn native_preference_uses_builtin_regardless_of_policy() {
        // With Native preferred and a Native backend available, the microVM tier
        // is used no matter what the missing-runtime policy is.
        for policy in [
            OnMissingRuntime::Block,
            OnMissingRuntime::FallBackToHost,
            OnMissingRuntime::WarnOnce,
        ] {
            let s = settings(PreferredBackend::Native, policy);
            let decision = gate(
                Some(&missing_podman_with_builtin()),
                BuiltInCapability::Supported,
                &s,
            );
            assert!(
                matches!(decision, SandboxGateDecision::UseBuiltIn { .. }),
                "policy {policy:?} with Native preferred should use the built-in tier, got {decision:?}"
            );
        }
    }

    // Finding #2, forward: a user who picks Native gets Native even when Podman
    // is also installed and healthy — no silent preference for Podman.
    #[test]
    fn native_preference_uses_native_even_when_podman_healthy() {
        let s = settings(PreferredBackend::Native, OnMissingRuntime::Block);
        let status = SandboxStatus {
            builtin: builtin_available(),
            ..happy()
        };
        match gate(Some(&status), BuiltInCapability::Supported, &s) {
            SandboxGateDecision::UseBuiltIn { backend, .. } => {
                assert_eq!(backend, NativeBackend::BuiltInKrun);
            }
            other => panic!("expected UseBuiltIn, got {other:?}"),
        }
    }

    // Finding #2, reverse: a user who picks Podman is never silently rerouted to
    // Native when Podman is missing — even though a built-in backend is present.
    #[test]
    fn podman_preference_blocks_when_podman_missing_not_native() {
        let s = settings(PreferredBackend::Podman, OnMissingRuntime::Block);
        let decision = gate(
            Some(&missing_podman_with_builtin()),
            BuiltInCapability::Supported,
            &s,
        );
        match decision {
            SandboxGateDecision::Block { reason } => assert!(reason.contains("podman")),
            other => panic!("expected Block (podman-missing), got {other:?}"),
        }
    }

    #[test]
    fn podman_preference_uses_podman_when_healthy_even_with_builtin() {
        let s = settings(PreferredBackend::Podman, OnMissingRuntime::Block);
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
        // phase 1; with `Unsupported` they must see the policy behavior even
        // when the built-in tier is available and Native is preferred.
        let s = settings(PreferredBackend::Native, OnMissingRuntime::Block);
        let decision = gate(
            Some(&missing_podman_with_builtin()),
            BuiltInCapability::Unsupported,
            &s,
        );
        assert!(matches!(decision, SandboxGateDecision::Block { .. }));
    }

    #[test]
    fn force_builtin_env_overrides_healthy_podman() {
        // Force wins even when the user preferred Podman and Podman is healthy.
        let s = settings(PreferredBackend::Podman, OnMissingRuntime::Block);
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
        let s = settings(PreferredBackend::Podman, OnMissingRuntime::Block);
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
        // macOS 26+ with the `container` CLI running, Podman missing, Native
        // preferred → the gate picks Apple's container.
        let s = settings(PreferredBackend::Native, OnMissingRuntime::Block);
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
        let s = settings(PreferredBackend::Native, OnMissingRuntime::Block);
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

    // Finding #6: the shield's active-tier derivation must never contradict the
    // gate, because it is derived from the same `decide_gate_inner`.
    #[test]
    fn active_tier_agrees_with_gate_decision() {
        struct Case {
            name: &'static str,
            prereqs: Option<SandboxStatus>,
            preferred: PreferredBackend,
            policy: OnMissingRuntime,
            os: Os,
            expect_kind: ActiveTierKind,
            expect_one_shot: bool,
        }
        let cases = [
            Case {
                name: "podman healthy",
                prereqs: Some(happy()),
                preferred: PreferredBackend::Podman,
                policy: OnMissingRuntime::Block,
                os: Os::Linux,
                expect_kind: ActiveTierKind::Podman,
                expect_one_shot: false,
            },
            Case {
                name: "native libkrun",
                prereqs: Some(missing_podman_with_builtin()),
                preferred: PreferredBackend::Native,
                policy: OnMissingRuntime::Block,
                os: Os::Linux,
                expect_kind: ActiveTierKind::BuiltInKrun,
                expect_one_shot: true,
            },
            Case {
                name: "podman missing, blocked",
                prereqs: Some(missing_podman()),
                preferred: PreferredBackend::Podman,
                policy: OnMissingRuntime::Block,
                os: Os::Linux,
                expect_kind: ActiveTierKind::Unavailable,
                expect_one_shot: false,
            },
            Case {
                name: "podman missing, host fallback",
                prereqs: Some(missing_podman()),
                preferred: PreferredBackend::Podman,
                policy: OnMissingRuntime::FallBackToHost,
                os: Os::Linux,
                expect_kind: ActiveTierKind::Host,
                expect_one_shot: false,
            },
            Case {
                name: "not probed yet",
                prereqs: None,
                preferred: PreferredBackend::Podman,
                policy: OnMissingRuntime::Block,
                os: Os::Linux,
                expect_kind: ActiveTierKind::Unknown,
                expect_one_shot: false,
            },
        ];
        for case in cases {
            let s = settings(case.preferred, case.policy);
            let tier = active_tier_inner(case.prereqs.as_ref(), &s, false, case.os);
            assert_eq!(
                tier.kind, case.expect_kind,
                "case {}: unexpected tier kind",
                case.name
            );
            assert_eq!(
                tier.one_shot_only, case.expect_one_shot,
                "case {}: unexpected one_shot flag",
                case.name
            );
            // The tier must be consistent with what the gate would actually do.
            let decision =
                decide_gate_inner(case.prereqs.as_ref(), BuiltInCapability::Supported, &s, false, case.os);
            let consistent = match (tier.kind, &decision) {
                (ActiveTierKind::Podman, SandboxGateDecision::Allow)
                | (ActiveTierKind::Podman, SandboxGateDecision::WarnOnce { .. })
                | (ActiveTierKind::AppleContainer, SandboxGateDecision::UseBuiltIn { .. })
                | (ActiveTierKind::BuiltInKrun, SandboxGateDecision::UseBuiltIn { .. })
                | (ActiveTierKind::Host, SandboxGateDecision::FallBackToHost { .. })
                | (ActiveTierKind::Unavailable, SandboxGateDecision::Block { .. })
                | (ActiveTierKind::Unknown, _) => true,
                _ => false,
            };
            assert!(
                consistent,
                "case {}: tier {:?} disagrees with gate {:?}",
                case.name, tier.kind, decision
            );
        }
    }
}
