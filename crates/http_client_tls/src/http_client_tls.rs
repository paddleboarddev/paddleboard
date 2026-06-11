use std::sync::OnceLock;

use rustls::ClientConfig;
use rustls_platform_verifier::ConfigVerifierExt;

static TLS_CONFIG: OnceLock<rustls::ClientConfig> = OnceLock::new();

pub fn tls_config() -> ClientConfig {
    TLS_CONFIG
        .get_or_init(|| {
            // PaddleBoard: ring instead of aws_lc_rs so the static musl
            // remote_server links with GCC >= 14 / glibc 2.38+ (zed#24880).
            // This only errors if the default provider has already
            // been installed. We can ignore this `Result`.
            rustls::crypto::ring::default_provider()
                .install_default()
                .ok();

            ClientConfig::with_platform_verifier()
        })
        .clone()
}
