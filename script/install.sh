#!/usr/bin/env sh
set -eu

# PaddleBoard: installs a locally built tarball (PADDLEBOARD_BUNDLE_PATH, set by
# script/install-linux) into ~/.local/. PaddleBoard has no hosted release
# server, so unlike upstream Zed this script never downloads anything —
# invoking it without a local bundle is an error.

main() {
    platform="$(uname -s)"
    arch="$(uname -m)"
    channel="${PADDLEBOARD_CHANNEL:-stable}"
    # Use TMPDIR if available (for environments with non-standard temp directories)
    if [ -n "${TMPDIR:-}" ] && [ -d "${TMPDIR}" ]; then
        temp="$(mktemp -d "$TMPDIR/paddleboard-XXXXXX")"
    else
        temp="$(mktemp -d "/tmp/paddleboard-XXXXXX")"
    fi

    if [ "$platform" = "Darwin" ]; then
        platform="macos"
    elif [ "$platform" = "Linux" ]; then
        platform="linux"
    else
        echo "Unsupported platform $platform"
        exit 1
    fi

    case "$platform-$arch" in
        macos-arm64* | linux-arm64* | linux-armhf | linux-aarch64)
            arch="aarch64"
            ;;
        macos-x86* | linux-x86* | linux-i686*)
            arch="x86_64"
            ;;
        *)
            echo "Unsupported platform or architecture"
            exit 1
            ;;
    esac

    if command -v curl >/dev/null 2>&1; then
        curl () {
            command curl -fL "$@"
        }
    elif command -v wget >/dev/null 2>&1; then
        curl () {
            wget -O- "$@"
        }
    else
        echo "Could not find 'curl' or 'wget' in your path"
        exit 1
    fi

    "$platform" "$@"

    if [ "$(command -v paddleboard)" = "$HOME/.local/bin/paddleboard" ]; then
        echo "PaddleBoard has been installed. Run with 'paddleboard'"
    else
        echo "To run PaddleBoard from your terminal, you must add ~/.local/bin to your PATH"
        echo "Run:"

        case "$SHELL" in
            *zsh)
                echo "   echo 'export PATH=\$HOME/.local/bin:\$PATH' >> ~/.zshrc"
                echo "   source ~/.zshrc"
                ;;
            *fish)
                echo "   fish_add_path -U $HOME/.local/bin"
                ;;
            *)
                echo "   echo 'export PATH=\$HOME/.local/bin:\$PATH' >> ~/.bashrc"
                echo "   source ~/.bashrc"
                ;;
        esac

        echo "To run PaddleBoard now, '~/.local/bin/paddleboard'"
    fi
}

linux() {
    if [ -n "${PADDLEBOARD_BUNDLE_PATH:-}" ]; then
        cp "$PADDLEBOARD_BUNDLE_PATH" "$temp/paddleboard-linux-$arch.tar.gz"
    else
        echo "PaddleBoard has no hosted Linux releases to download."
        echo "Build and install from source instead:  ./script/install-linux"
        exit 1
    fi

    suffix=""
    if [ "$channel" != "stable" ]; then
        suffix="-$channel"
    fi

    appid=""
    case "$channel" in
      stable)
        appid="dev.paddleboard.PaddleBoard"
        ;;
      nightly)
        appid="dev.paddleboard.PaddleBoard-Nightly"
        ;;
      preview)
        appid="dev.paddleboard.PaddleBoard-Preview"
        ;;
      dev)
        appid="dev.paddleboard.PaddleBoard-Dev"
        ;;
      *)
        echo "Unknown release channel: ${channel}. Using stable app ID."
        appid="dev.paddleboard.PaddleBoard"
        ;;
    esac

    # Unpack
    rm -rf "$HOME/.local/paddleboard$suffix.app"
    mkdir -p "$HOME/.local/paddleboard$suffix.app"
    tar -xzf "$temp/paddleboard-linux-$arch.tar.gz" -C "$HOME/.local/"

    # Setup ~/.local directories
    mkdir -p "$HOME/.local/bin" "$HOME/.local/share/applications"

    # Link the binary
    ln -sf "$HOME/.local/paddleboard$suffix.app/bin/paddleboard" "$HOME/.local/bin/paddleboard"

    # Copy .desktop file
    desktop_file_path="$HOME/.local/share/applications/${appid}.desktop"
    src_dir="$HOME/.local/paddleboard$suffix.app/share/applications"
    cp "$src_dir/${appid}.desktop" "${desktop_file_path}"
    sed -i "s|Icon=paddleboard|Icon=$HOME/.local/paddleboard$suffix.app/share/icons/hicolor/512x512/apps/paddleboard.png|g" "${desktop_file_path}"
    sed -i "s|Exec=paddleboard|Exec=$HOME/.local/paddleboard$suffix.app/bin/paddleboard|g" "${desktop_file_path}"
}

macos() {
    echo "PaddleBoard has no hosted macOS releases to download."
    echo "Build and install from source instead:  ./script/bundle-mac -d -o -i"
    exit 1
}

main "$@"
