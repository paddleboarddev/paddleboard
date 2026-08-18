//! PaddleBoard: guards `--user-data-dir` against a silent upstream revert.
//!
//! `logs_dir()`'s macOS branch used to hardcode `~/Library/Logs`, so a second
//! instance started with `--user-data-dir` wrote into — and rotated — the
//! primary profile's log, despite the flag's own `--help` listing logs as
//! covered and Linux already honouring it.
//!
//! This lives in `tests/` rather than inside `paths.rs` deliberately: it's a
//! file upstream does not have, so a merge can never conflict on it, and it
//! fails loudly if upstream rewrites `logs_dir()` and drops the override.
//!
//! One test per binary on purpose — the paths in this crate are `OnceLock`
//! globals, so `set_custom_data_dir` can only be called once per process.

#[test]
fn custom_data_dir_redirects_logs_dir() {
    let tmp = std::env::temp_dir().join("paddleboard-custom-data-dir-test");
    let _ = std::fs::remove_dir_all(&tmp);
    paths::set_custom_data_dir(tmp.to_str().expect("temp dir path is not UTF-8"));

    let logs = paths::logs_dir();
    // set_custom_data_dir canonicalizes, which on macOS resolves /var -> /private/var.
    let expected_root = tmp.canonicalize().expect("custom data dir was not created");

    assert!(
        logs.starts_with(&expected_root),
        "logs_dir() = {logs:?}, expected under {expected_root:?}"
    );
    assert!(logs.ends_with("logs"), "logs_dir() = {logs:?}");

    if let Some(home) = dirs::home_dir() {
        assert!(
            !logs.starts_with(home.join("Library/Logs")),
            "logs_dir() still resolves to the primary profile: {logs:?}"
        );
    }
}
