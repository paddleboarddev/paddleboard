pub use env_var::{EnvVar, bool_env_var, env_var};
use std::sync::LazyLock;

/// Whether Zed is running in stateless mode.
/// When true, Zed will use in-memory databases instead of persistent storage.
pub static PADDLEBOARD_STATELESS: LazyLock<bool> = bool_env_var!("PADDLEBOARD_STATELESS");

pub static PADDLEBOARD_OTEL_ENABLED: LazyLock<bool> = bool_env_var!("PADDLEBOARD_OTEL_ENABLED");

#[allow(dead_code)]
pub static PADDLEBOARD_OTEL_ENDPOINT: LazyLock<EnvVar> = env_var!("PADDLEBOARD_OTEL_ENDPOINT");

pub const SANDBOX_FORCE_BUILTIN_ENV_VAR: &str = "PADDLEBOARD_SANDBOX_FORCE_BUILTIN";

/// Dev/test override: route sandboxed tools through the built-in libkrun
/// microVM tier even when Podman + gVisor are healthy.
pub static PADDLEBOARD_SANDBOX_FORCE_BUILTIN: LazyLock<bool> =
    bool_env_var!(SANDBOX_FORCE_BUILTIN_ENV_VAR);
