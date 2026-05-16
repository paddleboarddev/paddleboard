// PaddleBoard: the AiUpsellCard is the centerpiece "join Zed AI" upsell —
// it described the Pro / Free / Trial plans and pointed users at
// `zed.dev/account/start-trial` and `zed.dev/account/upgrade`. PaddleBoard
// is BYO-keys and has no hosted plan tier, so the render is gutted to an
// empty element. The struct, constructor, and `tab_index` setter stay
// public so the half-dozen call sites (including the `Component` preview
// used by the storybook) keep compiling unchanged. The original render
// body sat between this comment block and the `impl Component` below;
// removed wholesale (~250 LOC).
use std::sync::Arc;

use client::{Client, UserStore};
use cloud_api_types::Plan;
use gpui::{AnyElement, App, Entity, IntoElement, RenderOnce, Window};
use ui::{RegisterComponent, prelude::*};

use crate::SignInStatus;

#[derive(IntoElement, RegisterComponent)]
pub struct AiUpsellCard {
    pub sign_in_status: SignInStatus,
    pub sign_in: Arc<dyn Fn(&mut Window, &mut App)>,
    pub account_too_young: bool,
    pub user_plan: Option<Plan>,
    pub tab_index: Option<isize>,
}

impl AiUpsellCard {
    pub fn new(
        client: Arc<Client>,
        user_store: &Entity<UserStore>,
        user_plan: Option<Plan>,
        cx: &mut App,
    ) -> Self {
        let status = *client.status().borrow();
        let store = user_store.read(cx);

        Self {
            user_plan,
            sign_in_status: status.into(),
            sign_in: Arc::new(move |_window, cx| {
                cx.spawn({
                    let client = client.clone();
                    async move |cx| client.sign_in_with_optional_connect(true, cx).await
                })
                .detach_and_log_err(cx);
            }),
            account_too_young: store.account_too_young(),
            tab_index: None,
        }
    }

    pub fn tab_index(mut self, tab_index: Option<isize>) -> Self {
        self.tab_index = tab_index;
        self
    }
}

impl RenderOnce for AiUpsellCard {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
    }
}

impl Component for AiUpsellCard {
    fn scope() -> ComponentScope {
        ComponentScope::Onboarding
    }

    fn name() -> &'static str {
        "AI Upsell Card"
    }

    fn sort_name() -> &'static str {
        "AI Upsell Card"
    }

    fn description() -> Option<&'static str> {
        // PaddleBoard: was "A card presenting the PaddleBoard AI product
        // during user's first-open onboarding flow." — the card no longer
        // renders anything, so the preview returns None below.
        None
    }

    fn preview(_window: &mut Window, _cx: &mut App) -> Option<AnyElement> {
        None
    }
}
