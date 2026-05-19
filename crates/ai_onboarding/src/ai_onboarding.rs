// PaddleBoard: most renders in this crate are gutted to empty elements
// (see the `// PaddleBoard:` comments on each `impl Render`). The
// gutted impls leave plenty of imports, struct fields, and helper
// methods unused — we keep them on disk so upstream merges resolve
// inside the deleted bodies cleanly rather than fighting reverted
// hunks. Suppress the predictable lint noise crate-wide instead of
// per-file.
#![allow(dead_code, unused_imports, unused_variables)]

mod agent_api_keys_onboarding;
mod agent_panel_onboarding_card;
mod agent_panel_onboarding_content;
mod edit_prediction_onboarding_content;
mod plan_definitions;
mod young_account_banner;

pub use agent_api_keys_onboarding::{ApiKeysWithProviders, ApiKeysWithoutProviders};
pub use agent_panel_onboarding_card::AgentPanelOnboardingCard;
pub use agent_panel_onboarding_content::AgentPanelOnboarding;
use cloud_api_types::Plan;
pub use edit_prediction_onboarding_content::EditPredictionOnboarding;
pub use plan_definitions::PlanDefinitions;
pub use young_account_banner::YoungAccountBanner;

use std::sync::Arc;

<<<<<<< HEAD
use client::{Client, UserStore};
use gpui::{AnyElement, Entity, IntoElement, ParentElement};
use ui::{RegisterComponent, Tooltip, prelude::*};
=======
use client::{Client, UserStore, zed_urls};
use gpui::{AnyElement, Entity, IntoElement, ParentElement, TaskExt};
use ui::{Divider, RegisterComponent, Tooltip, Vector, VectorName, prelude::*};
>>>>>>> zed/main

#[derive(PartialEq)]
pub enum SignInStatus {
    SignedIn,
    SigningIn,
    SignedOut,
}

impl From<client::Status> for SignInStatus {
    fn from(status: client::Status) -> Self {
        if status.is_signing_in() {
            Self::SigningIn
        } else if status.is_signed_out() {
            Self::SignedOut
        } else {
            Self::SignedIn
        }
    }
}

#[derive(RegisterComponent, IntoElement)]
pub struct ZedAiOnboarding {
    pub sign_in_status: SignInStatus,
    pub plan: Option<Plan>,
    pub account_too_young: bool,
    pub continue_with_zed_ai: Arc<dyn Fn(&mut Window, &mut App)>,
    pub sign_in: Arc<dyn Fn(&mut Window, &mut App)>,
    pub dismiss_onboarding: Option<Arc<dyn Fn(&mut Window, &mut App)>>,
}

impl ZedAiOnboarding {
    pub fn new(
        client: Arc<Client>,
        user_store: &Entity<UserStore>,
        continue_with_zed_ai: Arc<dyn Fn(&mut Window, &mut App)>,
        cx: &mut App,
    ) -> Self {
        let store = user_store.read(cx);
        let status = *client.status().borrow();

        Self {
            sign_in_status: status.into(),
            plan: store.plan(),
            account_too_young: store.account_too_young(),
            continue_with_zed_ai,
            sign_in: Arc::new(move |_window, cx| {
                cx.spawn({
                    let client = client.clone();
                    async move |cx| client.sign_in_with_optional_connect(true, cx).await
                })
                .detach_and_log_err(cx);
            }),
            dismiss_onboarding: None,
        }
    }

    pub fn with_dismiss(
        mut self,
        dismiss_callback: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.dismiss_onboarding = Some(Arc::new(dismiss_callback));
        self
    }

    fn certified_user_stamp(cx: &App) -> impl IntoElement {
        div().absolute().bottom_1().right_1().child(
            Vector::new(
                VectorName::ProUserStamp,
                rems_from_px(156.),
                rems_from_px(60.),
            )
            .color(Color::Custom(cx.theme().colors().text_accent.alpha(0.8))),
        )
    }

    fn pro_trial_stamp(cx: &App) -> impl IntoElement {
        div().absolute().bottom_1().right_1().child(
            Vector::new(
                VectorName::ProTrialStamp,
                rems_from_px(156.),
                rems_from_px(60.),
            )
            .color(Color::Custom(cx.theme().colors().text.alpha(0.8))),
        )
    }

    fn business_stamp(cx: &App) -> impl IntoElement {
        div().absolute().bottom_1().right_1().child(
            Vector::new(
                VectorName::BusinessStamp,
                rems_from_px(156.),
                rems_from_px(60.),
            )
            .color(Color::Custom(cx.theme().colors().text_accent.alpha(0.8))),
        )
    }

    fn student_stamp(cx: &App) -> impl IntoElement {
        div().absolute().bottom_1().right_1().child(
            Vector::new(
                VectorName::StudentStamp,
                rems_from_px(156.),
                rems_from_px(60.),
            )
            .color(Color::Custom(cx.theme().colors().text.alpha(0.8))),
        )
    }

    fn render_dismiss_button(&self) -> Option<AnyElement> {
        self.dismiss_onboarding.as_ref().map(|dismiss_callback| {
            let callback = dismiss_callback.clone();

            h_flex()
                .absolute()
                .top_0()
                .right_0()
                .child(
                    IconButton::new("dismiss_onboarding", IconName::Close)
                        .icon_size(IconSize::Small)
                        .tooltip(Tooltip::text("Dismiss"))
                        .on_click(move |_, window, cx| {
                            telemetry::event!("Banner Dismissed", source = "AI Onboarding",);
                            callback(window, cx)
                        }),
                )
                .into_any_element()
        })
    }

    fn render_sign_in_disclaimer(&self, _cx: &mut App) -> AnyElement {
        let _ = self.sign_in_status;

        v_flex()
            .w_full()
            .relative()
            .gap_1()
            .child(Headline::new("Welcome to PaddleBoard AI"))
            .child(
<<<<<<< HEAD
                Label::new("Configure a language model provider to get started.")
                    .color(Color::Muted)
                    .mb_2(),
            )
=======
                Label::new("Sign in to try Zed Pro free for 14 days.")
                    .color(Color::Muted)
                    .mb_2(),
            )
            .child(PlanDefinitions.sign_in_upsell())
            .child(
                Button::new("sign_in", "Try Zed Pro for Free")
                    .disabled(signing_in)
                    .full_width()
                    .style(ButtonStyle::Tinted(ui::TintColor::Accent))
                    .on_click({
                        let callback = self.sign_in.clone();
                        move |_, window, cx| {
                            telemetry::event!("Start Trial Clicked", state = "pre-sign-in");
                            callback(window, cx)
                        }
                    }),
            )
>>>>>>> zed/main
            .children(self.render_dismiss_button())
            .into_any_element()
    }

<<<<<<< HEAD
    fn render_free_plan_state(&self, _cx: &mut App) -> AnyElement {
        let _ = self.account_too_young;

        v_flex()
            .relative()
            .gap_1()
            .child(Headline::new("Welcome to PaddleBoard AI"))
            .child(
                Label::new("Configure a language model provider to get started.")
                    .color(Color::Muted)
                    .mb_2(),
            )
            .children(self.render_dismiss_button())
            .into_any_element()
=======
    fn render_free_plan_state(&self, cx: &mut App) -> AnyElement {
        if self.account_too_young {
            v_flex()
                .relative()
                .min_w_0()
                .gap_1()
                .child(Headline::new("Welcome to Zed AI"))
                .child(YoungAccountBanner)
                .child(
                    v_flex()
                        .mt_2()
                        .gap_1()
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Label::new("Pro")
                                        .size(LabelSize::Small)
                                        .color(Color::Accent)
                                        .buffer_font(cx),
                                )
                                .child(Divider::horizontal()),
                        )
                        .child(PlanDefinitions.pro_plan())
                        .child(
                            Button::new("pro", "Get Started")
                                .full_width()
                                .style(ButtonStyle::Tinted(ui::TintColor::Accent))
                                .on_click(move |_, _window, cx| {
                                    telemetry::event!(
                                        "Upgrade To Pro Clicked",
                                        state = "young-account"
                                    );
                                    cx.open_url(&zed_urls::upgrade_to_zed_pro_url(cx))
                                }),
                        ),
                )
                .into_any_element()
        } else {
            v_flex()
                .w_full()
                .relative()
                .gap_1()
                .child(Headline::new("Welcome to Zed AI"))
                .child(
                    v_flex()
                        .mt_2()
                        .gap_1()
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Label::new("Free")
                                        .size(LabelSize::Small)
                                        .color(Color::Muted)
                                        .buffer_font(cx),
                                )
                                .child(
                                    Label::new("(Current Plan)")
                                        .size(LabelSize::Small)
                                        .color(Color::Custom(
                                            cx.theme().colors().text_muted.opacity(0.6),
                                        ))
                                        .buffer_font(cx),
                                )
                                .child(Divider::horizontal()),
                        )
                        .child(PlanDefinitions.free_plan()),
                )
                .children(self.render_dismiss_button())
                .child(
                    v_flex()
                        .mt_2()
                        .gap_1()
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Label::new("Pro Trial")
                                        .size(LabelSize::Small)
                                        .color(Color::Accent)
                                        .buffer_font(cx),
                                )
                                .child(Divider::horizontal()),
                        )
                        .child(PlanDefinitions.pro_trial(true))
                        .child(
                            Button::new("pro", "Start Free Trial")
                                .full_width()
                                .style(ButtonStyle::Tinted(ui::TintColor::Accent))
                                .on_click(move |_, _window, cx| {
                                    telemetry::event!(
                                        "Start Trial Clicked",
                                        state = "post-sign-in"
                                    );
                                    cx.open_url(&zed_urls::start_trial_url(cx))
                                }),
                        ),
                )
                .into_any_element()
        }
>>>>>>> zed/main
    }

    fn render_trial_state(&self, cx: &mut App) -> AnyElement {
        v_flex()
            .w_full()
            .relative()
            .gap_1()
<<<<<<< HEAD
            .child(Headline::new("Welcome to PaddleBoard AI"))
=======
            .child(Self::pro_trial_stamp(cx))
            .child(Headline::new("Welcome to the Zed Pro Trial"))
            .child(
                Label::new("Here's what you get for the next 14 days:")
                    .color(Color::Muted)
                    .mb_2(),
            )
            .child(PlanDefinitions.pro_trial(false))
>>>>>>> zed/main
            .children(self.render_dismiss_button())
            .into_any_element()
    }

    fn render_pro_plan_state(&self, cx: &mut App) -> AnyElement {
        v_flex()
            .w_full()
            .relative()
            .gap_1()
<<<<<<< HEAD
            .child(Headline::new("Welcome to PaddleBoard Pro"))
=======
            .child(Self::certified_user_stamp(cx))
            .child(Headline::new("Welcome to Zed Pro"))
>>>>>>> zed/main
            .child(
                Label::new("Here's what you get:")
                    .color(Color::Muted)
                    .mb_2(),
            )
            .child(PlanDefinitions.pro_plan())
            .children(self.render_dismiss_button())
            .into_any_element()
    }

    fn render_business_plan_state(&self, cx: &mut App) -> AnyElement {
        v_flex()
            .w_full()
            .relative()
            .gap_1()
<<<<<<< HEAD
            .child(Headline::new("Welcome to PaddleBoard Business"))
=======
            .child(Self::business_stamp(cx))
            .child(Headline::new("Welcome to Zed Business"))
>>>>>>> zed/main
            .child(
                Label::new("Here's what you get:")
                    .color(Color::Muted)
                    .mb_2(),
            )
            .child(PlanDefinitions.business_plan())
            .children(self.render_dismiss_button())
            .into_any_element()
    }

    fn render_student_plan_state(&self, cx: &mut App) -> AnyElement {
        v_flex()
            .w_full()
            .relative()
            .gap_1()
<<<<<<< HEAD
            .child(Headline::new("Welcome to PaddleBoard Student"))
=======
            .child(Self::student_stamp(cx))
            .child(Headline::new("Welcome to Zed Student"))
>>>>>>> zed/main
            .child(
                Label::new("Here's what you get:")
                    .color(Color::Muted)
                    .mb_2(),
            )
            .child(PlanDefinitions.student_plan())
            .children(self.render_dismiss_button())
            .into_any_element()
    }
}

// PaddleBoard: gutted — see `young_account_banner.rs` for rationale.
// The match-arm helpers (`render_free_plan_state`, `render_trial_state`,
// etc.) are still defined above so upstream merges resolve cleanly into
// their bodies; nothing calls them after this render returns empty.
impl RenderOnce for ZedAiOnboarding {
    fn render(self, _window: &mut ui::Window, _cx: &mut App) -> impl IntoElement {
        div().into_any_element()
    }
}

impl Component for ZedAiOnboarding {
    fn scope() -> ComponentScope {
        ComponentScope::Onboarding
    }

    fn name() -> &'static str {
        "Agent New User Onboarding"
    }

    fn preview(_window: &mut Window, _cx: &mut App) -> Option<AnyElement> {
        fn onboarding(
            sign_in_status: SignInStatus,
            plan: Option<Plan>,
            account_too_young: bool,
        ) -> AnyElement {
            div()
                .w_full()
                .min_w_40()
                .max_w(px(1100.))
                .child(
                    AgentPanelOnboardingCard::new().child(
                        ZedAiOnboarding {
                            sign_in_status,
                            plan,
                            account_too_young,
                            continue_with_zed_ai: Arc::new(|_, _| {}),
                            sign_in: Arc::new(|_, _| {}),
                            dismiss_onboarding: None,
                        }
                        .into_any_element(),
                    ),
                )
                .into_any_element()
        }

        Some(
            v_flex()
                .min_w_0()
                .gap_4()
                .children(vec![
                    single_example(
                        "Not Signed-in",
                        onboarding(SignInStatus::SignedOut, None, false),
                    ),
                    single_example(
                        "Young Account",
                        onboarding(SignInStatus::SignedIn, None, true),
                    ),
                    single_example(
                        "Free Plan",
                        onboarding(SignInStatus::SignedIn, Some(Plan::ZedFree), false),
                    ),
                    single_example(
                        "Pro Trial",
                        onboarding(SignInStatus::SignedIn, Some(Plan::ZedProTrial), false),
                    ),
                    single_example(
                        "Pro Plan",
                        onboarding(SignInStatus::SignedIn, Some(Plan::ZedPro), false),
                    ),
                    single_example(
                        "Business Plan",
                        onboarding(SignInStatus::SignedIn, Some(Plan::ZedBusiness), false),
                    ),
                    single_example(
                        "Student Plan",
                        onboarding(SignInStatus::SignedIn, Some(Plan::ZedStudent), false),
                    ),
                ])
                .into_any_element(),
        )
    }
}
