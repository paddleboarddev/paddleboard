// PaddleBoard: the first-launch tour, consolidated. Materialization, the
// rendered-preview open, and content-hash re-surfacing all live here — replacing
// the two duplicated blocks that previously sat in main.rs (startup) and
// workspace.rs (the manual-open action). This module lives in the `paddleboard`
// crate because opening a markdown *preview* needs `markdown_preview`, which
// depends on `workspace` — so `workspace` itself can't reach it without a cycle.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{Action, App, Context, TaskExt, Window};
use markdown_preview::markdown_preview_view::MarkdownPreviewView;
use project::ProjectPath;
use util::ResultExt;
use workspace::{OpenPaddleBoardTour, Toast, Workspace, notifications::NotificationId};

const EMBEDDED_TOUR: &str = include_str!("../../workspace/src/tour.md");

// `observe_new` fires for every workspace window; this ensures the first-launch
// check runs once per process, not once per window.
static STARTUP_HANDLED: AtomicBool = AtomicBool::new(false);

fn tour_path() -> PathBuf {
    paths::config_dir().join("PaddleBoard_Tour.md")
}

fn marker_path() -> PathBuf {
    paths::config_dir().join(".tour_seen")
}

// FNV-1a over the embedded tour. Deterministic across builds and toolchains, so
// a marker written by any release stays comparable — that's what lets us tell
// the shipped tour has changed since the user last saw it.
fn tour_fingerprint() -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in EMBEDDED_TOUR.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Writes the embedded tour to disk when the on-disk copy is missing or stale,
/// so existing users pick up tour updates after upgrading.
fn materialize_tour() {
    let path = tour_path();
    let needs_write = std::fs::read_to_string(&path)
        .map(|existing| existing != EMBEDDED_TOUR)
        .unwrap_or(true);
    if needs_write {
        std::fs::write(&path, EMBEDDED_TOUR).log_err();
    }
}

pub fn init(cx: &mut App) {
    cx.observe_new(|workspace: &mut Workspace, window, cx| {
        // Only wire up real, windowed workspaces (mirrors markdown_preview::init).
        let Some(_window) = window else {
            return;
        };

        workspace.register_action(|workspace, _: &OpenPaddleBoardTour, window, cx| {
            open_tour(workspace, window, cx);
        });

        if !STARTUP_HANDLED.swap(true, Ordering::SeqCst) {
            handle_startup(workspace, cx);
        }
    })
    .detach();
}

// Runs once per launch on the first workspace with a window. The tour is no
// longer force-opened on first launch (the Welcome page's tour card is the entry
// point now); instead we quietly record the baseline and, on later launches,
// surface a toast when the shipped tour has gained new content.
fn handle_startup(workspace: &mut Workspace, cx: &mut Context<Workspace>) {
    materialize_tour();

    let fingerprint = tour_fingerprint();
    let previous = std::fs::read_to_string(marker_path())
        .ok()
        .map(|contents| contents.trim().to_string());

    match previous {
        // First launch: record the baseline so future changes can be detected.
        None => {
            std::fs::write(marker_path(), &fingerprint).log_err();
        }
        // The shipped tour changed since the user last saw it: surface a toast
        // instead of silently rewriting the on-disk copy.
        Some(previous) if previous != fingerprint => {
            std::fs::write(marker_path(), &fingerprint).log_err();
            let toast = Toast::new(
                NotificationId::named("paddleboard-tour-updated".into()),
                "The PaddleBoard tour has new sections.",
            )
            .on_click("Open Tour", |window, cx| {
                window.dispatch_action(OpenPaddleBoardTour.boxed_clone(), cx);
            })
            .autohide();
            workspace.show_toast(toast, cx);
        }
        // Unchanged — nothing to do.
        Some(_) => {}
    }
}

/// Materializes the tour and opens it as a rendered markdown preview (not a raw
/// editable buffer). The tour lives outside any project worktree, so we register
/// it as an invisible single-file worktree first — the same thing Zed does when
/// you open any external file.
pub fn open_tour(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    materialize_tour();
    std::fs::write(marker_path(), tour_fingerprint()).log_err();

    let abs_path = tour_path();
    let create_worktree = workspace.project().update(cx, |project, cx| {
        project.find_or_create_worktree(abs_path, false, cx)
    });

    cx.spawn_in(window, async move |workspace, cx| {
        let (worktree, relative_path) = create_worktree.await?;
        workspace.update_in(cx, |workspace, window, cx| {
            let worktree_id = worktree.read(cx).id();
            let project_path = ProjectPath {
                worktree_id,
                path: relative_path,
            };
            MarkdownPreviewView::open_for_project_path(project_path, workspace, window, cx);
        })?;
        anyhow::Ok(())
    })
    .detach_and_log_err(cx);
}
