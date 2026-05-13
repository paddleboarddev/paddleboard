fn main() {
    if let Ok(bundled) = std::env::var("PADDLEBOARD_BUNDLE") {
        println!("cargo:rustc-env=PADDLEBOARD_BUNDLE={}", bundled);
    }
}
