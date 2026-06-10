# Building PaddleBoard

PaddleBoard builds with [Cargo](https://doc.rust-lang.org/cargo/), the same toolchain as upstream Zed. Start by cloning **this** repository:

```sh
git clone https://github.com/paddleboarddev/paddleboard.git
cd paddleboard
```

Then install the platform prerequisites below, and you're ready to build.

> The legacy guides under `docs/src/development/` are inherited from upstream Zed and describe building *Zed* — they're kept in sync for merge hygiene, but **this file** is the PaddleBoard guide. The system dependencies are identical; only the repository, binary names, and bundle scripts differ.

## macOS (primary platform)

macOS on Apple Silicon is what PaddleBoard's releases ship for and where it's most tested.

1. Install [rustup](https://www.rust-lang.org/tools/install).
2. Install [Xcode](https://apps.apple.com/us/app/xcode/id497799835?mt=12) from the App Store (launch it once and install the default macOS components), then the command line tools:

   ```sh
   xcode-select --install
   sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer
   sudo xcodebuild -license accept
   ```

3. Install `cmake` (required by a wasmtime dependency):

   ```sh
   brew install cmake
   ```

**Run from source:**

```sh
cargo run -p paddleboard
```

**Build a proper `PaddleBoard.app`** — with the paddle icon, dock name, `paddleboard://` URL scheme, and the `paddleboard` CLI (a plain `cargo build` produces a bare executable macOS shows generically):

```sh
./script/bundle-mac -d -o      # debug .app, opens when done
./script/bundle-mac -d -o -i   # …and install to /Applications (also refreshes the `paddleboard` CLI symlink)
./script/bundle-mac -o         # release .app + .dmg (what the release pipeline ships)
```

## Linux

Builds are expected to work but get less testing than macOS.

1. Install [rustup](https://www.rust-lang.org/tools/install).
2. Install the system libraries:

   ```sh
   script/linux
   ```

   (To install them manually instead, the package list lives inside `script/linux`.)

**Run from source:**

```sh
cargo run -p paddleboard
```

**Install a development build** — builds in release mode, installs the binary to `~/.local/bin`, and adds `.desktop` files:

```sh
./script/install-linux
```

> **Linker errors mentioning `aws_lc_sys` / `__isoc23_sscanf`?** That's a known aws-lc-rs incompatibility with GCC ≥ 14. Work around it with `export REMOTE_SERVER_TARGET=x86_64-unknown-linux-gnu` before `script/install-linux`. Details: [zed#24880](https://github.com/zed-industries/zed/issues/24880).

## Windows

Builds are expected to work but get the least testing. The toolchain setup is the most involved:

1. Install [rustup](https://www.rust-lang.org/tools/install).
2. Install [Visual Studio](https://visualstudio.microsoft.com/downloads/) with the **Desktop development with C++** workload, including the `MSVC C++ x64/x86 build tools` and `Spectre-mitigated libs` optional components — or the slimmer [Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) variant (then build from the "developer" shell so the environment is initialized).
3. Install the **Windows 11 (or 10) SDK** — at least `10.0.20348.0`, from the [Windows SDK Archive](https://developer.microsoft.com/windows/downloads/windows-sdk/).
4. Install [CMake](https://cmake.org/download) and make sure it's on `PATH`.

The exact Visual Studio component lists (and PostgreSQL notes for the collab-server tests) are in the inherited [Windows guide](./docs/src/development/windows.md) — its dependency sections apply verbatim; just build *this* repo and substitute `paddleboard` for `zed` in any command.

**Run from source:**

```sh
cargo run -p paddleboard
```

## Tests and lints

```sh
cargo test --workspace    # tests
./script/clippy           # lints — use this, not `cargo clippy` (it carries the project's flags)
```

## Troubleshooting

- **Cargo errors about a missing crate or feature right after pulling** — rerun with a clean fetch (`cargo fetch`) first; the weekly upstream merges occasionally move dependencies.
- Anything platform-specific not covered here likely behaves exactly as upstream Zed; the inherited guides in [`docs/src/development/`](./docs/src/development/) go deeper (remember they name Zed and the Zed repo).
- Still stuck? [Open an issue](https://github.com/paddleboarddev/paddleboard/issues).
