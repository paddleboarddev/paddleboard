// PaddleBoard: end-to-end tests for model tiers (MODEL_TIERS.md) through the
// real turn loop: a fake model, the real terminal tool, and a terminal that
// exits with a scripted code per check.

use super::*;
use pretty_assertions::assert_eq;
use std::cell::RefCell;
use std::collections::VecDeque;

/// Returns a terminal exiting with the next scripted code on each call.
struct ScriptedEnvironment {
    exit_codes: RefCell<VecDeque<u32>>,
    commands: RefCell<Vec<String>>,
}

impl ScriptedEnvironment {
    fn new(exit_codes: &[u32]) -> Self {
        Self {
            exit_codes: RefCell::new(exit_codes.iter().copied().collect()),
            commands: RefCell::new(Vec::new()),
        }
    }
}

impl crate::ThreadEnvironment for ScriptedEnvironment {
    fn create_terminal(
        &self,
        command: String,
        _extra_env: Vec<acp::EnvVariable>,
        _cwd: Option<std::path::PathBuf>,
        _output_byte_limit: Option<u64>,
        _sandbox_wrap: Option<acp_thread::SandboxWrap>,
        cx: &mut AsyncApp,
    ) -> Task<Result<Rc<dyn crate::TerminalHandle>>> {
        self.commands.borrow_mut().push(command);
        let exit_code = self
            .exit_codes
            .borrow_mut()
            .pop_front()
            .expect("the test scripted fewer checks than ran");
        let handle = cx.update(|cx| {
            FakeTerminalHandle::new_with_immediate_exit(cx, exit_code).with_output(
                acp::TerminalOutputResponse::new(
                    if exit_code == 0 {
                        "test result: ok".to_string()
                    } else {
                        "error[E0063]: missing field `placid`".to_string()
                    },
                    false,
                )
                // The terminal tool reads the exit code from the output
                // response, not from the exit future.
                .exit_status(acp::TerminalExitStatus::new().exit_code(exit_code)),
            )
        });
        Task::ready(Ok(Rc::new(handle) as Rc<dyn crate::TerminalHandle>))
    }

    fn create_subagent(
        &self,
        _label: String,
        _model: Option<AgentModelId>,
        _cx: &mut App,
    ) -> Result<Rc<dyn SubagentHandle>> {
        Err(anyhow::anyhow!("not used by these tests"))
    }
}

fn enable_tiers(model: &LanguageModel, escalation: &str, cx: &mut TestAppContext) {
    let provider = model.provider_id().0.to_string();
    let local = model.id().0.to_string();
    cx.update(|cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |content| {
                content.project.paddleboard_spotter = Some(
                    serde_json::from_value(json!({
                        "enabled": true,
                        "tiers": [
                            { "name": "Local", "model": { "provider": provider, "model": local } },
                            { "name": "Precise", "model": { "provider": provider, "model": "fake-precise" } },
                        ],
                        "check": { "command": "cargo test" },
                        "retries_per_tier": 1,
                        "escalation": escalation,
                    }))
                    .unwrap(),
                );
            });
        });
    });
}

/// Send a message and end the model's reply with text. No tool is called: the
/// check runs after every turn, edits or not.
async fn turn(
    thread: &Entity<Thread>,
    fake: &FakeLanguageModelProvider,
    model: &LanguageModel,
    environment: Rc<ScriptedEnvironment>,
    cx: &mut TestAppContext,
) -> mpsc::UnboundedReceiver<Result<ThreadEvent>> {
    let events = thread
        .update(cx, |thread, cx| {
            thread.add_tool(crate::TerminalTool::new(
                thread.project().clone(),
                environment,
            ));
            thread.send(ClientUserMessageId::new(), ["add the field"], cx)
        })
        .unwrap();
    cx.run_until_parked();
    fake.send_last_text(model, "Done.");
    fake.end_last(model);
    events
}

fn last_request_text(fake: &FakeLanguageModelProvider) -> String {
    let request = fake
        .pending_completions()
        .pop()
        .expect("expected the model to be asked again");
    request
        .messages
        .last()
        .map(|message| message.string_contents())
        .unwrap_or_default()
}

#[gpui::test]
async fn test_passing_check_ends_the_turn(cx: &mut TestAppContext) {
    let ThreadTest {
        model,
        fake,
        thread,
        ..
    } = setup(cx, TestModel::Fake).await;
    enable_tiers(&model, "ask", cx);
    // After the settings write, which would otherwise drop these overrides.
    always_allow_tools(cx);
    disable_sandboxing(cx);
    let environment = Rc::new(ScriptedEnvironment::new(&[0]));

    let mut events = turn(&thread, &fake, &model, environment.clone(), cx).await;
    let events = collect_events_until_stop(&mut events, cx).await;

    assert_eq!(stop_events(events), vec![acp::StopReason::EndTurn]);
    assert_eq!(environment.commands.borrow().len(), 1);
    assert!(fake.pending_completions().is_empty());
    thread.read_with(cx, |thread, _| {
        assert!(thread.paddleboard_tier_blocker().is_none())
    });
}

#[gpui::test]
async fn test_failing_check_retries_once_then_blocks(cx: &mut TestAppContext) {
    let ThreadTest {
        model,
        fake,
        thread,
        ..
    } = setup(cx, TestModel::Fake).await;
    enable_tiers(&model, "ask", cx);
    // After the settings write, which would otherwise drop these overrides.
    always_allow_tools(cx);
    disable_sandboxing(cx);
    let environment = Rc::new(ScriptedEnvironment::new(&[1, 1]));

    let mut events = turn(&thread, &fake, &model, environment.clone(), cx).await;
    cx.run_until_parked();

    // Strike one: the failure goes back to the same model.
    let retry = last_request_text(&fake);
    assert!(
        retry.contains("The project check `cargo test` failed")
            && retry.contains("attempt 1 of 2")
            && retry.contains("missing field `placid`"),
        "unexpected retry message: {retry}"
    );

    // The model claims a fix without editing anything; the check runs anyway.
    fake.send_last_text(&model, "Fixed.");
    fake.end_last(&model);
    let events = collect_events_until_stop(&mut events, cx).await;

    // Strike two: stop and offer the stronger tier.
    assert_eq!(stop_events(events), vec![acp::StopReason::EndTurn]);
    assert_eq!(environment.commands.borrow().len(), 2);
    thread.read_with(cx, |thread, _| {
        let blocker = thread
            .paddleboard_tier_blocker()
            .expect("expected a Blocker card");
        assert_eq!(
            blocker.offer,
            vec![TierOffer {
                index: 1,
                name: "Precise".to_string(),
                model: "fake-precise".to_string(),
                // Not registered with the fake provider in this test.
                available: false,
            }]
        );
        assert!(
            blocker.summary.contains("Local (fake): 2 failed checks"),
            "unexpected summary: {}",
            blocker.summary
        );
    });
}

#[gpui::test]
async fn test_off_blocks_without_offering_a_tier(cx: &mut TestAppContext) {
    let ThreadTest {
        model,
        fake,
        thread,
        ..
    } = setup(cx, TestModel::Fake).await;
    enable_tiers(&model, "off", cx);
    // After the settings write, which would otherwise drop these overrides.
    always_allow_tools(cx);
    disable_sandboxing(cx);
    let environment = Rc::new(ScriptedEnvironment::new(&[1, 1]));

    let mut events = turn(&thread, &fake, &model, environment, cx).await;
    cx.run_until_parked();
    fake.send_last_text(&model, "Fixed.");
    fake.end_last(&model);
    collect_events_until_stop(&mut events, cx).await;

    thread.read_with(cx, |thread, _| {
        let blocker = thread.paddleboard_tier_blocker().expect("expected a block");
        assert!(blocker.offer.is_empty());
    });
}

#[gpui::test]
async fn test_check_runs_after_a_turn_with_no_tool_calls(cx: &mut TestAppContext) {
    let ThreadTest {
        model,
        fake,
        thread,
        ..
    } = setup(cx, TestModel::Fake).await;
    enable_tiers(&model, "ask", cx);
    // After the settings write, which would otherwise drop these overrides.
    always_allow_tools(cx);
    disable_sandboxing(cx);
    let environment = Rc::new(ScriptedEnvironment::new(&[1]));

    // The model describes a fix and calls no tool, as gemma4:e4b did on a real
    // run. The check still runs, and its failure goes back to the model.
    let _events = thread
        .update(cx, |thread, cx| {
            thread.add_tool(crate::TerminalTool::new(
                thread.project().clone(),
                environment.clone(),
            ));
            thread.send(ClientUserMessageId::new(), ["implement evaluate()"], cx)
        })
        .unwrap();
    cx.run_until_parked();
    fake.send_last_text(&model, "I have implemented evaluate() in calc.py.");
    fake.end_last(&model);
    cx.run_until_parked();

    assert_eq!(environment.commands.borrow().len(), 1);
    let retry = last_request_text(&fake);
    assert!(
        retry.contains("The project check `cargo test` failed"),
        "unexpected retry message: {retry}"
    );
}

#[gpui::test]
async fn test_escalating_from_the_blocker_switches_model(cx: &mut TestAppContext) {
    let ThreadTest {
        model,
        fake,
        thread,
        ..
    } = setup(cx, TestModel::Fake).await;
    enable_tiers(&model, "ask", cx);
    // After the settings write, which would otherwise drop these overrides.
    always_allow_tools(cx);
    disable_sandboxing(cx);
    let precise = fake.model("fake-precise");
    let environment = Rc::new(ScriptedEnvironment::new(&[1, 1]));

    let mut events = turn(&thread, &fake, &model, environment, cx).await;
    cx.run_until_parked();
    fake.send_last_text(&model, "Fixed.");
    fake.end_last(&model);
    collect_events_until_stop(&mut events, cx).await;

    let message = thread.update(cx, |thread, cx| thread.paddleboard_escalate_to_tier(1, cx));
    let message = message.expect("expected the escalation message");
    assert!(
        message.starts_with("Escalated from Local (fake) to Precise (fake-precise)"),
        "unexpected escalation message: {message}"
    );
    thread.read_with(cx, |thread, _| {
        assert_eq!(thread.model().map(|model| model.id()), Some(precise.id()));
        assert!(thread.paddleboard_tier_blocker().is_none());
    });
}
