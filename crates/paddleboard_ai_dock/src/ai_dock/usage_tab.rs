use std::path::PathBuf;

use gpui::ClickEvent;
use paddleboard_usage::{ProviderModelTotals, TokenCounts, UsageSummary};
use ui::{Callout, Severity, Tooltip, prelude::*};

use crate::ai_dock::AiDock;

pub(super) fn render(modal: &AiDock, cx: &mut Context<AiDock>) -> impl IntoElement {
    // The summary is cached on the modal and refreshed on tab-switch / the
    // refresh button, so rendering never hits disk (or git).
    let summary = modal.usage_summary.clone().unwrap_or_default();

    v_flex()
        .id("ai-dock-usage")
        .size_full()
        .p_4()
        .gap_3()
        .overflow_y_scroll()
        .child(render_header(&summary, cx))
        .map(|this| {
            if !summary.enabled {
                this.child(render_disabled_notice(cx))
            } else if summary.rows.is_empty() {
                this.child(render_empty_notice(cx))
            } else {
                this.child(render_totals_cards(&summary, cx))
                    .child(render_breakdown(&summary, cx))
            }
        })
}

fn render_header(summary: &UsageSummary, cx: &mut Context<AiDock>) -> impl IntoElement {
    let directory = summary.directory.clone();
    h_flex()
        .w_full()
        .justify_between()
        .pb_1()
        .child(
            v_flex()
                .gap_0p5()
                .child(
                    Label::new("Usage by provider")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .when_some(directory.clone(), |this, dir| {
                    this.child(
                        Label::new(dir.to_string_lossy().to_string())
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                }),
        )
        .child(
            h_flex()
                .gap_1()
                .when_some(directory, |this, dir| {
                    this.child(
                        Button::new("usage-open-folder", "Open Folder")
                            .style(ButtonStyle::Subtle)
                            .label_size(LabelSize::Small)
                            .on_click(move |_: &ClickEvent, _window, cx| {
                                reveal_directory(&dir, cx);
                            }),
                    )
                })
                .child(
                    IconButton::new("usage-refresh", IconName::ArrowCircle)
                        .tooltip(Tooltip::text("Refresh"))
                        .on_click(cx.listener(|modal, _: &ClickEvent, _window, cx| {
                            modal.refresh_usage(cx);
                        })),
                ),
        )
}

fn reveal_directory(dir: &PathBuf, cx: &mut App) {
    // Best-effort: make sure the directory exists so the file manager has
    // something to open the first time, before any usage has been written.
    let _ = std::fs::create_dir_all(dir);
    cx.reveal_path(dir);
}

fn render_disabled_notice(_cx: &mut Context<AiDock>) -> impl IntoElement {
    div().py_4().child(
        Callout::new()
            .severity(Severity::Info)
            .title("Usage tracking is turned off.")
            .description(
                "Set \"paddleboard_usage\": { \"enabled\": true } in your settings to record \
                 per-provider token usage locally.",
            ),
    )
}

fn render_empty_notice(_cx: &mut Context<AiDock>) -> impl IntoElement {
    div().py_4().child(
        Callout::new()
            .severity(Severity::Info)
            .title("No usage recorded yet.")
            .description(
                "Token usage will appear here after you run the agent. Everything stays on your \
                 machine.",
            ),
    )
}

fn render_totals_cards(summary: &UsageSummary, cx: &mut Context<AiDock>) -> impl IntoElement {
    h_flex()
        .w_full()
        .gap_2()
        .child(total_card("Today", summary.today, cx))
        .child(total_card("Last 7 days", summary.last_7_days, cx))
        .child(total_card("All time", summary.all_time, cx))
}

fn total_card(label: &str, counts: TokenCounts, cx: &mut Context<AiDock>) -> impl IntoElement {
    v_flex()
        .flex_1()
        .p_3()
        .gap_1()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().colors().border_variant)
        .bg(cx.theme().colors().elevated_surface_background.opacity(0.5))
        .child(
            Label::new(label.to_string())
                .size(LabelSize::XSmall)
                .color(Color::Muted),
        )
        .child(Headline::new(format_tokens(counts.total())).size(HeadlineSize::Small))
        .child(
            Label::new(format!(
                "{} in · {} out",
                format_tokens(counts.input_tokens),
                format_tokens(counts.output_tokens)
            ))
            .size(LabelSize::XSmall)
            .color(Color::Muted),
        )
}

fn render_breakdown(summary: &UsageSummary, cx: &mut Context<AiDock>) -> impl IntoElement {
    v_flex()
        .w_full()
        .mt_1()
        .child(render_breakdown_header(cx))
        .children(
            summary
                .rows
                .iter()
                .enumerate()
                .map(|(index, row)| render_breakdown_row(index, row, cx)),
        )
}

fn render_breakdown_header(cx: &mut Context<AiDock>) -> impl IntoElement {
    h_flex()
        .w_full()
        .py_1p5()
        .px_2()
        .border_b_1()
        .border_color(cx.theme().colors().border_variant)
        .child(div().flex_1().child(muted_label("Provider · Model")))
        .child(div().w_24().text_right().child(muted_label("Today")))
        .child(div().w_24().text_right().child(muted_label("7 days")))
        .child(div().w_24().text_right().child(muted_label("All time")))
}

fn render_breakdown_row(
    index: usize,
    row: &ProviderModelTotals,
    cx: &mut Context<AiDock>,
) -> AnyElement {
    h_flex()
        .w_full()
        .py_1p5()
        .px_2()
        .when(index % 2 == 1, |this| {
            this.bg(cx.theme().colors().elevated_surface_background.opacity(0.5))
        })
        .child(
            v_flex()
                .flex_1()
                .gap_0p5()
                .child(Label::new(row.model.clone()).size(LabelSize::Small))
                .child(
                    Label::new(row.provider.clone())
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                ),
        )
        .child(
            div()
                .w_24()
                .text_right()
                .child(value_label(row.today.total())),
        )
        .child(
            div()
                .w_24()
                .text_right()
                .child(value_label(row.last_7_days.total())),
        )
        .child(
            div()
                .w_24()
                .text_right()
                .child(value_label(row.all_time.total())),
        )
        .into_any_element()
}

fn muted_label(text: &str) -> Label {
    Label::new(text.to_string())
        .size(LabelSize::XSmall)
        .color(Color::Muted)
}

fn value_label(value: u64) -> Label {
    let color = if value == 0 {
        Color::Muted
    } else {
        Color::Default
    };
    Label::new(format_tokens(value))
        .size(LabelSize::Small)
        .color(color)
}

/// Human-readable token count, e.g. `0`, `1,204`, `3.4K`, `12.6M`.
fn format_tokens(value: u64) -> String {
    if value < 1_000 {
        value.to_string()
    } else if value < 1_000_000 {
        format!("{:.1}K", value as f64 / 1_000.0)
    } else if value < 1_000_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else {
        format!("{:.1}B", value as f64 / 1_000_000_000.0)
    }
}
