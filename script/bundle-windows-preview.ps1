# PaddleBoard: build the UNSIGNED Windows preview bundle.
#
# Decided 2026-10-05 for the v0.3.0 beta: Windows ships as a preview, not a
# release-grade build. What that means, concretely:
#   - unsigned: no Azure Trusted Signing account exists for PaddleBoard, so the
#     first run trips SmartScreen ("More info" -> "Run anyway"). /beta says so.
#   - a zip, not an installer: the inherited Inno Setup definition
#     (resources/windows/zed.iss) is Zed-branded through and through (AppId,
#     publisher, mutex, registry keys, Explorer integration). Porting it is a
#     separate job; a zip with PaddleBoard.exe is honest for a preview.
#   - no auto-update: the updater's Windows path expects an installer exe named
#     PaddleBoard.exe as the release asset, which this does not produce. Users
#     download the next zip by hand. Documented in docs/windows.md.
#   - no managed Local Models: the bundled llama.cpp runtime has no Windows
#     build. Bring-your-own-key, or any OpenAI-compatible endpoint, still works.
#
# This is a PaddleBoard-owned script, deliberately NOT an edit of the inherited
# script/bundle-windows.ps1 (which stays on check-zed-isms' allowlist as
# un-ported): upstream cannot conflict on a file it doesn't have, and the
# inherited script's signing, Sentry, appx and Inno steps are all things this
# bundle does not do.
#
# What it does:
#   1. cargo build --release for `paddleboard` (the app) and `cli`, MSVC target.
#   2. Lay out a folder: PaddleBoard.exe, bin\paddleboard.exe (the CLI),
#      conpty.dll + OpenConsole.exe (ConPTY, same nupkg upstream ships),
#      amd_ags_x64.dll (AMD GPU Services, same SDK upstream ships), licenses,
#      and a README-PREVIEW.txt that says what this is and isn't.
#   3. Zip it to target\PaddleBoard-<arch>-windows-preview.zip and print its
#      SHA-256, which goes in the release notes.
#
# Usage (from the workspace root, in CI or a VS 2022 developer shell):
#   script/bundle-windows-preview.ps1 [-Architecture x86_64|aarch64]

[CmdletBinding()]
Param(
    [Parameter()][Alias('a')][string]$Architecture = "x86_64"
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$ProgressPreference = 'SilentlyContinue'

$workspace = (Resolve-Path "$PSScriptRoot\..").Path
Set-Location $workspace

$target = "$Architecture-pc-windows-msvc"
$targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { "$workspace\target" }
$cargoOutDir = "$targetDir\$target\release"
$channel = (Get-Content "crates\paddleboard\RELEASE_CHANNEL").Trim()
$version = (& "$workspace\script\get-crate-version.ps1" paddleboard).Trim()
$bundleName = "PaddleBoard-$Architecture-windows-preview"
$stageDir = "$targetDir\$bundleName"
$zipPath = "$targetDir\$bundleName.zip"

Write-Host "PaddleBoard $version ($channel) -> $zipPath"

# A VS developer shell gives cmake-based deps (aws-lc-sys, zstd, ...) the MSVC
# toolchain without guessing. Hosted runners carry Enterprise, dev boxes usually
# Community; vswhere finds whichever is installed. Not fatal if absent: cargo's
# `cc` crate locates MSVC on its own for plain Rust crates.
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
if (Test-Path $vswhere) {
    $vsPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    $devShell = "$vsPath\Common7\Tools\Launch-VsDevShell.ps1"
    if ($vsPath -and (Test-Path $devShell)) {
        $vsArch = if ($Architecture -eq "aarch64") { "arm64" } else { "amd64" }
        Push-Location
        & $devShell -Arch $vsArch -HostArch amd64 -SkipAutomaticLocation
        Pop-Location
        Write-Host "Using Visual Studio at $vsPath"
    }
}

rustup target add $target

# RELEASE_CHANNEL is read by build.rs (icon, identifiers) and release_channel.
$env:RELEASE_CHANNEL = $channel
$env:PADDLEBOARD_RELEASE_CHANNEL = $channel

# PaddleBoard: a hosted windows-2022 runner has 16 GB of RAM and a personal
# account gets nothing bigger. The first release-profile probe (2026-10-05) died
# in wasmtime-wasi with "rustc-LLVM ERROR: out of memory" at four parallel jobs
# under the workspace profile (thin LTO, codegen-units = 1, limited debuginfo).
# This is an unsigned preview, so trade a little binary speed and size for a
# build that finishes: fewer parallel rustc processes, more codegen units per
# crate (smaller LLVM modules, lower peak memory), and no debuginfo (nothing
# consumes Windows symbols yet). Each knob yields to an explicit environment
# value so a bigger machine can override it.
if (-not $env:PADDLEBOARD_WINDOWS_BUILD_JOBS) { $env:PADDLEBOARD_WINDOWS_BUILD_JOBS = "3" }
$env:CARGO_BUILD_JOBS = $env:PADDLEBOARD_WINDOWS_BUILD_JOBS
if (-not $env:CARGO_PROFILE_RELEASE_CODEGEN_UNITS) { $env:CARGO_PROFILE_RELEASE_CODEGEN_UNITS = "16" }
if (-not $env:CARGO_PROFILE_RELEASE_DEBUG) { $env:CARGO_PROFILE_RELEASE_DEBUG = "0" }
Write-Host "build jobs=$env:CARGO_BUILD_JOBS codegen-units=$env:CARGO_PROFILE_RELEASE_CODEGEN_UNITS debug=$env:CARGO_PROFILE_RELEASE_DEBUG"

Write-Host "=== cargo build --release (paddleboard, cli) for $target ==="
$buildStarted = Get-Date
cargo build --release --locked --package paddleboard --package cli --target $target
if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)" }
Write-Host ("build took {0:N0} minutes" -f ((Get-Date) - $buildStarted).TotalMinutes)

Write-Host "=== laying out $stageDir ==="
if (Test-Path $stageDir) { Remove-Item -Recurse -Force $stageDir }
New-Item -ItemType Directory -Force -Path "$stageDir\bin", "$stageDir\x64", "$stageDir\arm64" | Out-Null
Copy-Item "$cargoOutDir\paddleboard.exe" "$stageDir\PaddleBoard.exe"
Copy-Item "$cargoOutDir\cli.exe" "$stageDir\bin\paddleboard.exe"
Copy-Item "$workspace\LICENSE-GPL" "$stageDir\LICENSE-GPL.txt"
Copy-Item "$workspace\LICENSE-APACHE" "$stageDir\LICENSE-APACHE.txt" -ErrorAction SilentlyContinue

# ConPTY: the same nupkg the inherited script downloads. The terminal uses
# conpty.dll from the exe's directory when present, and the OpenConsole host
# beside it; without them it falls back to the OS ConPTY, which is older.
$downloads = "$targetDir\windows-preview-downloads"
New-Item -ItemType Directory -Force -Path $downloads | Out-Null
$conptyPkg = "$downloads\conpty.nupkg"
if (-not (Test-Path $conptyPkg)) {
    Invoke-WebRequest -Uri "https://github.com/microsoft/terminal/releases/download/v1.25.1912.0/Microsoft.Windows.Console.ConPTY.1.25.260710002-preview.nupkg" -OutFile $conptyPkg
}
Expand-Archive -Path $conptyPkg -DestinationPath "$downloads\conpty" -Force
if ($Architecture -eq "aarch64") {
    Copy-Item "$downloads\conpty\runtimes\win-arm64\native\conpty.dll" "$stageDir\conpty.dll"
    Copy-Item "$downloads\conpty\build\native\runtimes\arm64\OpenConsole.exe" "$stageDir\arm64\OpenConsole.exe"
} else {
    Copy-Item "$downloads\conpty\runtimes\win-x64\native\conpty.dll" "$stageDir\conpty.dll"
    Copy-Item "$downloads\conpty\build\native\runtimes\x64\OpenConsole.exe" "$stageDir\x64\OpenConsole.exe"
    Copy-Item "$downloads\conpty\build\native\runtimes\arm64\OpenConsole.exe" "$stageDir\arm64\OpenConsole.exe"

    # AMD GPU Services, loaded by the DirectX renderer when present (x64 only).
    # Version must match the one named in gpui_windows/src/directx_renderer.rs.
    $agsZip = "$downloads\AGS_SDK_v6.3.0.zip"
    if (-not (Test-Path $agsZip)) {
        Invoke-WebRequest -Uri "https://codeload.github.com/GPUOpen-LibrariesAndSDKs/AGS_SDK/zip/refs/tags/v6.3.0" -OutFile $agsZip
    }
    Expand-Archive -Path $agsZip -DestinationPath $downloads -Force
    Copy-Item "$downloads\AGS_SDK-6.3.0\ags_lib\lib\amd_ags_x64.dll" "$stageDir\amd_ags_x64.dll"
}

@"
PaddleBoard $version for Windows ($Architecture) - UNSIGNED PREVIEW

This is a preview build, not a release-grade one. Known limits:

  * It is not code-signed. The first run shows a SmartScreen warning:
    click "More info", then "Run anyway". You can verify the download
    against the SHA-256 printed in the release notes.
  * There is no installer and no automatic update. Unzip anywhere, run
    PaddleBoard.exe. To update, download the next zip.
  * Managed Local Models are not available on Windows. Bring your own
    API key, or point PaddleBoard at any OpenAI-compatible endpoint.
  * bin\paddleboard.exe is the command-line launcher; add bin\ to PATH
    to use `paddleboard <path>` from a terminal.

Known issues and the full status: https://paddleboard.dev/beta
Report problems: https://github.com/paddleboarddev/paddleboard/issues
"@ | Out-File -FilePath "$stageDir\README-PREVIEW.txt" -Encoding utf8

Write-Host "=== zipping ==="
if (Test-Path $zipPath) { Remove-Item -Force $zipPath }
Compress-Archive -Path "$stageDir\*" -DestinationPath $zipPath -CompressionLevel Optimal
$hash = (Get-FileHash -Algorithm SHA256 $zipPath).Hash.ToLower()
$sizeMb = [math]::Round((Get-Item $zipPath).Length / 1MB, 1)
Write-Host "Bundle: $zipPath ($sizeMb MB)"
Write-Host "SHA-256: $hash"
"$hash  $bundleName.zip" | Out-File -FilePath "$zipPath.sha256" -Encoding ascii
