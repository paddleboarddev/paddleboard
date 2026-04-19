use anyhow::Result;
use gpui::{
    Action, App, AsyncWindowContext, Bounds, Context, Element, ElementId, Entity, EventEmitter,
    FocusHandle, Focusable, GlobalElementId, IntoElement, LayoutId, Pixels, Render, WeakEntity,
    Window, prelude::*, px,
};
use std::panic::Location;
use std::time::Duration;
use ui::IconName;
use util::{ResultExt, command::new_command};
use workspace::{
    Workspace,
    dock::{DockPosition, Panel, PanelEvent},
};

gpui::actions!(browser, [ToggleFocus]);

const BROWSER_PANEL_KEY: &str = "BrowserPanel";
const DEFAULT_URL: &str = "about:blank";
const UNSLOTH_IMAGE: &str = "unslothai/unsloth-studio";

pub struct Browser {
    focus_handle: FocusHandle,
    url: String,
    position: DockPosition,
    webview_initialized: bool,
}

impl Browser {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            url: DEFAULT_URL.to_string(),
            position: DockPosition::Left,
            webview_initialized: false,
        }
    }

    pub async fn load(
        _workspace: WeakEntity<Workspace>,
        mut cx: AsyncWindowContext,
    ) -> Result<Entity<Self>> {
        Ok(cx.new(|cx| Self::new(cx)))
    }
}

impl EventEmitter<PanelEvent> for Browser {}

impl Focusable for Browser {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for Browser {
    fn persistent_name() -> &'static str {
        "BrowserPanel"
    }

    fn panel_key() -> &'static str {
        BROWSER_PANEL_KEY
    }

    fn position(&self, _window: &Window, _cx: &App) -> DockPosition {
        self.position
    }

    fn position_is_valid(&self, position: DockPosition) -> bool {
        matches!(position, DockPosition::Left | DockPosition::Right)
    }

    fn set_position(
        &mut self,
        position: DockPosition,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.position = position;
        cx.notify();
    }

    fn default_size(&self, _window: &Window, _cx: &App) -> Pixels {
        px(400.0)
    }

    fn icon(&self, _window: &Window, _cx: &App) -> Option<IconName> {
        Some(IconName::ToolWeb)
    }

    fn icon_tooltip(&self, _window: &Window, _cx: &App) -> Option<&'static str> {
        Some("Browser")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(ToggleFocus)
    }

    fn activation_priority(&self) -> u32 {
        10
    }

    fn set_active(&mut self, active: bool, window: &mut Window, _cx: &mut Context<Self>) {
        if active {
            if !self.webview_initialized {
                window.add_webview(&self.url, Bounds::default());
                self.webview_initialized = true;
            }
        } else {
            window.hide_webview();
        }
    }
}

impl Render for Browser {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        BrowserElement
    }
}

struct BrowserElement;

impl IntoElement for BrowserElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for BrowserElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = gpui::Style::default();
        style.size.width = gpui::Length::Definite(gpui::DefiniteLength::Fraction(1.0));
        style.size.height = gpui::Length::Definite(gpui::DefiniteLength::Fraction(1.0));
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
        window.update_webview(bounds);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        _window: &mut Window,
        _cx: &mut App,
    ) {
    }
}

pub fn init(cx: &mut App) {
    cx.observe_new(
        |workspace: &mut Workspace,
         _window: Option<&mut Window>,
         _cx: &mut Context<Workspace>| {
            workspace.register_action(|workspace, _: &ToggleFocus, window, cx| {
                workspace.toggle_panel_focus::<Browser>(window, cx);
            });
            workspace.register_action(
                |_workspace, _: &workspace::OpenUnsloth, window, cx| {
                    cx.spawn_in(window, async move |workspace_handle, mut cx| {
                        start_unsloth(workspace_handle, &mut cx).await
                    })
                    .detach_and_log_err(cx);
                },
            );
        },
    )
    .detach();
}

async fn start_unsloth(
    workspace_handle: gpui::WeakEntity<Workspace>,
    cx: &mut gpui::AsyncWindowContext,
) -> Result<()> {
    let container_id = cx
        .background_spawn(async {
            let mut cmd = new_command("podman");
            cmd.args(["run", "-d", "-p", "8888:8888", UNSLOTH_IMAGE]);
            let output = cmd
                .output()
                .await
                .map_err(|error| anyhow::anyhow!("Failed to launch podman: {error}"))?;
            if !output.status.success() {
                anyhow::bail!(
                    "podman run failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            anyhow::Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        })
        .await?;

    let url = poll_for_jupyter_url(cx, &container_id).await;

    let url = match url {
        Some(url) => url,
        None => {
            let mut cmd = new_command("podman");
            cmd.args(["stop", &container_id]);
            cmd.spawn().log_err();
            anyhow::bail!("Timed out waiting for Jupyter server to start");
        }
    };

    workspace_handle
        .update_in(cx, |workspace, window, cx| {
            if let Some(browser_panel) = workspace.panel::<Browser>(cx) {
                browser_panel.update(cx, |browser, cx| {
                    browser.url = url.clone();
                    cx.notify();
                });
                if browser_panel.read(cx).webview_initialized {
                    window.navigate_webview(&url);
                }
                workspace.open_panel::<Browser>(window, cx);
            }
        })
        .map_err(|error| anyhow::anyhow!("Failed to open browser panel: {error}"))
}

async fn poll_for_jupyter_url(
    cx: &mut gpui::AsyncWindowContext,
    container_id: &str,
) -> Option<String> {
    for _ in 0..60u32 {
        cx.background_executor()
            .timer(Duration::from_secs(1))
            .await;

        let container_id = container_id.to_string();
        let logs = cx
            .background_spawn(async move {
                let mut cmd = new_command("podman");
                cmd.args(["logs", &container_id]);
                let output = cmd.output().await?;
                let mut combined = String::from_utf8_lossy(&output.stdout).to_string();
                combined.push_str(&String::from_utf8_lossy(&output.stderr));
                anyhow::Ok(combined)
            })
            .await
            .log_err();

        if let Some(logs) = logs {
            if let Some(url) = extract_jupyter_url(&logs) {
                return Some(url);
            }
        }
    }
    None
}

fn extract_jupyter_url(logs: &str) -> Option<String> {
    for line in logs.lines() {
        if (line.contains("127.0.0.1:8888") || line.contains("localhost:8888"))
            && line.contains("token=")
        {
            if let Some(start) = line.find("http://") {
                let url = line[start..].split_whitespace().next().unwrap_or("").trim();
                if !url.is_empty() {
                    return Some(url.to_string());
                }
            }
        }
    }
    None
}
