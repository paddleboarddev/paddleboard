//! PaddleBoard: "Manage Git Logins" modal — save a Personal Access Token per
//! git hosting provider so git HTTPS operations authenticate without prompting.
//! Tokens are written to the OS keychain via `paddleboard_git_login`.

use gpui::{DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Render};
use paddleboard_git_login::{KNOWN_PROVIDERS, known_provider};
use ui::{Modal, ModalFooter, ModalHeader, Section, prelude::*};
use ui_input::InputField;
use workspace::{ModalView, Workspace};

pub fn register(workspace: &mut Workspace) {
    workspace.register_action(|workspace, _: &paddleboard_actions::git_login::Manage, window, cx| {
        workspace.toggle_modal(window, cx, |window, cx| GitLoginModal::new(window, cx));
    });
}

pub struct GitLoginModal {
    focus_handle: FocusHandle,
    host: Entity<InputField>,
    username: Entity<InputField>,
    token: Entity<InputField>,
    status: Option<SharedString>,
}

impl GitLoginModal {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let default = &KNOWN_PROVIDERS[0];
        let host = cx.new(|cx| {
            let field = InputField::new(window, cx, "https://github.com")
                .label("Host")
                .tab_index(0)
                .tab_stop(true);
            field.set_text(default.url, window, cx);
            field
        });
        let username = cx.new(|cx| {
            let field = InputField::new(window, cx, "username")
                .label("Username")
                .tab_index(1)
                .tab_stop(true);
            field.set_text(default.token_username, window, cx);
            field
        });
        let token = cx.new(|cx| {
            InputField::new(window, cx, "Personal access token")
                .label("Token")
                .tab_index(2)
                .tab_stop(true)
                .masked(true)
        });

        let this = Self {
            focus_handle: cx.focus_handle(),
            host,
            username,
            token,
            status: None,
        };
        this.reload_status(cx);
        this
    }

    /// Prefill the form for a known provider when its button is clicked.
    fn select_provider(&mut self, host: &str, token_username: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.host.update(cx, |field, cx| field.set_text(host, window, cx));
        self.username
            .update(cx, |field, cx| field.set_text(token_username, window, cx));
        self.token.update(cx, |field, cx| field.set_text("", window, cx));
        self.reload_status(cx);
    }

    /// Look up whether a login already exists for the current host and reflect it
    /// in the status line + username field (the token is never redisplayed).
    fn reload_status(&self, cx: &mut Context<Self>) {
        let host = self.host.read(cx).text(cx);
        let provider = paddleboard_credentials_provider::global(cx);
        cx.spawn(async move |this, cx| {
            let login = paddleboard_git_login::load(&host, provider.as_ref(), cx)
                .await
                .ok()
                .flatten();
            this.update(cx, |this, cx| {
                this.status = Some(match &login {
                    Some(login) if login.from_env => {
                        format!("Signed in as {} (from environment variable)", login.username).into()
                    }
                    Some(login) => format!("Signed in as {}", login.username).into(),
                    None => "Not signed in".into(),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn save(&mut self, _: &menu::Confirm, _window: &mut Window, cx: &mut Context<Self>) {
        let host = self.host.read(cx).text(cx);
        let username = self.username.read(cx).text(cx);
        let token = self.token.read(cx).text(cx);
        if host.is_empty() || username.is_empty() {
            self.status = Some("Host and username are required".into());
            cx.notify();
            return;
        }
        if token.is_empty() {
            self.status = Some("Enter a token (or use Remove to delete a saved login)".into());
            cx.notify();
            return;
        }
        let provider = paddleboard_credentials_provider::global(cx);
        cx.spawn(async move |this, cx| {
            let result =
                paddleboard_git_login::save(&host, &username, &token, provider.as_ref(), cx).await;
            this.update(cx, |this, cx| {
                this.status = Some(match result {
                    Ok(()) => "Saved to keychain".into(),
                    Err(err) => format!("Failed to save: {err}").into(),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn remove(&mut self, cx: &mut Context<Self>) {
        let host = self.host.read(cx).text(cx);
        let provider = paddleboard_credentials_provider::global(cx);
        cx.spawn(async move |this, cx| {
            let result = paddleboard_git_login::delete(&host, provider.as_ref(), cx).await;
            this.update(cx, |this, cx| {
                this.status = Some(match result {
                    Ok(()) => "Removed".into(),
                    Err(err) => format!("Failed to remove: {err}").into(),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn cancel(&mut self, _: &menu::Cancel, _window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }
}

impl Focusable for GitLoginModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<DismissEvent> for GitLoginModal {}

impl ModalView for GitLoginModal {}

impl Render for GitLoginModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let host = self.host.read(cx).text(cx);
        let matched = known_provider(&host);

        let provider_buttons = h_flex().gap_1().children(KNOWN_PROVIDERS.iter().map(|provider| {
            let selected = matched.map(|m| m.url) == Some(provider.url);
            Button::new(SharedString::from(provider.name), provider.name)
                .toggle_state(selected)
                .on_click(cx.listener({
                    let host = provider.url;
                    let username = provider.token_username;
                    move |this, _, window, cx| this.select_provider(host, username, window, cx)
                }))
        }));

        v_flex()
            .key_context("GitLoginModal")
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::cancel))
            .track_focus(&self.focus_handle)
            .w(rems(34.))
            .elevation_3(cx)
            .child(
                Modal::new("git-login", None)
                    .header(ModalHeader::new().headline("Git Logins").description(
                        "Save a Personal Access Token per provider so git authenticates over HTTPS without prompting. Tokens are stored in your OS keychain.",
                    ))
                    .section(
                        Section::new()
                            .child(provider_buttons)
                            .child(self.host.clone())
                            .child(self.username.clone())
                            .child(self.token.clone())
                            .when_some(matched, |this, provider| {
                                this.child(
                                    h_flex()
                                        .gap_1()
                                        .child(
                                            Button::new("create-token", "Create a token →")
                                                .on_click({
                                                    let url = provider.token_url;
                                                    move |_, _, cx| cx.open_url(url)
                                                }),
                                        )
                                        .child(
                                            Label::new(format!("Scopes: {}", provider.scopes_hint))
                                                .size(LabelSize::Small)
                                                .color(Color::Muted),
                                        ),
                                )
                            })
                            .when_some(self.status.clone(), |this, status| {
                                this.child(Label::new(status).size(LabelSize::Small).color(Color::Muted))
                            }),
                    )
                    .footer(
                        ModalFooter::new()
                            .start_slot(
                                Button::new("remove", "Remove")
                                    .on_click(cx.listener(|this, _, _, cx| this.remove(cx))),
                            )
                            .end_slot(
                                h_flex()
                                    .gap_1()
                                    .child(Button::new("cancel", "Cancel").on_click(cx.listener(
                                        |_, _, _, cx| cx.emit(DismissEvent),
                                    )))
                                    .child(
                                        Button::new("save", "Save")
                                            .style(ButtonStyle::Filled)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.save(&menu::Confirm, window, cx)
                                            })),
                                    ),
                            ),
                    ),
            )
    }
}
