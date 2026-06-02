// PaddleBoard: this banner used to warn signed-in Zed users that their
// GitHub account was too young for the Zed Pro trial and pointed them at
// billing-support@zed.dev. PaddleBoard isn't a hosted service and has no
// trial concept, so the render is gutted to an empty element. The
// `YoungAccountBanner` struct stays public so upstream call sites (e.g.
// `crates/language_models/src/provider/cloud.rs::render`) keep
// compiling without any per-call-site PaddleBoard tag.
use gpui::IntoElement;
use ui::prelude::*;

#[derive(IntoElement)]
pub struct YoungAccountBanner;

impl RenderOnce for YoungAccountBanner {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
    }
}
