//! Recovery after a sandboxed agent command is killed mid-run, most often
//! because PaddleBoard itself crashed or was force-quit.
//!
//! Two mechanisms, both keyed on the PID of the PaddleBoard process that
//! started the work:
//!
//! * Every Podman container PaddleBoard starts carries [`OWNER_LABEL`]. At
//!   startup, [`reap_orphaned_containers`] force-removes labeled containers
//!   whose owner is no longer running, so no detached service keeps holding a
//!   host port and no one-shot command keeps writing to the workspace.
//! * Before a sandboxed command starts, [`begin_command`] writes a small
//!   record to disk, and dropping the returned [`InFlightCommand`] removes it
//!   once the tool call is over. A record whose owner is gone means the
//!   process died mid-command, and
//!   [`take_interrupted_commands`] hands it to the UI on the next launch so the
//!   user learns the workspace may be partially modified. Nothing is ever
//!   re-run.
//!
//! Owner liveness is a PID check, so a reused PID makes an orphan look alive.
//! That errs toward leaving things alone: the container or record is picked
//! up on a later launch instead of being removed from under a live process.

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MANAGED_LABEL: &str = "paddleboard.managed";
pub const OWNER_LABEL: &str = "paddleboard.owner";
pub const KIND_LABEL: &str = "paddleboard.kind";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerKind {
    OneShot,
    Service,
    McpServer,
    Kernel,
}

impl ContainerKind {
    fn label_value(self) -> &'static str {
        match self {
            ContainerKind::OneShot => "oneshot",
            ContainerKind::Service => "service",
            ContainerKind::McpServer => "mcp",
            ContainerKind::Kernel => "kernel",
        }
    }
}

/// `--label` arguments for `podman run` that tie the container to this
/// PaddleBoard process.
pub fn podman_label_args(kind: ContainerKind) -> Vec<String> {
    vec![
        "--label".to_string(),
        format!("{MANAGED_LABEL}=true"),
        "--label".to_string(),
        format!("{OWNER_LABEL}={}", std::process::id()),
        "--label".to_string(),
        format!("{KIND_LABEL}={}", kind.label_value()),
    ]
}

/// True when `pid` names a running process. Without a way to check, every
/// owner is treated as alive so nothing is removed.
pub fn process_is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return true;
        };
        // Signal 0 performs the existence and permission checks without
        // delivering anything. EPERM means the process exists but belongs to
        // someone else, which still counts as alive.
        let result = unsafe { libc::kill(pid, 0) };
        result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

fn owner_is_gone(pid: u32) -> bool {
    pid != std::process::id() && !process_is_alive(pid)
}

/// Force-removes PaddleBoard containers whose owning process has exited.
/// Returns how many were removed. Fails quietly enough to call at every
/// startup: an absent or stopped Podman is an error the caller can log.
pub async fn reap_orphaned_containers() -> Result<usize> {
    let output = util::command::new_command("podman")
        .args([
            "ps",
            "--all",
            "--filter",
            &format!("label={MANAGED_LABEL}=true"),
            "--format",
            "json",
        ])
        .output()
        .await
        .context("failed to run podman ps")?;
    if !output.status.success() {
        anyhow::bail!(
            "podman ps failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let orphans = orphaned_container_ids(&output.stdout, owner_is_gone)?;
    if orphans.is_empty() {
        return Ok(0);
    }

    let output = util::command::new_command("podman")
        .args(["rm", "--force", "--time", "0"])
        .args(&orphans)
        .output()
        .await
        .context("failed to run podman rm")?;
    if !output.status.success() {
        anyhow::bail!(
            "podman rm failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    log::info!(
        "removed {} sandbox container(s) left behind by an earlier PaddleBoard session",
        orphans.len()
    );
    Ok(orphans.len())
}

#[derive(Deserialize)]
struct ListedContainer {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Labels", default)]
    labels: Option<std::collections::HashMap<String, String>>,
}

/// Parses `podman ps --format json`. A container whose owner label is missing
/// or not a PID is left alone.
fn orphaned_container_ids(
    ps_json: &[u8],
    owner_is_gone: impl Fn(u32) -> bool,
) -> Result<Vec<String>> {
    let containers: Vec<ListedContainer> =
        serde_json::from_slice(ps_json).context("failed to parse podman ps output")?;
    Ok(containers
        .into_iter()
        .filter(|container| {
            container
                .labels
                .as_ref()
                .and_then(|labels| labels.get(OWNER_LABEL))
                .and_then(|owner| owner.parse::<u32>().ok())
                .is_some_and(&owner_is_gone)
        })
        .map(|container| container.id)
        .collect())
}

/// A sandboxed command that was running when its PaddleBoard process exited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterruptedCommand {
    pub command: String,
    pub working_directory: PathBuf,
    pub started_at_unix_seconds: u64,
    pub owner_pid: u32,
}

/// The on-disk record for a command that is currently running. Dropping it
/// removes the record, so only a process that dies without unwinding (a
/// crash, a force-quit, a kill) leaves one behind.
pub struct InFlightCommand {
    path: PathBuf,
}

impl Drop for InFlightCommand {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_file(&self.path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            log::warn!(
                "failed to remove sandbox command record {:?}: {error}",
                self.path
            );
        }
    }
}

fn journal_dir() -> PathBuf {
    paths::data_dir().join("sandbox").join("in-flight")
}

static RECORD_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn begin_command(command: &str, working_directory: &Path) -> Result<InFlightCommand> {
    begin_command_in(&journal_dir(), command, working_directory)
}

fn begin_command_in(
    dir: &Path,
    command: &str,
    working_directory: &Path,
) -> Result<InFlightCommand> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("failed to create sandbox command journal {dir:?}"))?;
    let started_at_unix_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let record = InterruptedCommand {
        command: command.to_string(),
        working_directory: working_directory.to_path_buf(),
        started_at_unix_seconds,
        owner_pid: std::process::id(),
    };
    let path = dir.join(format!(
        "{}-{}-{}.json",
        record.owner_pid,
        RECORD_COUNTER.fetch_add(1, Ordering::Relaxed),
        started_at_unix_seconds
    ));
    std::fs::write(&path, serde_json::to_vec(&record)?)
        .with_context(|| format!("failed to write sandbox command record {path:?}"))?;
    Ok(InFlightCommand { path })
}

/// Returns the commands left running by PaddleBoard processes that have
/// exited, oldest first, and removes their records so each is reported once.
/// Records owned by a live process (another open PaddleBoard window, or this
/// one) are left in place.
pub fn take_interrupted_commands() -> Vec<InterruptedCommand> {
    take_interrupted_commands_in(&journal_dir(), owner_is_gone)
}

fn take_interrupted_commands_in(
    dir: &Path,
    owner_is_gone: impl Fn(u32) -> bool,
) -> Vec<InterruptedCommand> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut interrupted = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let record = std::fs::read(&path)
            .ok()
            .and_then(|contents| serde_json::from_slice::<InterruptedCommand>(&contents).ok());
        match record {
            Some(record) if !owner_is_gone(record.owner_pid) => continue,
            Some(record) => interrupted.push(record),
            None => log::warn!("discarding unreadable sandbox command record {path:?}"),
        }
        if let Err(error) = std::fs::remove_file(&path) {
            log::warn!("failed to remove sandbox command record {path:?}: {error}");
        }
    }
    interrupted.sort_by_key(|record| record.started_at_unix_seconds);
    interrupted
}

/// Whether a `run-<pid>-…` scratch directory belongs to a process that has
/// exited. Names that don't carry a PID are never considered orphaned.
pub fn run_dir_is_orphaned(name: &str) -> bool {
    name.strip_prefix("run-")
        .and_then(|rest| rest.split('-').next())
        .and_then(|pid| pid.parse::<u32>().ok())
        .is_some_and(owner_is_gone)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_args_name_this_process_as_owner() {
        assert_eq!(
            podman_label_args(ContainerKind::Service),
            vec![
                "--label".to_string(),
                "paddleboard.managed=true".to_string(),
                "--label".to_string(),
                format!("paddleboard.owner={}", std::process::id()),
                "--label".to_string(),
                "paddleboard.kind=service".to_string(),
            ]
        );
    }

    #[test]
    fn only_containers_with_a_dead_owner_are_orphaned() {
        let output = br#"[
            {"Id": "aaa111", "Labels": {"paddleboard.managed": "true", "paddleboard.owner": "100"}},
            {"Id": "bbb222", "Labels": {"paddleboard.managed": "true", "paddleboard.owner": "200"}},
            {"Id": "ccc333", "Labels": {"paddleboard.managed": "true"}},
            {"Id": "ddd444", "Labels": {"paddleboard.owner": "not-a-pid"}},
            {"Id": "eee555", "Labels": null}
        ]"#;
        let orphans = orphaned_container_ids(output, |pid| pid == 200).expect("parse");
        assert_eq!(orphans, vec!["bbb222".to_string()]);
    }

    #[test]
    fn this_process_is_alive() {
        assert!(process_is_alive(std::process::id()));
        assert!(!owner_is_gone(std::process::id()));
    }

    #[test]
    fn finished_commands_leave_no_record() {
        let dir = tempfile::tempdir().expect("tempdir");
        let in_flight =
            begin_command_in(dir.path(), "cargo test", Path::new("/work")).expect("begin");
        drop(in_flight);
        assert!(take_interrupted_commands_in(dir.path(), |_| true).is_empty());
    }

    #[test]
    fn unfinished_commands_are_reported_once_after_their_owner_exits() {
        let dir = tempfile::tempdir().expect("tempdir");
        // A crash never runs the destructor.
        std::mem::forget(
            begin_command_in(dir.path(), "echo partial > out.txt", Path::new("/work"))
                .expect("begin"),
        );

        // While the owner is alive, the record is someone else's live command.
        assert!(take_interrupted_commands_in(dir.path(), |_| false).is_empty());

        let interrupted = take_interrupted_commands_in(dir.path(), |_| true);
        assert_eq!(interrupted.len(), 1);
        assert_eq!(interrupted[0].command, "echo partial > out.txt");
        assert_eq!(interrupted[0].working_directory, Path::new("/work"));
        assert_eq!(interrupted[0].owner_pid, std::process::id());

        assert!(take_interrupted_commands_in(dir.path(), |_| true).is_empty());
    }

    #[test]
    fn unreadable_records_are_discarded() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("garbage.json"), "{").expect("write");
        assert!(take_interrupted_commands_in(dir.path(), |_| true).is_empty());
        assert!(!dir.path().join("garbage.json").exists());
    }

    #[test]
    fn run_dirs_of_this_process_are_not_orphaned() {
        assert!(!run_dir_is_orphaned(&format!(
            "run-{}-0-123",
            std::process::id()
        )));
        assert!(!run_dir_is_orphaned("run-notapid-0-123"));
        assert!(!run_dir_is_orphaned("something-else"));
    }

    /// End-to-end check against a real Podman: a labeled container whose owner
    /// PID is dead is removed, and one owned by this process survives.
    ///
    /// ```sh
    /// cargo test -p paddleboard_sandbox_recovery -- --ignored live_reaper
    /// ```
    #[test]
    #[ignore = "requires a running podman"]
    #[allow(clippy::disallowed_methods)]
    fn live_reaper_removes_only_dead_owners_containers() {
        // PIDs are capped well below this on macOS and Linux, so it is never
        // a running process.
        let dead_owner = 999_999_u32;
        let run = |owner: u32| {
            let output = std::process::Command::new("podman")
                .args([
                    "run",
                    "-d",
                    "--rm",
                    "--label",
                    &format!("{MANAGED_LABEL}=true"),
                    "--label",
                    &format!("{OWNER_LABEL}={owner}"),
                    "--pull=never",
                    "docker.io/library/alpine:3.22",
                    "sleep",
                    "300",
                ])
                .output()
                .expect("podman run");
            assert!(output.status.success(), "{output:?}");
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        };
        let orphan = run(dead_owner);
        let live = run(std::process::id());

        let removed = smol::block_on(reap_orphaned_containers()).expect("reap");
        assert!(removed >= 1);

        let exists = |id: &str| {
            std::process::Command::new("podman")
                .args(["container", "exists", id])
                .status()
                .expect("podman container exists")
                .success()
        };
        assert!(!exists(&orphan), "orphaned container survived");
        assert!(exists(&live), "live owner's container was removed");
        std::process::Command::new("podman")
            .args(["rm", "--force", "--time", "0", &live])
            .output()
            .expect("cleanup");
    }
}
