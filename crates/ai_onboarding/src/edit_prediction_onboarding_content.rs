use std::sync::Arc;

use client::{Client, UserStore};
use gpui::{Entity, IntoElement};
use ui::prelude::*;

use crate::ZedAiOnboarding;

pub struct EditPredictionOnboarding {
    user_store: Entity<UserStore>,
    client: Arc<Client>,
    continue_with_zed_ai: Arc<dyn Fn(&mut Window, &mut App)>,
}

impl EditPredictionOnboarding {
    pub fn new(
        user_store: Entity<UserStore>,
        client: Arc<Client>,
        continue_with_zed_ai: Arc<dyn Fn(&mut Window, &mut App)>,
        _cx: &mut Context<Self>,
    ) -> Self {
        Self {
            user_store,
            client,
            continue_with_zed_ai,
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
<<<<<<< HEAD
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
=======
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ZedAiOnboarding::new(
            self.client.clone(),
            &self.user_store,
            self.continue_with_zed_ai.clone(),
            cx,
        )
>>>>>>> zed/main
    }
}
