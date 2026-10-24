//! PaddleBoard: model tiers inside the agent thread. After a turn that edited
//! files, run the project's check through the agent's own terminal tool (same
//! sandbox, same permission prompt, same card in the thread); hand a failure
//! back to the model once, then escalate or stop with a Blocker Summary.
//!
//! A child module of `thread` so it can use the thread's private event-stream
//! helpers without widening their visibility in the upstream file. The rules
//! themselves are plain data in the `paddleboard_tiers` crate. Design:
//! `MODEL_TIERS.md`.

use super::*;
use paddleboard_tiers::{CheckOutcome, Decision, TierAttempts, TiersSettings};
use settings::SettingsLocation;

/// Output lines the terminal tool returns to us. The tail is what matters for
/// compiler and test failures; `paddleboard_tiers` trims again for the model.
const CHECK_TAIL_LINES: usize = 200;

/// A stopped run waiting for the user, shown as the Blocker card.
#[derive(Debug, Clone, PartialEq)]
pub struct TierBlocker {
    /// The model the thread was on, e.g. "Local (gemma3:12b)".
    pub current_label: String,
    /// Tiers the user may move to, cheapest first.
    pub offer: Vec<TierOffer>,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TierOffer {
    pub index: usize,
    pub name: String,
    /// The model id, e.g. "claude-sonnet-5".
    pub model: String,
    /// Whether the tier's model is configured right now. An unconfigured tier
    /// is shown but cannot be chosen.
    pub available: bool,
}

#[derive(Debug, Default)]
pub(super) struct TierState {
    /// Failed checks on the current model since the last escalation.
    failed_on_current: u32,
    /// Failed checks per model for the current task, for the Blocker Summary.
    history: Vec<TierAttempts>,
    blocker: Option<TierBlocker>,
    /// The next user message is the escalation we sent; keep the history.
    escalation_pending: bool,
    check_count: u64,
}

/// What the turn loop should do after the check.
pub(super) enum AfterTurn {
    End,
    /// Continue the turn: a message was appended for the model.
    Continue,
    /// Continue on the model the thread now has (an automatic escalation),
    /// dropping any refusal-fallback override.
    ContinueOnNewModel,
}

impl Thread {
    /// Called when the user sends a message. A fresh request starts a fresh
    /// count; the message an escalation sends keeps the history it summarises.
    pub(super) fn paddleboard_tiers_note_user_message(&mut self) {
        let state = &mut self.paddleboard_tiers;
        state.blocker = None;
        state.failed_on_current = 0;
        if state.escalation_pending {
            state.escalation_pending = false;
        } else {
            state.history.clear();
        }
    }

    pub fn paddleboard_tier_blocker(&self) -> Option<&TierBlocker> {
        self.paddleboard_tiers.blocker.as_ref()
    }

    pub fn paddleboard_dismiss_tier_blocker(&mut self, cx: &mut Context<Self>) {
        self.paddleboard_tiers.blocker = None;
        cx.notify();
    }

    /// Switch to `tier` and return the message to send so the stronger model
    /// continues with the facts. `None` if the tier's model is not available.
    pub fn paddleboard_escalate_to_tier(
        &mut self,
        tier: usize,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let settings = self.paddleboard_tier_settings(cx);
        let target = settings.tiers.get(tier)?.clone();
        let model = Self::resolve_model_from_selection(&target.model, cx)?;
        let blocker = self.paddleboard_tiers.blocker.take()?;
        let to_label = format!("{} ({})", target.name, target.model.model);
        self.set_model(model, cx);
        self.paddleboard_tiers.failed_on_current = 0;
        self.paddleboard_tiers.escalation_pending = true;
        cx.notify();
        Some(paddleboard_tiers::escalation_message(
            &blocker.current_label,
            &to_label,
            &blocker.summary,
        ))
    }

    fn paddleboard_tier_settings(&self, cx: &App) -> TiersSettings {
        let location = self
            .project
            .read(cx)
            .visible_worktrees(cx)
            .next()
            .map(|worktree| SettingsLocation {
                worktree_id: worktree.read(cx).id(),
                path: util::rel_path::RelPath::empty(),
            });
        TiersSettings::get(location, cx).clone()
    }

    /// "Local (gemma3:12b)" for a tier model, or the model's own name.
    fn paddleboard_current_tier(&self, settings: &TiersSettings) -> (Option<usize>, String) {
        let Some(model) = self.model() else {
            return (None, "no model".to_string());
        };
        let index = settings.tier_index_for_model(&model.provider_id().0, &model.id().0);
        let label = match index.and_then(|index| settings.tiers.get(index)) {
            Some(tier) => format!("{} ({})", tier.name, tier.model.model),
            None => model.name().0.to_string(),
        };
        (index, label)
    }

    fn paddleboard_changed_files(&self, cx: &App) -> Vec<String> {
        let mut files: Vec<String> = self
            .action_log
            .read(cx)
            .changed_buffers(cx)
            .filter_map(|(buffer, _)| {
                let file = buffer.read(cx).file()?;
                Some(file.path().display(file.path_style(cx)).into_owned())
            })
            .collect();
        files.sort();
        files
    }

    fn paddleboard_record_failure(&mut self, label: &str) {
        let state = &mut self.paddleboard_tiers;
        state.failed_on_current += 1;
        match state.history.last_mut() {
            Some(last) if last.label == label => last.failed_checks += 1,
            _ => state.history.push(TierAttempts {
                label: label.to_string(),
                failed_checks: 1,
            }),
        }
    }

    /// Push a message for the model into the history and show it in the thread.
    fn paddleboard_push_check_message(
        &mut self,
        text: String,
        event_stream: &ThreadEventStream,
        cx: &mut Context<Self>,
    ) {
        let message = UserMessage {
            id: ClientUserMessageId::new(),
            content: vec![UserMessageContent::Text(text)].into(),
        };
        event_stream.send_user_message(&message);
        self.messages.push(Arc::new(Message::User(message)));
        cx.notify();
    }

    /// Run after a turn ends with no tool calls left. Decides whether the turn
    /// really ends, or continues with the check's failure or on a stronger tier.
    pub(super) async fn paddleboard_tiers_after_turn(
        this: &WeakEntity<Thread>,
        event_stream: &ThreadEventStream,
        cancellation_rx: watch::Receiver<bool>,
        cx: &mut AsyncApp,
    ) -> Result<AfterTurn> {
        let prepared = this.update(cx, |this, cx| {
            // Every turn, not only turns that edited files: a small model will
            // describe a fix, call no tool and say "done", and that run must not
            // end unverified (seen with gemma4:e4b, 2026-10-08).
            // Subagents report to a parent thread, which runs its own check.
            if this.parent_thread_id().is_some() {
                return None;
            }
            let settings = this.paddleboard_tier_settings(cx);
            if !settings.is_active() {
                return None;
            }
            let check = settings.check.clone()?;
            let cd = this
                .project
                .read(cx)
                .visible_worktrees(cx)
                .next()?
                .read(cx)
                .abs_path()
                .to_string_lossy()
                .into_owned();
            // The sandboxed variant when the sandbox is on, so the check runs
            // under exactly the policy the agent's own commands do.
            let tool = this
                .tools
                .get(SandboxedTerminalTool::NAME)
                .or_else(|| this.tools.get(TerminalTool::NAME))
                .cloned();
            this.paddleboard_tiers.check_count += 1;
            let tool_use_id = LanguageModelToolUseId::from(format!(
                "paddleboard-check-{}",
                this.paddleboard_tiers.check_count
            ));
            let tool_call_id = scoped_tool_call_id(this.messages.len(), &tool_use_id);
            let fs = this.project.read(cx).fs().clone();
            Some((
                settings,
                check,
                cd,
                tool,
                tool_use_id,
                tool_call_id,
                fs,
                this.sandbox_grants.clone(),
                cx.weak_entity(),
            ))
        })?;
        let Some((settings, check, cd, tool, tool_use_id, tool_call_id, fs, grants, weak)) =
            prepared
        else {
            return Ok(AfterTurn::End);
        };

        let outcome = match tool {
            None => CheckOutcome::Unavailable {
                reason: "the terminal tool is not available in this thread".to_string(),
            },
            Some(tool) => {
                let input = serde_json::json!({
                    "command": check.command,
                    "cd": cd,
                    "timeout_ms": check.timeout.as_millis() as u64,
                    "tail_lines": CHECK_TAIL_LINES,
                });
                event_stream.send_tool_call(
                    &tool_call_id,
                    tool.name().as_ref(),
                    // The bare command, as for any terminal call: the card's
                    // copy button copies this label, so a prefix would not run.
                    SharedString::from(check.command.clone()),
                    acp::ToolKind::Execute,
                    input.clone(),
                );
                let tool_event_stream = ToolCallEventStream::new(
                    tool_use_id,
                    tool_call_id.clone(),
                    event_stream.clone(),
                    Some(fs),
                    cancellation_rx,
                    grants,
                    Some(weak),
                );
                tool_event_stream.update_fields(
                    acp::ToolCallUpdateFields::new().status(acp::ToolCallStatus::InProgress),
                );
                let result = cx
                    .update(|cx| tool.run(ToolInput::ready(input), tool_event_stream, cx))
                    .await;
                let is_error = result.is_err();
                let output = match &result {
                    Ok(output) | Err(output) => output,
                };
                event_stream.update_tool_call_fields(
                    &tool_call_id,
                    acp::ToolCallUpdateFields::new()
                        .status(if is_error {
                            acp::ToolCallStatus::Failed
                        } else {
                            acp::ToolCallStatus::Completed
                        })
                        .raw_output(output.raw_output.clone()),
                    None,
                );
                let text = tool_output_text(output);
                paddleboard_tiers::classify_terminal_result(
                    &check.command,
                    if is_error {
                        Err(text.as_str())
                    } else {
                        Ok(text.as_str())
                    },
                )
            }
        };

        this.update(cx, |this, cx| {
            let (current_tier, current_label) = this.paddleboard_current_tier(&settings);
            let failed_before = this.paddleboard_tiers.failed_on_current;
            let decision =
                paddleboard_tiers::decide(&settings, current_tier, failed_before, &outcome);
            let output = match &outcome {
                CheckOutcome::Failed { output } => output.as_str(),
                _ => "",
            };
            match decision {
                Decision::Done => {
                    this.paddleboard_tiers.history.clear();
                    this.paddleboard_tiers.failed_on_current = 0;
                    Ok(AfterTurn::End)
                }
                Decision::Unavailable { reason } => {
                    event_stream.send_text(&format!(
                        "\n\nThe project check `{}` could not run: {reason}",
                        check.command
                    ));
                    Ok(AfterTurn::End)
                }
                Decision::Retry => {
                    this.paddleboard_record_failure(&current_label);
                    let text = paddleboard_tiers::retry_message(
                        &check.command,
                        failed_before + 1,
                        settings.retries_per_tier + 1,
                        output,
                    );
                    this.paddleboard_push_check_message(text, event_stream, cx);
                    Ok(AfterTurn::Continue)
                }
                Decision::Escalate { to } => {
                    this.paddleboard_record_failure(&current_label);
                    let summary = paddleboard_tiers::blocker_summary(
                        &check.command,
                        output,
                        &this.paddleboard_tiers.history,
                        &this.paddleboard_changed_files(cx),
                    );
                    let Some(tier) = settings.tiers.get(to) else {
                        return Ok(AfterTurn::End);
                    };
                    let Some(model) = Self::resolve_model_from_selection(&tier.model, cx) else {
                        // The tier's model is not configured; stop and ask
                        // rather than silently staying on the failing model.
                        this.paddleboard_tiers.blocker = Some(TierBlocker {
                            current_label,
                            offer: Vec::new(),
                            summary: format!(
                                "{summary}\n\nAutomatic escalation to {} failed: its model \
                                 {} is not configured.",
                                tier.name, tier.model.model
                            ),
                        });
                        cx.notify();
                        return Ok(AfterTurn::End);
                    };
                    let to_label = format!("{} ({})", tier.name, tier.model.model);
                    this.set_model(model, cx);
                    this.paddleboard_tiers.failed_on_current = 0;
                    event_stream.send_retry(acp_thread::RetryStatus {
                        last_error: format!("The check failed on {current_label}").into(),
                        attempt: 1,
                        max_attempts: 1,
                        started_at: Instant::now(),
                        duration: Duration::MAX,
                        meta: Some(acp_thread::meta_with_refusal_fallback(&to_label)),
                    });
                    let text =
                        paddleboard_tiers::escalation_message(&current_label, &to_label, &summary);
                    this.paddleboard_push_check_message(text, event_stream, cx);
                    Ok(AfterTurn::ContinueOnNewModel)
                }
                Decision::Blocked { offer } => {
                    this.paddleboard_record_failure(&current_label);
                    let summary = paddleboard_tiers::blocker_summary(
                        &check.command,
                        output,
                        &this.paddleboard_tiers.history,
                        &this.paddleboard_changed_files(cx),
                    );
                    let registry = LanguageModelRegistry::read_global(cx);
                    let offer = offer
                        .into_iter()
                        .filter_map(|index| {
                            let tier = settings.tiers.get(index)?;
                            let available = registry.available_models(cx).any(|model| {
                                model.provider_id().0 == tier.model.provider.0.as_str()
                                    && model.id().0 == tier.model.model.as_str()
                            });
                            Some(TierOffer {
                                index,
                                name: tier.name.clone(),
                                model: tier.model.model.clone(),
                                available,
                            })
                        })
                        .collect();
                    this.paddleboard_tiers.blocker = Some(TierBlocker {
                        current_label,
                        offer,
                        summary,
                    });
                    cx.notify();
                    Ok(AfterTurn::End)
                }
            }
        })?
    }
}

fn tool_output_text(output: &AgentToolOutput) -> String {
    output
        .llm_output
        .iter()
        .filter_map(|part| match part {
            LanguageModelToolResultContent::Text(text) => Some(text.as_ref()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::super::*;

    /// The check classifier reads the terminal tool's wording. Run the real
    /// formatter so a change to that wording fails here instead of silently
    /// turning every failed check into a pass.
    #[test]
    fn classifier_agrees_with_the_terminal_tool() {
        let outcome = |exit_code: Option<u32>, output: &str, timed_out, user_stopped| {
            let mut response = acp::TerminalOutputResponse::new(output.to_string(), false);
            if let Some(code) = exit_code {
                response = response.exit_status(acp::TerminalExitStatus::new().exit_code(code));
            }
            let text = crate::tools::paddleboard_process_content_for_tests(
                response,
                "cargo test",
                timed_out,
                user_stopped,
            );
            paddleboard_tiers::classify_terminal_result("cargo test", Ok(&text))
        };
        assert_eq!(
            outcome(Some(0), "ok", false, false),
            paddleboard_tiers::CheckOutcome::Passed
        );
        assert_eq!(
            outcome(Some(0), "", false, false),
            paddleboard_tiers::CheckOutcome::Passed
        );
        assert!(matches!(
            outcome(Some(101), "error[E0063]", false, false),
            paddleboard_tiers::CheckOutcome::Failed { .. }
        ));
        assert!(matches!(
            outcome(Some(1), "", false, false),
            paddleboard_tiers::CheckOutcome::Failed { .. }
        ));
        assert!(matches!(
            outcome(None, "partial", true, false),
            paddleboard_tiers::CheckOutcome::Failed { .. }
        ));
        assert!(matches!(
            outcome(None, "", false, false),
            paddleboard_tiers::CheckOutcome::Failed { .. }
        ));
        assert_eq!(
            outcome(None, "", false, true),
            paddleboard_tiers::CheckOutcome::Stopped
        );
    }
}
