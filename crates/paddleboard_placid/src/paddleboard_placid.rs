// PaddleBoard: "Placid mode" — a per-window quiet layout for quick edits to a
// config file or a bit of YAML, the job that otherwise sends people to a
// separate lightweight editor.
//
// It hides the three docks and centers the editor. Both of those are already
// per-workspace state (`Workspace::left_dock` etc. and `centered_layout`), which
// is why this can be genuinely per-window with no changes to the settings system.
//
// The chrome that ISN'T touched here — tab bar, status bar, gutter, breadcrumbs —
// is global settings read through `Settings::get_global(cx)` at ~1100 call sites,
// so it cannot be scoped to one window without a settings-system refactor. That's
// a deliberate boundary, not an oversight: the reference point (Sublime Text)
// shows tabs, a status bar and a gutter too, so the docks are most of the gap.
//
// Leaving the status bar visible is also load-bearing: the Placid toggle lives
// there, so it stays reachable while Placid is on. A toggle that hides itself
// would strand the user.

use collections::HashMap;
use gpui::{Action, App, Context, Entity, EntityId, Global, IntoElement, Render, Window};
use ui::{IconButton, IconName, IconSize, Tooltip, prelude::*};
use workspace::Workspace;

/// What the layout looked like before Placid mode was entered, so exiting can
/// put it back. Deliberately NOT persisted: Placid is transient and clears on
/// restart (see `restore_on_close`).
#[derive(Copy, Clone, Debug)]
struct Snapshot {
    left_dock_open: bool,
    bottom_dock_open: bool,
    right_dock_open: bool,
    centered_layout: bool,
}

#[derive(Default)]
struct PlacidState {
    /// Keyed by workspace, so each window has its own Placid state.
    active: HashMap<EntityId, Snapshot>,
}

impl Global for PlacidState {}

pub fn init(cx: &mut App) {
    cx.set_global(PlacidState::default());
}

/// Whether the given workspace is currently in Placid mode.
pub fn is_placid(workspace: &Entity<Workspace>, cx: &App) -> bool {
    cx.try_global::<PlacidState>()
        .is_some_and(|state| state.active.contains_key(&workspace.entity_id()))
}

/// Turn Placid mode on or off for one workspace. Idempotent, so the CLI can call
/// it on a freshly-opened window without checking the current state first.
pub fn set_placid(
    workspace: &Entity<Workspace>,
    enabled: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let id = workspace.entity_id();
    let currently_on = is_placid(workspace, cx);
    if enabled == currently_on {
        return;
    }

    if enabled {
        let snapshot = workspace.read_with(cx, |workspace, cx| Snapshot {
            left_dock_open: workspace.left_dock().read(cx).is_open(),
            bottom_dock_open: workspace.bottom_dock().read(cx).is_open(),
            right_dock_open: workspace.right_dock().read(cx).is_open(),
            centered_layout: workspace.centered_layout,
        });

        workspace.update(cx, |workspace, cx| {
            set_docks(workspace, false, false, false, window, cx);
            set_centered_layout(workspace, true, cx);
        });

        cx.global_mut::<PlacidState>().active.insert(id, snapshot);
    } else {
        let Some(snapshot) = cx.global_mut::<PlacidState>().active.remove(&id) else {
            return;
        };
        workspace.update(cx, |workspace, cx| {
            set_docks(
                workspace,
                snapshot.left_dock_open,
                snapshot.bottom_dock_open,
                snapshot.right_dock_open,
                window,
                cx,
            );
            set_centered_layout(workspace, snapshot.centered_layout, cx);
        });
    }
}

pub fn toggle(workspace: &Entity<Workspace>, window: &mut Window, cx: &mut App) {
    let enabled = !is_placid(workspace, cx);
    set_placid(workspace, enabled, window, cx);
}

/// Drop a workspace's Placid state without touching its layout. Called when the
/// window goes away so the map doesn't grow for the life of the process.
pub fn forget(workspace_id: EntityId, cx: &mut App) {
    if let Some(state) = cx.try_global::<PlacidState>() {
        if state.active.contains_key(&workspace_id) {
            cx.global_mut::<PlacidState>().active.remove(&workspace_id);
        }
    }
}

fn set_docks(
    workspace: &mut Workspace,
    left: bool,
    bottom: bool,
    right: bool,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    workspace
        .left_dock()
        .update(cx, |dock, cx| dock.set_open(left, window, cx));
    workspace
        .bottom_dock()
        .update(cx, |dock, cx| dock.set_open(bottom, window, cx));
    workspace
        .right_dock()
        .update(cx, |dock, cx| dock.set_open(right, window, cx));
}

fn set_centered_layout(workspace: &mut Workspace, centered: bool, cx: &mut Context<Workspace>) {
    if workspace.centered_layout != centered {
        workspace.centered_layout = centered;
        cx.notify();
    }
}

/// Status bar toggle. Lives on the right, next to the other PaddleBoard items,
/// and is hideable through `paddleboard_ui.placid_status` like its siblings.
pub struct PlacidStatusItem {
    workspace: gpui::WeakEntity<Workspace>,
}

impl PlacidStatusItem {
    pub fn new(workspace: &Workspace, _cx: &mut Context<Self>) -> Self {
        Self {
            workspace: workspace.weak_handle(),
        }
    }
}

impl Render for PlacidStatusItem {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !paddleboard_ui::PaddleboardUiSettings::get(cx).placid_status {
            return gpui::Empty.into_any_element();
        }

        let active = self
            .workspace
            .upgrade()
            .is_some_and(|workspace| is_placid(&workspace, cx));

        IconButton::new("placid-status", IconName::Sailboat)
            .icon_size(IconSize::Small)
            .toggle_state(active)
            .tooltip(Tooltip::text(if active {
                "Placid mode on — click to restore the full layout"
            } else {
                "Placid mode: hide the docks and center the editor"
            }))
            .on_click(|_, window, cx| {
                window.dispatch_action(
                    paddleboard_actions::placid::TogglePlacidMode.boxed_clone(),
                    cx,
                );
            })
            .into_any_element()
    }
}

impl workspace::StatusItemView for PlacidStatusItem {
    fn set_active_pane_item(
        &mut self,
        _active_pane_item: Option<&dyn workspace::ItemHandle>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    fn hide_setting(&self, _cx: &App) -> Option<workspace::HideStatusItem> {
        Some(workspace::HideStatusItem::new(|settings| {
            settings
                .paddleboard_ui
                .get_or_insert_default()
                .placid_status = Some(false);
        }))
    }
}
