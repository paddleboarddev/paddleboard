use askpass::EncryptedPassword;
use editor::Editor;
use futures::channel::oneshot;
use gpui::{AppContext, DismissEvent, Entity, EventEmitter, Focusable, Styled, TaskExt};
use ui::{
    ActiveTheme, AnyElement, App, Button, Checkbox, Clickable, Color, Context, DynamicSpacing,
    Headline, HeadlineSize, Icon, IconName, IconSize, InteractiveElement, IntoElement, Label,
    LabelCommon, LabelSize, ParentElement, Render, SharedString, StyledExt, StyledTypography,
    ToggleState, Window, div, h_flex, v_flex,
};
use util::maybe;
use workspace::ModalView;
use zeroize::Zeroize;

pub(crate) struct AskPassModal {
    operation: SharedString,
    prompt: SharedString,
    editor: Entity<Editor>,
    tx: Option<oneshot::Sender<EncryptedPassword>>,
    // PaddleBoard: when the prompt is an HTTPS password prompt with an embedded
    // username, offer to save the submitted token as a Git Login (keychain).
    remember_target: Option<(String, String)>,
    remember: bool,
}

impl EventEmitter<DismissEvent> for AskPassModal {}
impl ModalView for AskPassModal {}
impl Focusable for AskPassModal {
    fn focus_handle(&self, cx: &App) -> gpui::FocusHandle {
        self.editor.focus_handle(cx)
    }
}

impl AskPassModal {
    pub fn new(
        operation: SharedString,
        prompt: SharedString,
        tx: oneshot::Sender<EncryptedPassword>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let editor = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            if prompt.contains("yes/no") || prompt.contains("Username") {
                editor.set_masked(false, cx);
            } else {
                editor.set_masked(true, cx);
            }
            editor
        });
        let remember_target = paddleboard_git_login::parse_git_prompt(&prompt)
            .filter(|(_, kind)| *kind == paddleboard_git_login::PromptKind::Password)
            .and_then(|(url, _)| {
                let username = paddleboard_git_login::url_username(&url)?;
                Some((url, username))
            });
        Self {
            operation,
            prompt,
            editor,
            tx: Some(tx),
            remember_target,
            remember: false,
        }
    }

    fn cancel(&mut self, _: &menu::Cancel, _window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }

    fn confirm(&mut self, _: &menu::Confirm, window: &mut Window, cx: &mut Context<Self>) {
        maybe!({
            let tx = self.tx.take()?;
            let mut text = self.editor.update(cx, |this, cx| {
                let text = this.text(cx);
                this.clear(window, cx);
                text
            });
            // PaddleBoard: save-on-submit. The keychain write races the git
            // operation on purpose — answering git must not wait on the keychain.
            if self.remember && !text.is_empty() {
                if let Some((url, username)) = self.remember_target.clone() {
                    let token = text.clone();
                    let provider = paddleboard_credentials_provider::global(cx);
                    cx.spawn(async move |_, cx| {
                        paddleboard_git_login::save(&url, &username, &token, provider.as_ref(), cx)
                            .await
                    })
                    .detach_and_log_err(cx);
                }
            }
            let pw = askpass::EncryptedPassword::try_from(text.as_ref()).ok()?;
            text.zeroize();
            tx.send(pw).ok();
            Some(())
        });

        cx.emit(DismissEvent);
    }

    fn render_remember(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.remember_target.as_ref()?;
        Some(
            h_flex()
                .px_3()
                .pb_2()
                .bg(cx.theme().colors().editor_background)
                .child(
                    Checkbox::new("remember-credential", self.remember.into())
                        .label("Remember on this device (saved to your keychain)")
                        .label_size(LabelSize::Small)
                        .on_click(cx.listener(|this, state: &ToggleState, _window, cx| {
                            this.remember = *state == ToggleState::Selected;
                            cx.notify();
                        })),
                )
                .into_any_element(),
        )
    }

    fn render_hint(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let color = cx.theme().status().info_background;
        if (self.prompt.contains("Password") || self.prompt.contains("Username"))
            && self.prompt.contains("github.com")
        {
            return Some(
            div()
                .p_2()
                .bg(color)
                .border_t_1()
                .border_color(cx.theme().status().info_border)
                .child(
                    h_flex().gap_2()
                        .child(
                            Icon::new(IconName::Github).size(IconSize::Small)
                        )
                        .child(
                            Label::new("You may need to configure git for Github.")
                                .size(LabelSize::Small),
                        )
                        .child(Button::new("learn-more", "Learn more").color(Color::Accent).label_size(LabelSize::Small).on_click(|_, _, cx| {
                            cx.open_url("https://docs.github.com/en/get-started/git-basics/set-up-git#authenticating-with-github-from-git")
                        })),
                )
                .into_any_element(),
        );
        }
        None
    }
}

impl Render for AskPassModal {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("PasswordPrompt")
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::confirm))
            .elevation_2(cx)
            .size_full()
            .child(
                h_flex()
                    .font_buffer(cx)
                    .px(DynamicSpacing::Base12.rems(cx))
                    .pt(DynamicSpacing::Base08.rems(cx))
                    .pb(DynamicSpacing::Base04.rems(cx))
                    .rounded_t_sm()
                    .w_full()
                    .gap_1p5()
                    .child(Icon::new(IconName::GitBranch).size(IconSize::XSmall))
                    .child(h_flex().gap_1().overflow_x_hidden().child(
                        div().max_w_96().overflow_x_hidden().text_ellipsis().child(
                            Headline::new(self.operation.clone()).size(HeadlineSize::XSmall),
                        ),
                    )),
            )
            .child(
                div()
                    .font_buffer(cx)
                    .text_buffer(cx)
                    .py_2()
                    .px_3()
                    .bg(cx.theme().colors().editor_background)
                    .border_t_1()
                    .border_color(cx.theme().colors().border_variant)
                    .size_full()
                    .overflow_hidden()
                    .child(self.prompt.clone())
                    .child(self.editor.clone()),
            )
            .children(self.render_remember(cx))
            .children(self.render_hint(cx))
    }
}
