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
<<<<<<< HEAD
=======
            .child(ListBulletItem::new("Unlimited edit predictions"))
            .child(ListBulletItem::new("$5 of GPT Luna"))
            .child(ListBulletItem::new("No credit card required"))
    }

    pub fn pro_trial(&self, period: bool) -> impl IntoElement {
        List::new()
            .child(ListBulletItem::new("$5 of GPT Luna"))
            .child(ListBulletItem::new("Unlimited edit predictions"))
            .when(period, |this| {
                this.child(ListBulletItem::new(
                    "14 days from trial start, no credit card required",
                ))
            })
>>>>>>> zed/main
    }

    pub fn pro_plan(&self) -> impl IntoElement {
        List::new()
    }

    pub fn business_plan(&self) -> impl IntoElement {
        List::new()
    }

    pub fn vip_plan(&self) -> impl IntoElement {
        List::new()
    }

    pub fn student_plan(&self) -> impl IntoElement {
        List::new()
    }
}
