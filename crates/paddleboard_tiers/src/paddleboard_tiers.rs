//! PaddleBoard: model tiers. Routine work runs on a cheap model; when a project
//! check fails twice, the work moves to a stronger one (or the user is asked).
//!
//! This crate holds the typed settings and the decision logic as plain data, so
//! the rules can be tested without a running app. The agent thread owns the
//! hooks that run the check and switch the model. Design: `MODEL_TIERS.md`.

use std::time::Duration;

use gpui::App;
use settings::{
    LanguageModelSelection, PaddleboardTierEscalationContent, RegisterSetting, Settings,
};

/// Force-link the crate so the `RegisterSetting` inventory entry for
/// [`TiersSettings`] is reachable.
pub fn init(_cx: &mut App) {}

const DEFAULT_RETRIES_PER_TIER: u32 = 1;
const DEFAULT_CHECK_TIMEOUT_SECONDS: u64 = 300;
/// How much of a failed check's output goes back to the model and into the
/// Blocker Summary. Compiler and test failures put the useful part at the end.
const OUTPUT_TAIL_LINES: usize = 60;

#[derive(Debug, Clone, PartialEq)]
pub struct Tier {
    pub name: String,
    pub model: LanguageModelSelection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckConfig {
    pub command: String,
    pub timeout: Duration,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Escalation {
    #[default]
    Ask,
    Auto,
    Off,
}

#[derive(Debug, Clone, Default, PartialEq, RegisterSetting)]
pub struct TiersSettings {
    pub enabled: bool,
    pub tiers: Vec<Tier>,
    pub check: Option<CheckConfig>,
    pub retries_per_tier: u32,
    pub escalation: Escalation,
    pub auto_ceiling: Option<String>,
}

impl Settings for TiersSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let Some(content) = content.project.paddleboard_spotter.as_ref() else {
            return Self {
                retries_per_tier: DEFAULT_RETRIES_PER_TIER,
                ..Self::default()
            };
        };
        let tiers = content
            .tiers
            .iter()
            .flatten()
            .filter_map(|tier| {
                let model = tier.model.clone()?;
                let name = tier.name.clone().unwrap_or_else(|| model.model.clone());
                Some(Tier { name, model })
            })
            .collect();
        let check = content.check.as_ref().and_then(|check| {
            let command = check.command.as_ref()?.trim();
            if command.is_empty() {
                return None;
            }
            Some(CheckConfig {
                command: command.to_string(),
                timeout: Duration::from_secs(
                    check
                        .timeout_seconds
                        .unwrap_or(DEFAULT_CHECK_TIMEOUT_SECONDS),
                ),
            })
        });
        Self {
            enabled: content.enabled.unwrap_or(false),
            tiers,
            check,
            retries_per_tier: content.retries_per_tier.unwrap_or(DEFAULT_RETRIES_PER_TIER),
            escalation: match content.escalation.unwrap_or_default() {
                PaddleboardTierEscalationContent::Ask => Escalation::Ask,
                PaddleboardTierEscalationContent::Auto => Escalation::Auto,
                PaddleboardTierEscalationContent::Off => Escalation::Off,
            },
            auto_ceiling: content.auto_ceiling.clone(),
        }
    }
}

impl TiersSettings {
    /// Tiers only act when they are on and there is a check to run.
    pub fn is_active(&self) -> bool {
        self.enabled && self.check.is_some()
    }

    /// The tier whose model is `(provider, model)`, if any.
    pub fn tier_index_for_model(&self, provider: &str, model: &str) -> Option<usize> {
        self.tiers
            .iter()
            .position(|tier| tier.model.provider.0 == provider && tier.model.model == model)
    }

    /// The tiers a thread on `current` may move up to, cheapest first. A thread
    /// on a model that is not a tier may move to any tier.
    pub fn escalation_targets(&self, current: Option<usize>) -> Vec<usize> {
        let start = current.map_or(0, |index| index + 1);
        (start..self.tiers.len()).collect()
    }

    fn ceiling_index(&self) -> Option<usize> {
        let name = self.auto_ceiling.as_ref()?;
        self.tiers.iter().position(|tier| &tier.name == name)
    }
}

/// What the check said about the agent's work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    Passed,
    Failed {
        output: String,
    },
    /// The user stopped the check. Their call; nothing runs after it.
    Stopped,
    /// The check could not run at all (denied, no terminal, no project).
    Unavailable {
        reason: String,
    },
}

/// Classify the terminal tool's result for the check `command`.
///
/// The terminal tool reports a non-zero exit, a timeout and an unexpected
/// termination with a fixed sentence, and a success as fenced output or
/// `Command executed successfully.` (see `process_content` in the agent's
/// terminal tool). Sandbox notes may come first, separated by a blank line, so
/// the sentence is matched at the start of the text or of any paragraph, never
/// only at the very start. The agent crate tests this against the real
/// formatter so a wording change there fails loudly.
pub fn classify_terminal_result(command: &str, result: Result<&str, &str>) -> CheckOutcome {
    let text = match result {
        Err(reason) => {
            return CheckOutcome::Unavailable {
                reason: reason.to_string(),
            };
        }
        Ok(text) => text,
    };
    let has_paragraph =
        |marker: &str| text.starts_with(marker) || text.contains(&format!("\n\n{marker}"));
    if has_paragraph("The user stopped this command") {
        return CheckOutcome::Stopped;
    }
    let failed = has_paragraph(&format!("Command \"{command}\" failed with exit code"))
        || has_paragraph(&format!("Command \"{command}\" timed out"))
        || has_paragraph("Command terminated unexpectedly");
    if failed {
        CheckOutcome::Failed {
            output: text.to_string(),
        }
    } else {
        CheckOutcome::Passed
    }
}

/// What the thread should do after a check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// The work passed, or the user stopped the check. The turn ends.
    Done,
    /// Hand the failure back to the same model for another attempt.
    Retry,
    /// Switch to this tier and continue without asking.
    Escalate { to: usize },
    /// Stop and show the Blocker card, offering these tiers (possibly none).
    Blocked { offer: Vec<usize> },
    /// The check could not run. Say so and end the turn.
    Unavailable { reason: String },
}

/// Decide the next step after a check.
///
/// `failed_attempts` counts the failed checks on the current tier *before*
/// this one, so the first failure is `0`.
pub fn decide(
    settings: &TiersSettings,
    current_tier: Option<usize>,
    failed_attempts: u32,
    outcome: &CheckOutcome,
) -> Decision {
    match outcome {
        CheckOutcome::Passed | CheckOutcome::Stopped => Decision::Done,
        CheckOutcome::Unavailable { reason } => Decision::Unavailable {
            reason: reason.clone(),
        },
        CheckOutcome::Failed { .. } => {
            if failed_attempts < settings.retries_per_tier {
                return Decision::Retry;
            }
            let offer = settings.escalation_targets(current_tier);
            match settings.escalation {
                Escalation::Off => Decision::Blocked { offer: Vec::new() },
                Escalation::Ask => Decision::Blocked { offer },
                Escalation::Auto => {
                    let Some(&next) = offer.first() else {
                        return Decision::Blocked { offer };
                    };
                    let within_ceiling = settings
                        .ceiling_index()
                        .is_none_or(|ceiling| next <= ceiling);
                    if within_ceiling {
                        Decision::Escalate { to: next }
                    } else {
                        Decision::Blocked { offer }
                    }
                }
            }
        }
    }
}

/// The last `OUTPUT_TAIL_LINES` lines of `output`.
pub fn output_tail(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    let start = lines.len().saturating_sub(OUTPUT_TAIL_LINES);
    let tail = lines[start..].join("\n");
    if start > 0 {
        format!("[{start} earlier lines omitted]\n{tail}")
    } else {
        tail
    }
}

/// The message that hands a failed check back to the same model.
pub fn retry_message(command: &str, attempt: u32, attempts_allowed: u32, output: &str) -> String {
    format!(
        "The project check `{command}` failed after your changes \
         (attempt {attempt} of {attempts_allowed} on this model). \
         Find and fix the cause, then stop: the check runs again automatically.\n\n{}",
        output_tail(output)
    )
}

/// One line of the Blocker Summary's history: a model and how many failed
/// checks it had.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TierAttempts {
    pub label: String,
    pub failed_checks: u32,
}

/// The facts a stronger model (or the user) needs. Deliberately not generated
/// by the model that failed: asking it to summarise its own failure costs a
/// call and invites the same mistake.
pub fn blocker_summary(
    command: &str,
    output: &str,
    history: &[TierAttempts],
    changed_files: &[String],
) -> String {
    let mut summary = format!("Blocker Summary\n\nCheck: `{command}`\n");
    summary.push_str("Tried:\n");
    for attempt in history {
        let noun = if attempt.failed_checks == 1 {
            "failed check"
        } else {
            "failed checks"
        };
        summary.push_str(&format!(
            "- {}: {} {noun}\n",
            attempt.label, attempt.failed_checks
        ));
    }
    if !changed_files.is_empty() {
        summary.push_str("Files changed so far:\n");
        for file in changed_files {
            summary.push_str(&format!("- {file}\n"));
        }
    }
    summary.push_str("\nLast check output:\n");
    summary.push_str(&output_tail(output));
    summary
}

/// The message that continues the task on a stronger tier.
pub fn escalation_message(from: &str, to: &str, summary: &str) -> String {
    format!(
        "Escalated from {from} to {to} because the project check kept failing. \
         Continue the task and make the check pass.\n\n{summary}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use settings::LanguageModelProviderSetting;

    fn tier(name: &str, provider: &str, model: &str) -> Tier {
        Tier {
            name: name.to_string(),
            model: LanguageModelSelection {
                provider: LanguageModelProviderSetting(provider.to_string()),
                model: model.to_string(),
                enable_thinking: false,
                effort: None,
                speed: None,
            },
        }
    }

    fn settings(escalation: Escalation, ceiling: Option<&str>) -> TiersSettings {
        TiersSettings {
            enabled: true,
            tiers: vec![
                tier("Local", "ollama", "gemma3:12b"),
                tier("Fast", "google", "gemini-3.5-flash"),
                tier("Precise", "anthropic", "claude-sonnet-5"),
            ],
            check: Some(CheckConfig {
                command: "cargo test".into(),
                timeout: Duration::from_secs(300),
            }),
            retries_per_tier: 1,
            escalation,
            auto_ceiling: ceiling.map(str::to_string),
        }
    }

    fn failed() -> CheckOutcome {
        CheckOutcome::Failed {
            output: "Command \"cargo test\" failed with exit code 101.".into(),
        }
    }

    #[test]
    fn pass_ends_the_turn() {
        let settings = settings(Escalation::Ask, None);
        assert_eq!(
            decide(&settings, Some(0), 0, &CheckOutcome::Passed),
            Decision::Done
        );
    }

    #[test]
    fn user_stop_ends_the_turn_without_a_strike() {
        let settings = settings(Escalation::Auto, None);
        assert_eq!(
            decide(&settings, Some(0), 1, &CheckOutcome::Stopped),
            Decision::Done
        );
    }

    #[test]
    fn first_failure_retries_on_the_same_tier() {
        let settings = settings(Escalation::Ask, None);
        assert_eq!(decide(&settings, Some(0), 0, &failed()), Decision::Retry);
    }

    #[test]
    fn second_failure_with_ask_blocks_and_offers_every_higher_tier() {
        let settings = settings(Escalation::Ask, None);
        assert_eq!(
            decide(&settings, Some(0), 1, &failed()),
            Decision::Blocked { offer: vec![1, 2] }
        );
    }

    #[test]
    fn second_failure_with_auto_escalates_one_tier() {
        let settings = settings(Escalation::Auto, None);
        assert_eq!(
            decide(&settings, Some(0), 1, &failed()),
            Decision::Escalate { to: 1 }
        );
    }

    #[test]
    fn auto_stops_and_asks_above_the_ceiling() {
        let settings = settings(Escalation::Auto, Some("Fast"));
        assert_eq!(
            decide(&settings, Some(0), 1, &failed()),
            Decision::Escalate { to: 1 }
        );
        assert_eq!(
            decide(&settings, Some(1), 1, &failed()),
            Decision::Blocked { offer: vec![2] }
        );
    }

    #[test]
    fn off_blocks_without_offering_anything() {
        let settings = settings(Escalation::Off, None);
        assert_eq!(
            decide(&settings, Some(0), 1, &failed()),
            Decision::Blocked { offer: Vec::new() }
        );
    }

    #[test]
    fn top_tier_blocks_with_nothing_to_offer() {
        let settings = settings(Escalation::Auto, None);
        assert_eq!(
            decide(&settings, Some(2), 1, &failed()),
            Decision::Blocked { offer: Vec::new() }
        );
    }

    #[test]
    fn a_model_outside_the_tiers_is_offered_every_tier() {
        let settings = settings(Escalation::Ask, None);
        assert_eq!(
            decide(&settings, None, 1, &failed()),
            Decision::Blocked {
                offer: vec![0, 1, 2]
            }
        );
    }

    #[test]
    fn more_retries_delay_the_block() {
        let mut settings = settings(Escalation::Ask, None);
        settings.retries_per_tier = 2;
        assert_eq!(decide(&settings, Some(0), 1, &failed()), Decision::Retry);
        assert_eq!(
            decide(&settings, Some(0), 2, &failed()),
            Decision::Blocked { offer: vec![1, 2] }
        );
    }

    #[test]
    fn unavailable_check_is_reported_not_retried() {
        let settings = settings(Escalation::Auto, None);
        assert_eq!(
            decide(
                &settings,
                Some(0),
                0,
                &CheckOutcome::Unavailable {
                    reason: "denied".into()
                }
            ),
            Decision::Unavailable {
                reason: "denied".into()
            }
        );
    }

    #[test]
    fn tiers_need_both_enabled_and_a_check() {
        let mut settings = settings(Escalation::Ask, None);
        assert!(settings.is_active());
        settings.check = None;
        assert!(!settings.is_active());
        settings = self::settings(Escalation::Ask, None);
        settings.enabled = false;
        assert!(!settings.is_active());
    }

    #[test]
    fn tier_lookup_matches_provider_and_model() {
        let settings = settings(Escalation::Ask, None);
        assert_eq!(
            settings.tier_index_for_model("google", "gemini-3.5-flash"),
            Some(1)
        );
        assert_eq!(
            settings.tier_index_for_model("anthropic", "gemini-3.5-flash"),
            None
        );
    }

    #[test]
    fn classifies_terminal_results() {
        let classify = |text: &str| classify_terminal_result("x", Ok(text));
        assert_eq!(classify("```\nok\n```"), CheckOutcome::Passed);
        assert_eq!(
            classify("Command executed successfully."),
            CheckOutcome::Passed
        );
        assert!(matches!(
            classify("Command \"x\" failed with exit code 1."),
            CheckOutcome::Failed { .. }
        ));
        assert!(matches!(
            classify("Command \"x\" timed out. No output was captured."),
            CheckOutcome::Failed { .. }
        ));
        assert!(matches!(
            classify("Command terminated unexpectedly. No output was captured."),
            CheckOutcome::Failed { .. }
        ));
        assert_eq!(
            classify("The user stopped this command. No output was captured before stopping."),
            CheckOutcome::Stopped
        );
        assert_eq!(
            classify_terminal_result("x", Err("Permission denied")),
            CheckOutcome::Unavailable {
                reason: "Permission denied".into()
            }
        );
    }

    #[test]
    fn a_sandbox_note_before_the_result_does_not_hide_a_failure() {
        let classify = |text: &str| classify_terminal_result("x", Ok(text));
        assert!(matches!(
            classify("Ran without the sandbox.\n\nCommand \"x\" failed with exit code 2."),
            CheckOutcome::Failed { .. }
        ));
        assert_eq!(
            classify("Ran without the sandbox.\n\n```\nok\n```"),
            CheckOutcome::Passed
        );
    }

    #[test]
    fn another_commands_failure_text_in_the_output_is_not_a_failure() {
        assert_eq!(
            classify_terminal_result(
                "cargo test",
                Ok("```\nCommand \"make\" failed with exit code 2.\n```")
            ),
            CheckOutcome::Passed
        );
    }

    /// The docs promise that a project can add just its check on top of the
    /// tiers in the user settings. That relies on settings merging field by
    /// field, so prove it with the real store.
    #[gpui::test]
    fn project_check_merges_with_user_tiers(cx: &mut gpui::App) {
        use settings::{LocalSettingsKind, LocalSettingsPath, SettingsLocation, SettingsStore};
        use util::rel_path::rel_path;

        let mut store = SettingsStore::test(cx);
        store.register_setting::<TiersSettings>();
        store
            .set_user_settings(
                r#"{ "paddleboard_spotter": { "enabled": true, "tiers": [
                    { "name": "Local", "model": { "provider": "ollama", "model": "gemma3:12b" } }
                ] } }"#,
                cx,
            )
            .result()
            .unwrap();
        let worktree_id = settings::WorktreeId::from_usize(1);
        store
            .set_local_settings(
                worktree_id,
                LocalSettingsPath::InWorktree(rel_path("").into()),
                LocalSettingsKind::Settings,
                Some(r#"{ "paddleboard_spotter": { "check": { "command": "cargo test" } } }"#),
                cx,
            )
            .unwrap();

        let user = store.get::<TiersSettings>(None);
        assert!(user.check.is_none());
        assert!(!user.is_active());

        let project = store.get::<TiersSettings>(Some(SettingsLocation {
            worktree_id,
            path: rel_path("src"),
        }));
        assert!(project.is_active());
        assert_eq!(project.tiers.len(), 1);
        assert_eq!(project.tiers[0].name, "Local");
        assert_eq!(
            project.check.as_ref().map(|check| check.command.as_str()),
            Some("cargo test")
        );
    }

    #[test]
    fn output_tail_keeps_the_end() {
        let output: String = (1..=100).map(|n| format!("line {n}\n")).collect();
        let tail = output_tail(&output);
        assert!(tail.starts_with("[40 earlier lines omitted]\nline 41"));
        assert!(tail.ends_with("line 100"));
        assert_eq!(output_tail("short"), "short");
    }

    #[test]
    fn blocker_summary_lists_the_facts() {
        let summary = blocker_summary(
            "cargo test",
            "error[E0063]: missing field",
            &[TierAttempts {
                label: "Local (gemma3:12b)".into(),
                failed_checks: 2,
            }],
            &["src/foo.rs".into()],
        );
        assert_eq!(
            summary,
            "Blocker Summary\n\nCheck: `cargo test`\nTried:\n- Local (gemma3:12b): 2 failed checks\n\
             Files changed so far:\n- src/foo.rs\n\nLast check output:\nerror[E0063]: missing field"
        );
    }
}
