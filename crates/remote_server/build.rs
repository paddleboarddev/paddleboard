#![allow(clippy::disallowed_methods, reason = "build scripts are exempt")]
use std::process::Command;

const PADDLEBOARD_MANIFEST: &str = include_str!("../paddleboard/Cargo.toml");

fn main() {
    let zed_cargo_toml =
        toml_edit::ImDocument::parse(PADDLEBOARD_MANIFEST).expect("failed to parse zed Cargo.toml");
    let version = zed_cargo_toml
        .get("package")
        .and_then(|package| package.get("version"))
        .and_then(toml_edit::Item::as_str)
        .expect("zed Cargo.toml must declare a package version string");
    println!("cargo:rustc-env=PADDLEBOARD_PKG_VERSION={version}");
    println!(
        "cargo:rustc-env=TARGET={}",
        std::env::var("TARGET").unwrap()
    );

    // Populate git sha environment variable if git is available
    println!("cargo:rerun-if-changed=../../.git/logs/HEAD");
    if let Some(output) = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
    {
        let git_sha = String::from_utf8_lossy(&output.stdout);
        let git_sha = git_sha.trim();

        println!("cargo:rustc-env=PADDLEBOARD_COMMIT_SHA={git_sha}");
    }
    if let Some(build_identifier) = option_env!("GITHUB_RUN_NUMBER") {
        println!("cargo:rustc-env=PADDLEBOARD_BUILD_ID={build_identifier}");
    }
}
