mod agent_api_keys_onboarding;
mod agent_panel_onboarding_card;
mod agent_panel_onboarding_content;
mod ai_upsell_card;
mod edit_prediction_onboarding_content;
mod plan_definitions;
mod young_account_banner;

pub use agent_api_keys_onboarding::{ApiKeysWithProviders, ApiKeysWithoutProviders};
pub use agent_panel_onboarding_card::AgentPanelOnboardingCard;
pub use agent_panel_onboarding_content::AgentPanelOnboarding;
pub use ai_upsell_card::AiUpsellCard;
use cloud_api_types::Plan;
pub use edit_prediction_onboarding_content::EditPredictionOnboarding;
pub use plan_definitions::PlanDefinitions;
pub use young_account_banner::YoungAccountBanner;

use std::sync::Arc;

use client::{Client, UserStore};
use gpui::{AnyElement, Entity, IntoElement, ParentElement};
use ui::{RegisterComponent, Tooltip, prelude::*};

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
    }

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
    }

    fn render_trial_state(&self, _cx: &mut App) -> AnyElement {
        v_flex()
            .relative()
            .gap_1()
            .child(Headline::new("Welcome to PaddleBoard AI"))
            .children(self.render_dismiss_button())
            .into_any_element()
    }

    fn render_pro_plan_state(&self, _cx: &mut App) -> AnyElement {
        v_flex()
            .gap_1()
            .child(Headline::new("Welcome to PaddleBoard Pro"))
            .child(
                Label::new("Here's what you get:")
                    .color(Color::Muted)
                    .mb_2(),
            )
            .child(PlanDefinitions.pro_plan())
            .children(self.render_dismiss_button())
            .into_any_element()
    }

    fn render_business_plan_state(&self, _cx: &mut App) -> AnyElement {
        v_flex()
            .gap_1()
            .child(Headline::new("Welcome to PaddleBoard Business"))
            .child(
                Label::new("Here's what you get:")
                    .color(Color::Muted)
                    .mb_2(),
            )
            .child(PlanDefinitions.business_plan())
            .children(self.render_dismiss_button())
            .into_any_element()
    }

    fn render_student_plan_state(&self, _cx: &mut App) -> AnyElement {
        v_flex()
            .gap_1()
            .child(Headline::new("Welcome to PaddleBoard Student"))
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

impl RenderOnce for ZedAiOnboarding {
    fn render(self, _window: &mut ui::Window, cx: &mut App) -> impl IntoElement {
        if matches!(self.sign_in_status, SignInStatus::SignedIn) {
            match self.plan {
                None => self.render_free_plan_state(cx),
                Some(Plan::ZedFree) => self.render_free_plan_state(cx),
                Some(Plan::ZedProTrial) => self.render_trial_state(cx),
                Some(Plan::ZedPro) => self.render_pro_plan_state(cx),
                Some(Plan::ZedBusiness) => self.render_business_plan_state(cx),
                Some(Plan::ZedStudent) => self.render_student_plan_state(cx),
            }
        } else {
            self.render_sign_in_disclaimer(cx)
        }
    }
}

impl Component for ZedAiOnboarding {
    fn scope() -> ComponentScope {
        ComponentScope::Onboarding
    }

    fn name() -> &'static str {
        "Agent Panel Banners"
    }

    fn sort_name() -> &'static str {
        "Agent Panel Banners"
    }

    fn preview(_window: &mut Window, _cx: &mut App) -> Option<AnyElement> {
        fn onboarding(
            sign_in_status: SignInStatus,
            plan: Option<Plan>,
            account_too_young: bool,
        ) -> AnyElement {
            ZedAiOnboarding {
                sign_in_status,
                plan,
                account_too_young,
                continue_with_zed_ai: Arc::new(|_, _| {}),
                sign_in: Arc::new(|_, _| {}),
                dismiss_onboarding: None,
            }
            .into_any_element()
        }

        Some(
            v_flex()
                .gap_4()
                .items_center()
                .max_w_4_5()
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
