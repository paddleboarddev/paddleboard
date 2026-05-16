// PaddleBoard: this module used to enumerate Zed AI plan tiers (Free /
// Pro Trial / Pro / Business / Student) as bullet lists rendered inside
// the AI upsell card. PaddleBoard has no plan model, so every method
// returns an empty list — call sites keep compiling unchanged.

use gpui::IntoElement;
use ui::{List, prelude::*};

pub struct PlanDefinitions;

impl PlanDefinitions {
    pub const AI_DESCRIPTION: &'static str = "";

    pub fn free_plan(&self) -> impl IntoElement {
        List::new()
    }

    pub fn pro_trial(&self, _period: bool) -> impl IntoElement {
        List::new()
    }

    pub fn pro_plan(&self) -> impl IntoElement {
        List::new()
    }

    pub fn business_plan(&self) -> impl IntoElement {
        List::new()
    }

    pub fn student_plan(&self) -> impl IntoElement {
        List::new()
    }
}
