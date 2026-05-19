// PaddleBoard: this module used to enumerate Zed AI plan tiers (Free /
// Pro Trial / Pro / Business / Student) as bullet lists rendered inside
// the AI upsell card. PaddleBoard has no plan model, so every method
// returns an empty list — call sites keep compiling unchanged.

use gpui::IntoElement;
use ui::{List, prelude::*};

pub struct PlanDefinitions;

impl PlanDefinitions {
<<<<<<< HEAD
    pub const AI_DESCRIPTION: &'static str = "";

    pub fn free_plan(&self) -> impl IntoElement {
        List::new()
=======
    pub fn free_plan(&self) -> impl IntoElement {
        List::new()
            .child(ListBulletItem::new("2,000 accepted edit predictions"))
            .child(ListBulletItem::new(
                "Unlimited prompts with your AI API keys",
            ))
            .child(ListBulletItem::new("Unlimited use of external agents"))
    }

    pub fn sign_in_upsell(&self) -> impl IntoElement {
        List::new()
            .child(ListBulletItem::new("Unlimited edit predictions"))
            .child(ListBulletItem::new("$20 of tokens in Zed agent"))
            .child(ListBulletItem::new("No credit card required"))
>>>>>>> zed/main
    }

    pub fn pro_trial(&self, _period: bool) -> impl IntoElement {
        List::new()
<<<<<<< HEAD
=======
            .child(ListBulletItem::new("$20 of tokens in Zed agent"))
            .child(ListBulletItem::new("Unlimited edit predictions"))
            .when(period, |this| {
                this.child(ListBulletItem::new(
                    "Try it out for 14 days, no credit card required",
                ))
            })
>>>>>>> zed/main
    }

    pub fn pro_plan(&self) -> impl IntoElement {
        List::new()
<<<<<<< HEAD
=======
            .child(ListBulletItem::new("$5 of tokens in Zed agent"))
            .child(ListBulletItem::new("Usage-based billing beyond $5"))
            .child(ListBulletItem::new("Unlimited edit predictions"))
>>>>>>> zed/main
    }

    pub fn business_plan(&self) -> impl IntoElement {
        List::new()
    }

    pub fn student_plan(&self) -> impl IntoElement {
        List::new()
<<<<<<< HEAD
=======
            .child(ListBulletItem::new("Unlimited edit predictions"))
            .child(ListBulletItem::new("$10 of tokens in Zed agent"))
            .child(ListBulletItem::new(
                "Optional credit packs for additional usage",
            ))
>>>>>>> zed/main
    }
}
