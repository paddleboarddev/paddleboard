---
title: Update PaddleBoard
description: "PaddleBoard keeps itself up to date from GitHub Releases. You can change or turn off that behavior in your settings."
---

# Update PaddleBoard

PaddleBoard checks for new releases once an hour, downloads them in the background, and applies them when you restart. Updates come straight from [GitHub Releases](https://github.com/paddleboarddev/paddleboard/releases) — there is no update server, and no account or install identifier is sent with the check.

## What gets offered

PaddleBoard installs the **highest released version** that ships a build for your platform:

- macOS on Apple silicon
- Linux on x86_64

On any other platform — Intel Macs, ARM Linux, Windows — PaddleBoard is built from source and does not self-update. Checking for updates there tells you so rather than failing quietly.

Prerelease builds are skipped by default, and unpublished drafts are never offered.

## How to check your current version

Open the Command Palette (`cmd-shift-p` on macOS, `ctrl-shift-p` on Linux) and run {#action zed::About}. The version appears in the modal.

To check immediately rather than waiting for the hourly poll, run {#action auto_update::Check} from the same palette. A manual check runs even when automatic updates are turned off.

## How to control update behavior

Open your settings (`cmd-,`) and set:

```json
{
  "auto_update": false
}
```

That stops the hourly check. You can still update by hand with {#action auto_update::Check}, and toggling the setting takes effect immediately — no restart needed.

To ride beta builds as they are cut, opt into prereleases:

```json
{
  "paddleboard_auto_update": {
    "include_prereleases": true
  }
}
```

Leave this off unless you specifically want beta builds. PaddleBoard publishes each release as a prerelease and promotes it afterwards, so with this turned on you will generally be offered a build before it has been promoted.

## Development builds

A PaddleBoard you built yourself (`cargo run`, or any Dev-channel build) never polls for updates and will not replace itself with a release build.
