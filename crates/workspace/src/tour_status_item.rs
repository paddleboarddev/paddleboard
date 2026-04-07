use crate::item::ItemHandle;
use crate::{OpenPaddleBoardTour, StatusItemView};
use gpui::{Context, IntoElement, Render, Window};
use ui::{Button, Tooltip, prelude::*};

pub struct TourStatusItem;

impl TourStatusItem {
    pub fn new(_: &mut Context<Self>) -> Self {
        Self
    }
}

impl StatusItemView for TourStatusItem {
    fn set_active_pane_item(
        &mut self,
        _active_pane_item: Option<&dyn ItemHandle>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }
}

impl Render for TourStatusItem {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        Button::new("tour-btn", "🏄‍♂️ Tour")
            .style(ButtonStyle::Subtle)
            .tooltip(move |window, _cx| Tooltip::text("Open PaddleBoard Guided Tour")(window, _cx))
            .on_click(cx.listener(|_, _, window, cx| {
                window.dispatch_action(Box::new(OpenPaddleBoardTour), cx);
            }))
    }
}
