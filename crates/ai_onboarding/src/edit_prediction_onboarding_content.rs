use std::sync::Arc;

use client::{Client, UserStore};
use cloud_api_types::Plan;
use gpui::{Entity, IntoElement, ParentElement};
use ui::prelude::*;

use crate::ZedAiOnboarding;

pub struct EditPredictionOnboarding {
    user_store: Entity<UserStore>,
    client: Arc<Client>,
    copilot_is_configured: bool,
    continue_with_zed_ai: Arc<dyn Fn(&mut Window, &mut App)>,
    continue_with_copilot: Arc<dyn Fn(&mut Window, &mut App)>,
}

impl EditPredictionOnboarding {
    pub fn new(
        user_store: Entity<UserStore>,
        client: Arc<Client>,
        copilot_is_configured: bool,
        continue_with_zed_ai: Arc<dyn Fn(&mut Window, &mut App)>,
        continue_with_copilot: Arc<dyn Fn(&mut Window, &mut App)>,
        _cx: &mut Context<Self>,
    ) -> Self {
        Self {
            user_store,
            copilot_is_configured,
            client,
            continue_with_zed_ai,
            continue_with_copilot,
        }
    }
}

// PaddleBoard: render gutted — was a `ZedAiOnboarding` card with an
// optional "Configure GitHub Copilot" fallback for free-plan users.
// `ZedAiOnboarding` is itself empty now, and the Copilot affordance
// was framed as an alternative to Zed AI, not a primary path; we
// surface Copilot configuration through the regular settings UI
// instead. Constructor stays so call sites compile.
impl Render for EditPredictionOnboarding {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
