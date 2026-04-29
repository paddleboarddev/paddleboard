use anyhow::Result;
use editor::{Editor, EditorEvent};
use gpui::{
    Action, App, AsyncWindowContext, Bounds, Context, Element, ElementId, Entity, EventEmitter,
    FocusHandle, Focusable, GlobalElementId, IntoElement, LayoutId, Pixels, Render, WeakEntity,
    Window, prelude::*, px,
};
use std::panic::Location;
use std::time::Duration;
use theme::ActiveTheme;
use ui::{ButtonSize, ButtonStyle, prelude::*};
use util::{ResultExt, command::new_command};
use workspace::{
    Workspace,
    dock::{DockPosition, Panel, PanelEvent},
};

gpui::actions!(browser, [ToggleFocus]);

const BROWSER_PANEL_KEY: &str = "BrowserPanel";
const DEFAULT_URL: &str = "https://www.google.com";
const UNSLOTH_IMAGE: &str = "unslothai/unsloth-studio";

const BOOKMARKS: &[(&str, &str)] = &[
    ("Google", "https://www.google.com"),
    ("GitHub", "https://github.com"),
    ("Hacker News", "https://news.ycombinator.com"),
];

pub struct Browser {
    focus_handle: FocusHandle,
    url: String,
    address_bar: Entity<Editor>,
    position: DockPosition,
    webview_initialized: bool,
    weak_self: WeakEntity<Self>,
    _subscriptions: Vec<gpui::Subscription>,
}

impl Browser {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();

        let address_bar = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text("Search or enter URL", window, cx);
            editor.set_text(DEFAULT_URL, window, cx);
            editor
        });

        let subscriptions = vec![cx.subscribe(&address_bar, |_, _, event: &EditorEvent, cx| {
            if matches!(event, EditorEvent::Blurred) {
                cx.notify();
            }
        })];

        Self {
            focus_handle,
            url: DEFAULT_URL.to_string(),
            address_bar,
            position: DockPosition::Left,
            webview_initialized: false,
            weak_self: cx.weak_entity(),
            _subscriptions: subscriptions,
        }
    }

    pub async fn load(
        _workspace: WeakEntity<Workspace>,
        mut cx: AsyncWindowContext,
    ) -> Result<Entity<Self>> {
        cx.new_window_entity(|window, cx| Self::new(window, cx))
    }

    fn navigate_to_address_bar_content(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.address_bar.read(cx).text(cx);
        let url = normalize_url(input.as_str());
        self.url = url.clone();
        self.address_bar.update(cx, |editor, cx| {
            editor.set_text(url.clone(), window, cx);
        });
        if self.webview_initialized {
            window.navigate_webview(&url);
        }
        cx.notify();
    }

    fn navigate_to(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.url = url.to_string();
        self.address_bar.update(cx, |editor, cx| {
            editor.set_text(url, window, cx);
        });
        if self.webview_initialized {
            window.navigate_webview(url);
        }
        cx.notify();
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
        if !active {
            window.hide_webview();
        }
    }
}

impl Render for Browser {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();

        v_flex()
            .size_full()
            .bg(colors.panel_background)
            .child(
                h_flex()
                    .h(DynamicSpacing::Base32.px(cx))
                    .gap_1p5()
                    .px_2()
                    .border_b_1()
                    .border_color(colors.border_variant)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .capture_action(cx.listener(
                                |this, _: &editor::actions::Newline, window, cx| {
                                    this.navigate_to_address_bar_content(window, cx);
                                },
                            ))
                            .child(self.address_bar.clone()),
                    ),
            )
            .child(
                v_flex()
                    .px_2()
                    .py(DynamicSpacing::Base06.rems(cx))
                    .border_b_1()
                    .border_color(colors.border_variant)
                    .child(
                        h_flex()
                            .min_h_8()
                            .items_center()
                            .gap_0p5()
                            .children(BOOKMARKS.iter().enumerate().map(|(index, (title, url))| {
                                let url = url.to_string();
                                Button::new(index, *title)
                                    .style(ButtonStyle::Subtle)
                                    .size(ButtonSize::Compact)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.navigate_to(&url, window, cx);
                                    }))
                            })),
                    ),
            )
            .child(BrowserElement {
                browser: self.weak_self.clone(),
                url: self.url.clone(),
            })
    }
}

struct BrowserElement {
    browser: WeakEntity<Browser>,
    url: String,
}

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
        style.flex_grow = 1.0;
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let initialized = self
            .browser
            .read_with(cx, |browser, _| browser.webview_initialized)
            .unwrap_or(true);
        if initialized {
            window.update_webview(bounds);
        } else {
            window.add_webview(&self.url, bounds);
            self.browser
                .update(cx, |browser, _| {
                    browser.webview_initialized = true;
                })
                .ok();
        }
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

fn normalize_url(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return trimmed.to_string();
    }
    if !trimmed.contains(' ') && trimmed.contains('.') {
        return format!("https://{trimmed}");
    }
    let encoded = urlencoding::encode(trimmed);
    format!("https://www.google.com/search?q={encoded}")
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
                    browser.navigate_to(&url, window, cx);
                });
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
