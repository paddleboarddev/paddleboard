//! PaddleBoard: the Blocker card for model tiers (MODEL_TIERS.md). Shown when
//! the project check has failed on the current model as many times as allowed:
//! it offers the stronger tiers, and choosing one switches the thread's model
//! and sends the Blocker Summary so the stronger model continues with the facts.
//!
//! A child module of `thread_view` so the upstream file only gains a module
//! declaration and one render call.

use super::*;

impl ThreadView {
    pub(super) fn render_paddleboard_tier_blocker(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<Callout> {
        let thread = self.as_native_thread(cx)?;
        let blocker = thread.read(cx).paddleboard_tier_blocker()?.clone();

        let description = if blocker.offer.is_empty() {
            "No stronger tier to offer. The check's output is in the thread above: fix the failure, or send a message to try again."
                .to_string()
        } else {
            "Move the task to a stronger model. It gets the full history and a summary of what failed."
                .to_string()
        };

        let buttons = blocker.offer.iter().map(|offer| {
            let index = offer.index;
            let button = Button::new(
                SharedString::from(format!("paddleboard-escalate-tier-{index}")),
                format!("Escalate to {}", offer.name),
            )
            .label_size(LabelSize::Small)
            .disabled(!offer.available);
            let button = if offer.available {
                button.tooltip(Tooltip::text(format!("Switch this thread to {}", offer.model)))
            } else {
                button.tooltip(Tooltip::text(format!(
                    "{} is not configured. Add its provider in the model settings.",
                    offer.model
                )))
            };
            button.on_click(cx.listener(move |this, _, window, cx| {
                this.paddleboard_escalate_to_tier(index, window, cx);
            }))
        });

        Some(
            Callout::new()
                .severity(Severity::Warning)
                .icon(IconName::Warning)
                .title(format!(
                    "Spotter: the check kept failing on {}",
                    blocker.current_label
                ))
                .description(description)
                .border_position(self.callout_border_position())
                .actions_slot(h_flex().gap_1().children(buttons))
                .dismiss_action(
                    IconButton::new("paddleboard-dismiss-tier-blocker", IconName::Close)
                        .icon_size(IconSize::Small)
                        .icon_color(Color::Muted)
                        .tooltip(Tooltip::text("Dismiss"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(thread) = this.as_native_thread(cx) {
                                thread.update(cx, |thread, cx| {
                                    thread.paddleboard_dismiss_tier_blocker(cx)
                                });
                            }
                        })),
                ),
        )
    }

    fn paddleboard_escalate_to_tier(
        &mut self,
        tier: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(thread) = self.as_native_thread(cx) else {
            return;
        };
        let Some(message) =
            thread.update(cx, |thread, cx| thread.paddleboard_escalate_to_tier(tier, cx))
        else {
            log::warn!("model tiers: could not escalate to tier {tier}; its model is unavailable");
            return;
        };
        // Sent like a typed message, so the conversation view, history and
        // persistence all see it the same way.
        let contents = Task::ready(Ok(Some((
            vec![acp_v1::ContentBlock::Text(acp_v1::TextContent::new(
                message,
            ))],
            Vec::new(),
        ))));
        self.send_content(contents, false, window, cx);
    }
}
