fn main() {
    let cargo_toml =
        std::fs::read_to_string("../paddleboard/Cargo.toml").expect("Failed to read crat../paddleboard/Cargo.toml");
    let version = cargo_toml
        .lines()
        .find(|line| line.starts_with("version = "))
        .expect("Version not found in crat../paddleboard/Cargo.toml")
        .split('=')
        .nth(1)
        .expect("Invalid version format")
        .trim()
        .trim_matches('"');
    println!("cargo:rustc-env=PADDLEBOARD_PKG_VERSION={}", version);
}
