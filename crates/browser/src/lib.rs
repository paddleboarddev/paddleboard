use gpui::{
    App, AppContext, Bounds, Context, Element, ElementId, EventEmitter, FocusHandle, Focusable,
    GlobalElementId, IntoElement, LayoutId, Pixels, Render, SharedString, Window,
};
use std::panic::Location;
use ui::{Icon, IconName};
use workspace::{Workspace, item::Item};

pub struct Browser {
    focus_handle: FocusHandle,
    url: String,
}

impl Browser {
    pub fn new(url: impl Into<String>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            url: url.into(),
        }
    }
}

pub enum BrowserEvent {}

impl EventEmitter<BrowserEvent> for Browser {}

impl Focusable for Browser {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Item for Browser {
    type Event = BrowserEvent;

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> SharedString {
        format!("Browser: {}", self.url).into()
    }

    fn tab_icon(&self, _window: &Window, _cx: &App) -> Option<Icon> {
        Some(Icon::new(IconName::ToolWeb))
    }
}

struct BrowserElement {
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
        // Here we intercept the bounds of the element and tell the native window to update the WKWebView
        // using the GPUI method we added!
        window.update_webview(bounds);
        ()
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

impl Render for Browser {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        // Render the BrowserElement, which will track layout bounds and communicate with WKWebView
        BrowserElement {
            url: self.url.clone(),
        }
    }
}

pub fn init(cx: &mut App) {
    cx.observe_new(
        |workspace: &mut Workspace, _window: Option<&mut Window>, _cx: &mut Context<Workspace>| {
            workspace.register_action(|workspace, _: &workspace::OpenBrowser, window, cx| {
                let browser = Box::new(cx.new(|cx| Browser::new("https://google.com", cx)));
                // Inform native side to mount Webview
                window.add_webview("https://google.com", Default::default());
                workspace.add_item_to_active_pane(browser, None, true, window, cx);
            });
        },
    )
    .detach();
}
