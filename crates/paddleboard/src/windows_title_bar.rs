//! PaddleBoard: a bare title bar for the main window on Windows.
//!
//! PaddleBoard drops Zed's `title_bar` crate (`title_bar::init` runs only in the
//! visual test runner), so `workspace.titlebar_item` is `None` and the window has
//! no title bar of its own. On macOS that is fine: AppKit still moves the window
//! by its title area (see `app_owns_titlebar_drag` in `zed.rs`) and draws the
//! traffic lights. Windows has no such fallback. The window hides the native
//! caption, and Windows asks the app where the caption is through `WM_NCHITTEST`,
//! which answers `HTCAPTION` only for a `WindowControlArea::Drag` region. With no
//! title bar there is none, so the window can't be dragged, and nothing draws the
//! minimize, maximize and close buttons either (paddleboarddev/paddleboard#87).
//!
//! This puts upstream's `PlatformTitleBar` back on Windows only. It already does
//! everything the caption needs there: a drag region (with double-click to
//! maximize, handled by Windows), and the caption buttons, which defer to the
//! threads sidebar when it is open on the right.

use gpui::{App, AppContext as _, Context, Entity, Render, WeakEntity, Window};
use platform_title_bar::PlatformTitleBar;
use project::Project;
use ui::prelude::*;
use workspace::Workspace;

pub fn init(cx: &mut App) {
    if !cfg!(target_os = "windows") {
        return;
    }

    cx.observe_new(|workspace: &mut Workspace, window, cx| {
        let Some(window) = window else {
            return;
        };
        let item = cx.new(|cx| WindowsTitleBar::new(workspace, cx));
        workspace.set_titlebar_item(item.into(), window, cx);
    })
    .detach();
}

struct WindowsTitleBar {
    platform_titlebar: Entity<PlatformTitleBar>,
    workspace: WeakEntity<Workspace>,
    project: Entity<Project>,
    has_multi_workspace: bool,
}

impl WindowsTitleBar {
    fn new(workspace: &Workspace, cx: &mut Context<Self>) -> Self {
        let multi_workspace = workspace.multi_workspace().cloned();
        let has_multi_workspace = multi_workspace.is_some();
        let platform_titlebar = cx.new(|cx| {
            let mut titlebar = PlatformTitleBar::new("paddleboard-title-bar", cx);
            if let Some(multi_workspace) = multi_workspace {
                titlebar = titlebar.with_multi_workspace(multi_workspace);
            }
            titlebar
        });
        let project = workspace.project().clone();
        cx.observe(&project, |_, _, cx| cx.notify()).detach();

        Self {
            platform_titlebar,
            workspace: workspace.weak_handle(),
            project,
            has_multi_workspace,
        }
    }

    fn title(&self, cx: &App) -> SharedString {
        let names = self
            .project
            .read(cx)
            .visible_worktrees(cx)
            .map(|worktree| worktree.read(cx).root_name_str().to_string())
            .collect::<Vec<_>>();
        if names.is_empty() {
            "PaddleBoard".into()
        } else {
            names.join(", ").into()
        }
    }
}

impl Render for WindowsTitleBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The workspace may join its multi-workspace after this view is created, and
        // the title bar needs it to know which side the threads sidebar is on.
        if !self.has_multi_workspace
            && let Some(multi_workspace) = self
                .workspace
                .upgrade()
                .and_then(|workspace| workspace.read(cx).multi_workspace().cloned())
        {
            self.has_multi_workspace = true;
            self.platform_titlebar.update(cx, |titlebar, _| {
                titlebar.set_multi_workspace(multi_workspace);
            });
        }

        // A label has no hitbox, so the whole row stays a drag region.
        let title = Label::new(self.title(cx))
            .size(LabelSize::Small)
            .color(Color::Muted)
            .single_line()
            .truncate()
            .into_any_element();
        self.platform_titlebar.update(cx, |titlebar, _| {
            titlebar.set_children([title]);
        });

        self.platform_titlebar.clone()
    }
}
