use anyhow::Result;
use collections::HashMap;
use editor::{CurrentLineHighlight, Editor, EditorElement, EditorStyle, actions::Tab};
use gpui::{
    App, Bounds, DEFAULT_ADDITIONAL_WINDOW_SIZE, Entity, EventEmitter, Focusable, Subscription,
    Task, TextStyle, Tiling, TitlebarOptions, WindowBounds, WindowHandle, WindowOptions, point,
    size,
};
use language::{Buffer, LanguageRegistry, language_settings::SoftWrap};
use language_model::{ConfiguredModel, LanguageModelRegistry};
use picker::{Picker, PickerDelegate};
use platform_title_bar::PlatformTitleBar;
use release_channel::ReleaseChannel;
use settings::{ActionSequence, Settings};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use theme_settings::ThemeSettings;
use ui::{Divider, ListItem, ListItemSpacing, ListSubHeader, Tooltip, prelude::*};
use ui_input::ErasedEditor;
use util::ResultExt;
use workspace::{MultiWorkspace, Workspace, WorkspaceSettings, client_side_decorations};
use paddleboard_actions::assistant::InlineAssist;

use prompt_store::*;

// PaddleBoard: Upstream's "rules → skills" migration trimmed `PromptStore` down to a
// read-only view (`load` + `all_prompt_metadata`), dropping `search`, `metadata`,
// `first`, `save`, `save_metadata`, `delete`, and `PromptId::can_edit`. Rather than
// re-expose a write API on `PromptStore` — an upstream-shaped change that would
// re-conflict on every weekly upstream merge, and that fights upstream's direction —
// PaddleBoard keeps the Rules Library as a *read-only viewer*. Creating and editing
// rules now lives in the file-based Skills system (the AI Dock Skills tab and the
// `skill_creator` flow). The helpers below reimplement the removed *query* methods
// locally against `all_prompt_metadata`, preserving their exact read behavior; all
// former mutation paths (new / save / delete / duplicate / toggle-default) are gone
// so the window no longer silently drops edits that can't be persisted.

/// Local reimplementation of the removed `PromptStore::search`. Mirrors the old
/// behavior: empty query returns every entry, otherwise a fuzzy match over titles,
/// with default rules sorted first.
fn search_metadata(
    store: &PromptStore,
    query: String,
    cancellation_flag: Arc<AtomicBool>,
    cx: &App,
) -> Task<Vec<PromptMetadata>> {
    let cached_metadata = store.all_prompt_metadata();
    let executor = cx.background_executor().clone();
    cx.background_spawn(async move {
        let mut matches = if query.is_empty() {
            cached_metadata
        } else {
            let candidates = cached_metadata
                .iter()
                .enumerate()
                .filter_map(|(index, metadata)| {
                    Some(fuzzy::StringMatchCandidate::new(
                        index,
                        metadata.title.as_ref()?,
                    ))
                })
                .collect::<Vec<_>>();
            let string_matches = fuzzy::match_strings(
                &candidates,
                &query,
                false,
                true,
                100,
                &cancellation_flag,
                executor,
            )
            .await;
            string_matches
                .into_iter()
                .filter_map(|string_match| cached_metadata.get(string_match.candidate_id).cloned())
                .collect()
        };
        matches.sort_by_key(|metadata| std::cmp::Reverse(metadata.default));
        matches
    })
}

/// Local reimplementation of the removed `PromptStore::metadata`.
fn metadata_for(store: &PromptStore, prompt_id: PromptId) -> Option<PromptMetadata> {
    store
        .all_prompt_metadata()
        .into_iter()
        .find(|metadata| metadata.id == prompt_id)
}

pub fn init(cx: &mut App) {
    prompt_store::init(cx);
}

pub trait InlineAssistDelegate {
    fn assist(
        &self,
        prompt_editor: &Entity<Editor>,
        initial_prompt: Option<String>,
        window: &mut Window,
        cx: &mut Context<RulesLibrary>,
    );

    /// Returns whether the Agent panel was focused.
    fn focus_agent_panel(
        &self,
        workspace: &mut Workspace,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> bool;
}

/// This function opens a new rules library window if one doesn't exist already.
/// If one exists, it brings it to the foreground.
///
/// Note that, when opening a new window, this waits for the PromptStore to be
/// initialized. If it was initialized successfully, it returns a window handle
/// to a rules library.
pub fn open_rules_library(
    language_registry: Arc<LanguageRegistry>,
    inline_assist_delegate: Box<dyn InlineAssistDelegate>,
    prompt_to_select: Option<PromptId>,
    cx: &mut App,
) -> Task<Result<WindowHandle<RulesLibrary>>> {
    let store = PromptStore::global(cx);
    cx.spawn(async move |cx| {
        // We query windows in spawn so that all windows have been returned to GPUI
        let existing_window = cx.update(|cx| {
            let existing_window = cx
                .windows()
                .into_iter()
                .find_map(|window| window.downcast::<RulesLibrary>());
            if let Some(existing_window) = existing_window {
                existing_window
                    .update(cx, |rules_library, window, cx| {
                        if let Some(prompt_to_select) = prompt_to_select {
                            rules_library.load_rule(prompt_to_select, true, window, cx);
                        }
                        window.activate_window()
                    })
                    .ok();

                Some(existing_window)
            } else {
                None
            }
        });

        if let Some(existing_window) = existing_window {
            return Ok(existing_window);
        }

        let store = store.await?;
        cx.update(|cx| {
            let app_id = ReleaseChannel::global(cx).app_id();
            let bounds = Bounds::centered(None, size(px(1024.0), px(768.0)), cx);
            let window_decorations = match std::env::var("PADDLEBOARD_WINDOW_DECORATIONS") {
                Ok(val) if val == "server" => gpui::WindowDecorations::Server,
                Ok(val) if val == "client" => gpui::WindowDecorations::Client,
                _ => match WorkspaceSettings::get_global(cx).window_decorations {
                    settings::WindowDecorations::Server => gpui::WindowDecorations::Server,
                    settings::WindowDecorations::Client => gpui::WindowDecorations::Client,
                },
            };
            cx.open_window(
                WindowOptions {
                    titlebar: Some(TitlebarOptions {
                        title: Some("Rules Library".into()),
                        appears_transparent: true,
                        traffic_light_position: Some(point(px(12.0), px(12.0))),
                    }),
                    app_id: Some(app_id.to_owned()),
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_background: cx.theme().window_background_appearance(),
                    window_decorations: Some(window_decorations),
                    window_min_size: Some(DEFAULT_ADDITIONAL_WINDOW_SIZE),
                    kind: gpui::WindowKind::Floating,
                    ..Default::default()
                },
                |window, cx| {
                    cx.new(|cx| {
                        RulesLibrary::new(
                            store,
                            language_registry,
                            inline_assist_delegate,
                            prompt_to_select,
                            window,
                            cx,
                        )
                    })
                },
            )
        })
    })
}

pub struct RulesLibrary {
    title_bar: Option<Entity<PlatformTitleBar>>,
    store: Entity<PromptStore>,
    language_registry: Arc<LanguageRegistry>,
    rule_editors: HashMap<PromptId, RuleEditor>,
    active_rule_id: Option<PromptId>,
    picker: Entity<Picker<RulePickerDelegate>>,
    pending_load: Task<()>,
    inline_assist_delegate: Box<dyn InlineAssistDelegate>,
    _subscriptions: Vec<Subscription>,
}

struct RuleEditor {
    title_editor: Entity<Editor>,
    body_editor: Entity<Editor>,
}

enum RulePickerEntry {
    Header(SharedString),
    Rule(PromptMetadata),
    Separator,
}

struct RulePickerDelegate {
    store: Entity<PromptStore>,
    selected_index: usize,
    filtered_entries: Vec<RulePickerEntry>,
}

// PaddleBoard: the picker is read-only now — it only navigates between rules. The
// former `Deleted` / `ToggledDefault` events were mutation paths and are gone.
enum RulePickerEvent {
    Selected { prompt_id: PromptId },
    Confirmed { prompt_id: PromptId },
}

impl EventEmitter<RulePickerEvent> for Picker<RulePickerDelegate> {}

impl PickerDelegate for RulePickerDelegate {
    type ListItem = AnyElement;

    fn name() -> &'static str {
        "rules library"
    }

    fn match_count(&self) -> usize {
        self.filtered_entries.len()
    }

    fn no_matches_text(&self, _window: &mut Window, _cx: &mut App) -> Option<SharedString> {
        Some("No rules found matching your search.".into())
    }

    fn selected_index(&self) -> usize {
        self.selected_index
    }

    fn set_selected_index(&mut self, ix: usize, _: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.selected_index = ix.min(self.filtered_entries.len().saturating_sub(1));

        if let Some(RulePickerEntry::Rule(rule)) = self.filtered_entries.get(self.selected_index) {
            cx.emit(RulePickerEvent::Selected { prompt_id: rule.id });
        }

        cx.notify();
    }

    fn can_select(&self, ix: usize, _: &mut Window, _: &mut Context<Picker<Self>>) -> bool {
        match self.filtered_entries.get(ix) {
            Some(RulePickerEntry::Rule(_)) => true,
            Some(RulePickerEntry::Header(_)) | Some(RulePickerEntry::Separator) | None => false,
        }
    }

    fn select_on_hover(&self) -> bool {
        false
    }

    fn placeholder_text(&self, _window: &mut Window, _cx: &mut App) -> Arc<str> {
        "Search…".into()
    }

    fn update_matches(
        &mut self,
        query: String,
        window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Task<()> {
        let cancellation_flag = Arc::new(AtomicBool::default());
        let search = search_metadata(self.store.read(cx), query, cancellation_flag, cx);

        let prev_prompt_id = self
            .filtered_entries
            .get(self.selected_index)
            .and_then(|entry| {
                if let RulePickerEntry::Rule(rule) = entry {
                    Some(rule.id)
                } else {
                    None
                }
            });

        cx.spawn_in(window, async move |this, cx| {
            let (filtered_entries, selected_index) = cx
                .background_spawn(async move {
                    let matches = search.await;

                    let (built_in_rules, user_rules): (Vec<_>, Vec<_>) =
                        matches.into_iter().partition(|rule| rule.id.is_built_in());
                    let (default_rules, other_rules): (Vec<_>, Vec<_>) =
                        user_rules.into_iter().partition(|rule| rule.default);

                    let mut filtered_entries = Vec::new();

                    if !built_in_rules.is_empty() {
                        filtered_entries.push(RulePickerEntry::Header("Built-in Rules".into()));

                        for rule in built_in_rules {
                            filtered_entries.push(RulePickerEntry::Rule(rule));
                        }

                        filtered_entries.push(RulePickerEntry::Separator);
                    }

                    if !default_rules.is_empty() {
                        filtered_entries.push(RulePickerEntry::Header("Default Rules".into()));

                        for rule in default_rules {
                            filtered_entries.push(RulePickerEntry::Rule(rule));
                        }

                        filtered_entries.push(RulePickerEntry::Separator);
                    }

                    for rule in other_rules {
                        filtered_entries.push(RulePickerEntry::Rule(rule));
                    }

                    let selected_index = prev_prompt_id
                        .and_then(|prev_prompt_id| {
                            filtered_entries.iter().position(|entry| {
                                if let RulePickerEntry::Rule(rule) = entry {
                                    rule.id == prev_prompt_id
                                } else {
                                    false
                                }
                            })
                        })
                        .unwrap_or_else(|| {
                            filtered_entries
                                .iter()
                                .position(|entry| matches!(entry, RulePickerEntry::Rule(_)))
                                .unwrap_or(0)
                        });

                    (filtered_entries, selected_index)
                })
                .await;

            this.update_in(cx, |this, window, cx| {
                this.delegate.filtered_entries = filtered_entries;
                this.set_selected_index(
                    selected_index,
                    Some(picker::Direction::Down),
                    true,
                    window,
                    cx,
                );
                cx.notify();
            })
            .ok();
        })
    }

    fn confirm(&mut self, _secondary: bool, _: &mut Window, cx: &mut Context<Picker<Self>>) {
        if let Some(RulePickerEntry::Rule(rule)) = self.filtered_entries.get(self.selected_index) {
            cx.emit(RulePickerEvent::Confirmed { prompt_id: rule.id });
        }
    }

    fn dismissed(&mut self, _window: &mut Window, _cx: &mut Context<Picker<Self>>) {}

    fn render_match(
        &self,
        ix: usize,
        selected: bool,
        _: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) -> Option<Self::ListItem> {
        match self.filtered_entries.get(ix)? {
            RulePickerEntry::Header(title) => {
                let tooltip_text = if title.as_ref() == "Built-in Rules" {
                    "Built-in rules are those included out of the box."
                } else {
                    "Default Rules are attached by default with every new thread."
                };

                Some(
                    ListSubHeader::new(title.clone())
                        .end_slot(
                            IconButton::new("info", IconName::Info)
                                .style(ButtonStyle::Transparent)
                                .icon_size(IconSize::Small)
                                .icon_color(Color::Muted)
                                .tooltip(Tooltip::text(tooltip_text))
                                .into_any_element(),
                        )
                        .inset(true)
                        .into_any_element(),
                )
            }
            RulePickerEntry::Separator => Some(
                h_flex()
                    .py_1()
                    .child(Divider::horizontal())
                    .into_any_element(),
            ),
            RulePickerEntry::Rule(rule) => {
                let default = rule.default;
                let prompt_id = rule.id;

                Some(
                    ListItem::new(ix)
                        .inset(true)
                        .spacing(ListItemSpacing::Sparse)
                        .toggle_state(selected)
                        .child(
                            Label::new(rule.title.clone().unwrap_or("Untitled".into()))
                                .truncate()
                                .mr_10(),
                        )
                        // PaddleBoard: a non-interactive marker for default rules. The
                        // former add/remove-default and delete buttons were the
                        // window's mutation paths and are gone now that the store is
                        // read-only.
                        .when(default && !prompt_id.is_built_in(), |this| {
                            this.end_slot(
                                Icon::new(IconName::Paperclip)
                                    .color(Color::Accent)
                                    .size(IconSize::Small),
                            )
                        })
                        .into_any_element(),
                )
            }
        }
    }

    fn render_editor(
        &self,
        editor: &Arc<dyn ErasedEditor>,
        _: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Div {
        let editor = editor.as_any().downcast_ref::<Entity<Editor>>().unwrap();

        h_flex()
            .py_1()
            .px_1p5()
            .mx_1()
            .gap_1p5()
            .rounded_sm()
            .bg(cx.theme().colors().editor_background)
            .border_1()
            .border_color(cx.theme().colors().border)
            .child(Icon::new(IconName::MagnifyingGlass).color(Color::Muted))
            .child(editor.clone())
    }
}

impl RulesLibrary {
    fn new(
        store: Entity<PromptStore>,
        language_registry: Arc<LanguageRegistry>,
        inline_assist_delegate: Box<dyn InlineAssistDelegate>,
        rule_to_select: Option<PromptId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (_selected_index, _matches) = if let Some(rule_to_select) = rule_to_select {
            let matches = store.read(cx).all_prompt_metadata();
            let selected_index = matches
                .iter()
                .enumerate()
                .find(|(_, metadata)| metadata.id == rule_to_select)
                .map_or(0, |(ix, _)| ix);
            (selected_index, matches)
        } else {
            (0, vec![])
        };

        let picker_delegate = RulePickerDelegate {
            store: store.clone(),
            selected_index: 0,
            filtered_entries: Vec::new(),
        };

        let picker = cx.new(|cx| {
            let picker = Picker::list(picker_delegate, window, cx)
                .modal(false);
            picker.focus(window, cx);
            picker
        });

        Self {
            title_bar: if !cfg!(target_os = "macos") {
                Some(cx.new(|cx| PlatformTitleBar::new("rules-library-title-bar", cx)))
            } else {
                None
            },
            store,
            language_registry,
            rule_editors: HashMap::default(),
            active_rule_id: None,
            pending_load: Task::ready(()),
            inline_assist_delegate,
            _subscriptions: vec![cx.subscribe_in(&picker, window, Self::handle_picker_event)],
            picker,
        }
    }

    fn handle_picker_event(
        &mut self,
        _: &Entity<Picker<RulePickerDelegate>>,
        event: &RulePickerEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            RulePickerEvent::Selected { prompt_id } => {
                self.load_rule(*prompt_id, false, window, cx);
            }
            RulePickerEvent::Confirmed { prompt_id } => {
                self.load_rule(*prompt_id, true, window, cx);
            }
        }
    }

    pub fn load_rule(
        &mut self,
        prompt_id: PromptId,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(rule_editor) = self.rule_editors.get(&prompt_id) {
            if focus {
                rule_editor
                    .body_editor
                    .update(cx, |editor, cx| window.focus(&editor.focus_handle(cx), cx));
            }
            self.set_active_rule(Some(prompt_id), window, cx);
        } else if let Some(rule_metadata) = metadata_for(self.store.read(cx), prompt_id) {
            let language_registry = self.language_registry.clone();
            let rule = self.store.read(cx).load(prompt_id, cx);
            self.pending_load = cx.spawn_in(window, async move |this, cx| {
                let rule = rule.await;
                let markdown = language_registry.language_for_name("Markdown").await;
                this.update_in(cx, |this, window, cx| match rule {
                    Ok(rule) => {
                        // PaddleBoard: the Rules Library is a read-only viewer now;
                        // both editors are non-editable so the window can't surface
                        // edits the read-only store would silently drop.
                        let title_editor = cx.new(|cx| {
                            let mut editor = Editor::single_line(window, cx);
                            editor.set_placeholder_text("Untitled", window, cx);
                            editor.set_text(rule_metadata.title.unwrap_or_default(), window, cx);
                            editor.set_read_only(true);
                            editor.set_show_edit_predictions(Some(false), window, cx);
                            editor
                        });
                        let body_editor = cx.new(|cx| {
                            let buffer = cx.new(|cx| {
                                let mut buffer = Buffer::local(rule, cx);
                                buffer.set_language(markdown.log_err(), cx);
                                buffer.set_language_registry(language_registry);
                                buffer
                            });

                            let mut editor = Editor::for_buffer(buffer, None, window, cx);
                            editor.set_read_only(true);
                            editor.set_show_edit_predictions(Some(false), window, cx);
                            editor.set_soft_wrap_mode(SoftWrap::EditorWidth, cx);
                            editor.set_show_gutter(false, cx);
                            editor.set_show_wrap_guides(false, cx);
                            editor.set_show_indent_guides(false, cx);
                            editor.set_use_modal_editing(true);
                            editor.set_current_line_highlight(Some(CurrentLineHighlight::None));
                            if focus {
                                window.focus(&editor.focus_handle(cx), cx);
                            }
                            editor
                        });
                        this.rule_editors.insert(
                            prompt_id,
                            RuleEditor {
                                title_editor,
                                body_editor,
                            },
                        );
                        this.set_active_rule(Some(prompt_id), window, cx);
                    }
                    Err(error) => {
                        // TODO: we should show the error in the UI.
                        log::error!("error while loading rule: {:?}", error);
                    }
                })
                .ok();
            });
        }
    }

    fn set_active_rule(
        &mut self,
        prompt_id: Option<PromptId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.active_rule_id = prompt_id;
        self.picker.update(cx, |picker, cx| {
            if let Some(prompt_id) = prompt_id {
                if picker
                    .delegate
                    .filtered_entries
                    .get(picker.delegate.selected_index())
                    .is_none_or(|old_selected_prompt| {
                        if let RulePickerEntry::Rule(rule) = old_selected_prompt {
                            rule.id != prompt_id
                        } else {
                            true
                        }
                    })
                    && let Some(ix) = picker.delegate.filtered_entries.iter().position(|mat| {
                        if let RulePickerEntry::Rule(rule) = mat {
                            rule.id == prompt_id
                        } else {
                            false
                        }
                    })
                {
                    picker.set_selected_index(ix, None, true, window, cx);
                }
            } else {
                picker.focus(window, cx);
            }
        });
        cx.notify();
    }

    fn focus_active_rule(&mut self, _: &Tab, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(active_rule) = self.active_rule_id {
            self.rule_editors[&active_rule]
                .body_editor
                .update(cx, |editor, cx| window.focus(&editor.focus_handle(cx), cx));
            cx.stop_propagation();
        }
    }

    fn focus_picker(&mut self, _: &menu::Cancel, window: &mut Window, cx: &mut Context<Self>) {
        self.picker
            .update(cx, |picker, cx| picker.focus(window, cx));
    }

    pub fn inline_assist(
        &mut self,
        action: &InlineAssist,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(active_rule_id) = self.active_rule_id else {
            cx.propagate();
            return;
        };

        let rule_editor = &self.rule_editors[&active_rule_id].body_editor;
        let Some(ConfiguredModel { provider, .. }) =
            LanguageModelRegistry::read_global(cx).inline_assistant_model()
        else {
            return;
        };

        let initial_prompt = action.prompt.clone();
        if provider.is_authenticated(cx) {
            self.inline_assist_delegate
                .assist(rule_editor, initial_prompt, window, cx);
        } else {
            for window in cx.windows() {
                if let Some(multi_workspace) = window.downcast::<MultiWorkspace>() {
                    let panel = multi_workspace
                        .update(cx, |multi_workspace, window, cx| {
                            window.activate_window();
                            multi_workspace.workspace().update(cx, |workspace, cx| {
                                self.inline_assist_delegate
                                    .focus_agent_panel(workspace, window, cx)
                            })
                        })
                        .ok();
                    if panel == Some(true) {
                        return;
                    }
                }
            }
        }
    }

    fn move_down_from_title(
        &mut self,
        _: &paddleboard_actions::editor::MoveDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(rule_id) = self.active_rule_id
            && let Some(rule_editor) = self.rule_editors.get(&rule_id)
        {
            window.focus(&rule_editor.body_editor.focus_handle(cx), cx);
        }
    }

    fn move_up_from_body(
        &mut self,
        _: &paddleboard_actions::editor::MoveUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(rule_id) = self.active_rule_id
            && let Some(rule_editor) = self.rule_editors.get(&rule_id)
        {
            window.focus(&rule_editor.title_editor.focus_handle(cx), cx);
        }
    }

    // PaddleBoard: editing moved to the file-based Skills system. This banner points
    // users there instead of leaving them to discover the viewer is read-only.
    fn render_read_only_banner(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .flex_none()
            .items_start()
            .gap_2()
            .px_3()
            .py_2()
            .bg(cx.theme().colors().editor_background)
            .border_b_1()
            .border_color(cx.theme().colors().border)
            .child(
                Icon::new(IconName::Info)
                    .size(IconSize::Small)
                    .color(Color::Muted),
            )
            .child(
                v_flex()
                    .gap_0p5()
                    .child(Label::new("Rules Library is read-only").size(LabelSize::Small))
                    .child(
                        Label::new(
                            "Creating and editing rules has moved to Skills. Open the AI Dock \
                             and switch to the Skills tab to add or edit a skill.",
                        )
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                    ),
            )
    }

    fn render_rule_list(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("rule-list")
            .capture_action(cx.listener(Self::focus_active_rule))
            .px_1p5()
            .h_full()
            .w_64()
            .overflow_x_hidden()
            .bg(cx.theme().colors().panel_background)
            .child(div().flex_grow(1.).child(self.picker.clone()))
    }

    fn render_active_rule_editor(
        &self,
        editor: &Entity<Editor>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let settings = ThemeSettings::get_global(cx);

        div()
            .w_full()
            .pl_1()
            .rounded_sm()
            .on_action(cx.listener(Self::move_down_from_title))
            .child(EditorElement::new(
                &editor,
                EditorStyle {
                    background: cx.theme().system().transparent,
                    local_player: cx.theme().players().local(),
                    text: TextStyle {
                        color: cx.theme().colors().text,
                        font_family: settings.ui_font.family.clone(),
                        font_features: settings.ui_font.features.clone(),
                        font_size: HeadlineSize::Medium.rems().into(),
                        font_weight: settings.ui_font.weight,
                        line_height: relative(settings.buffer_line_height.value()),
                        ..Default::default()
                    },
                    scrollbar_width: Pixels::ZERO,
                    syntax: cx.theme().syntax().clone(),
                    status: cx.theme().status().clone(),
                    inlay_hints_style: editor::make_inlay_hints_style(cx),
                    edit_prediction_styles: editor::make_suggestion_styles(cx),
                    ..EditorStyle::default()
                },
            ))
    }

    fn render_default_indicator(&self) -> impl IntoElement {
        h_flex()
            .gap_1()
            .child(
                Icon::new(IconName::Paperclip)
                    .size(IconSize::Small)
                    .color(Color::Accent),
            )
            .child(
                Label::new("Default Rule")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
    }

    fn render_active_rule(&mut self, cx: &mut Context<RulesLibrary>) -> gpui::Stateful<Div> {
        div()
            .id("rule-editor")
            .h_full()
            .flex_grow(1.)
            .border_l_1()
            .border_color(cx.theme().colors().border)
            .bg(cx.theme().colors().editor_background)
            .children(self.active_rule_id.and_then(|prompt_id| {
                let rule_metadata = metadata_for(self.store.read(cx), prompt_id)?;
                let rule_editor = &self.rule_editors[&prompt_id];
                let focus_handle = rule_editor.body_editor.focus_handle(cx);
                let default = rule_metadata.default;

                Some(
                    v_flex()
                        .id("rule-editor-inner")
                        .size_full()
                        .relative()
                        .overflow_hidden()
                        .on_click(cx.listener(move |_, _, window, cx| {
                            window.focus(&focus_handle, cx);
                        }))
                        .child(
                            h_flex()
                                .group("active-editor-header")
                                .h_12()
                                .px_2()
                                .gap_2()
                                .justify_between()
                                .child(self.render_active_rule_editor(
                                    &rule_editor.title_editor,
                                    cx,
                                ))
                                .child(
                                    h_flex().h_full().flex_shrink_0().when(default, |this| {
                                        this.child(self.render_default_indicator())
                                    }),
                                ),
                        )
                        .child(
                            div()
                                .on_action(cx.listener(Self::focus_picker))
                                .on_action(cx.listener(Self::inline_assist))
                                .on_action(cx.listener(Self::move_up_from_body))
                                .h_full()
                                .flex_grow(1.)
                                .child(
                                    h_flex()
                                        .py_2()
                                        .pl_2p5()
                                        .h_full()
                                        .flex_1()
                                        .child(rule_editor.body_editor.clone()),
                                ),
                        ),
                )
            }))
    }
}

impl Render for RulesLibrary {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui_font = theme_settings::setup_ui_font(window, cx);
        let theme = cx.theme().clone();

        client_side_decorations(
            v_flex()
                .id("rules-library")
                .key_context("RulesLibrary")
                .on_action(
                    |action_sequence: &ActionSequence, window: &mut Window, cx: &mut App| {
                        for action in &action_sequence.0 {
                            window.dispatch_action(action.boxed_clone(), cx);
                        }
                    },
                )
                .size_full()
                .overflow_hidden()
                .font(ui_font)
                .text_color(theme.colors().text)
                .children(self.title_bar.clone())
                // PaddleBoard: on macOS the title bar is a transparent overlay with no
                // `PlatformTitleBar`, so reserve a strip for the traffic lights before
                // any content is drawn.
                .when(cfg!(target_os = "macos"), |this| {
                    this.child(div().h(px(36.)).w_full().flex_none())
                })
                .bg(theme.colors().background)
                .child(self.render_read_only_banner(cx))
                .child(
                    h_flex()
                        .flex_1()
                        .child(self.render_rule_list(cx))
                        .child(self.render_active_rule(cx)),
                ),
            window,
            cx,
            Tiling::default(),
        )
    }
}
