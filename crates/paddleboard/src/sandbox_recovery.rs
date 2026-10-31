// PaddleBoard: the startup half of sandbox crash recovery (the mechanism lives
// in `paddleboard_sandbox_recovery`). Leftover containers and microVM scratch
// directories are removed in the background; commands a crash cut off are
// reported once, on the first window, and never re-run.

use std::cell::Cell;

use gpui::App;
use paddleboard_container_engine::image_store::ImageStore;
use paddleboard_sandbox_recovery::InterruptedCommand;
use workspace::{Toast, Workspace, notifications::NotificationId};

const COMMAND_PREVIEW_CHARS: usize = 80;

/// Must run after Podman's install dir is on PATH, or every launch from the
/// Dock would skip container cleanup.
pub async fn remove_orphans() {
    match paddleboard_sandbox_recovery::reap_orphaned_containers().await {
        Ok(_) => {}
        // Podman being absent or its machine stopped is the common case on
        // machines that use the built-in tier, and its containers can't be
        // running then anyway.
        Err(error) => log::debug!("skipped sandbox container cleanup: {error:#}"),
    }
    ImageStore::new(ImageStore::global_root()).remove_stale_ephemerals();
}

pub fn init(cx: &mut App) {
    let interrupted = paddleboard_sandbox_recovery::take_interrupted_commands();
    if interrupted.is_empty() {
        return;
    }
    let pending = Cell::new(Some(interrupted));
    cx.observe_new(move |workspace: &mut Workspace, window, cx| {
        if window.is_none() {
            return;
        }
        let Some(interrupted) = pending.take() else {
            return;
        };
        let toast = Toast::new(
            NotificationId::named("paddleboard-sandbox-interrupted".into()),
            interrupted_message(&interrupted),
        );
        workspace.show_toast(toast, cx);
    })
    .detach();
}

fn interrupted_message(interrupted: &[InterruptedCommand]) -> String {
    let Some(latest) = interrupted.last() else {
        return String::new();
    };
    let mut command: String = latest.command.chars().take(COMMAND_PREVIEW_CHARS).collect();
    if latest.command.chars().count() > COMMAND_PREVIEW_CHARS {
        command.push('…');
    }
    let directory = latest.working_directory.display();
    let what = if interrupted.len() == 1 {
        format!(
            "PaddleBoard quit while a sandboxed command was running: `{command}` in {directory}."
        )
    } else {
        format!(
            "PaddleBoard quit while {} sandboxed commands were running, most recently `{command}` in {directory}.",
            interrupted.len()
        )
    };
    format!(
        "{what} Files there may be partly written, so check `git status` before you carry on. Nothing was run again."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn interrupted(command: &str) -> InterruptedCommand {
        InterruptedCommand {
            command: command.to_string(),
            working_directory: PathBuf::from("/work/app"),
            started_at_unix_seconds: 0,
            owner_pid: 1,
        }
    }

    #[test]
    fn message_names_the_command_and_directory() {
        assert_eq!(
            interrupted_message(&[interrupted("cargo test")]),
            "PaddleBoard quit while a sandboxed command was running: `cargo test` in /work/app. \
             Files there may be partly written, so check `git status` before you carry on. \
             Nothing was run again."
        );
    }

    #[test]
    fn message_counts_several_and_shortens_long_commands() {
        let long = "x".repeat(COMMAND_PREVIEW_CHARS + 5);
        let message = interrupted_message(&[interrupted("make"), interrupted(&long)]);
        assert!(message.starts_with("PaddleBoard quit while 2 sandboxed commands were running"));
        assert!(message.contains(&format!("`{}…`", "x".repeat(COMMAND_PREVIEW_CHARS))));
    }
}
