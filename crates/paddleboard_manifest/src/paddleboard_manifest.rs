//! Manifest: a dock panel giving a tree overview of the project's git state —
//! repositories, branches, and stashes (commits and contributors to follow).

use std::collections::HashSet;
use std::ops::Range;

use anyhow::Result;
use git::Oid;
use git::repository::{Branch, Contributor, LogOrder, LogSource};
use git::stash::StashEntry;
use git_ui::commit_view::CommitView;
use gpui::{
    Action, AnyElement, App, AsyncWindowContext, Context, Entity, EventEmitter, FocusHandle,
    Focusable, IntoElement, Pixels, Render, SharedString, Subscription, Task, WeakEntity, Window,
    prelude::*, px, uniform_list,
};
use project::git_store::{
    CommitDataState, GitStore, GitStoreEvent, Repository, RepositoryEvent, RepositoryId,
};
use ui::{ListItem, prelude::*};
use util::ResultExt;
use workspace::{
    Workspace,
    dock::{DockPosition, Panel, PanelEvent},
};

gpui::actions!(manifest, [ToggleFocus]);

const MANIFEST_PANEL_KEY: &str = "ManifestPanel";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Section {
    Repositories,
    Branches,
    Commits,
    Stashes,
    Contributors,
}

impl Section {
    fn title(self) -> &'static str {
        match self {
            Section::Repositories => "Repositories",
            Section::Branches => "Branches",
            Section::Commits => "Commits",
            Section::Stashes => "Stashes",
            Section::Contributors => "Contributors",
        }
    }
}

// A sidebar overview, not the full log — the Git Graph is the surface for deep
// history, and rebuilding an unbounded entry vec on every repo event is wasted
// work on large repos.
const COMMIT_ROW_LIMIT: usize = 250;

enum ManifestEntry {
    Header {
        section: Section,
        count: usize,
    },
    Repository {
        repository: Entity<Repository>,
        name: SharedString,
        branch: Option<SharedString>,
        is_active: bool,
    },
    Branch(Branch),
    Commit {
        repository: WeakEntity<Repository>,
        sha: Oid,
    },
    Stash {
        repository: WeakEntity<Repository>,
        entry: StashEntry,
    },
    Contributor(Contributor),
}

pub struct ManifestPanel {
    focus_handle: FocusHandle,
    position: DockPosition,
    workspace: WeakEntity<Workspace>,
    git_store: Entity<GitStore>,
    collapsed_sections: HashSet<Section>,
    entries: Vec<ManifestEntry>,
    tracked_repository_id: Option<RepositoryId>,
    contributors: Vec<Contributor>,
    _subscriptions: Vec<Subscription>,
    _repository_subscriptions: Vec<Subscription>,
    _contributors_task: Option<Task<()>>,
}

impl ManifestPanel {
    fn new(workspace: &Workspace, cx: &mut Context<Self>) -> Self {
        let git_store = workspace.project().read(cx).git_store().clone();

        let subscription = cx.subscribe(&git_store, |this, _git_store, event, cx| match event {
            GitStoreEvent::RepositoryUpdated(_, RepositoryEvent::HeadChanged, true) => {
                this.fetch_contributors(cx);
                this.update_entries(cx);
            }
            GitStoreEvent::ActiveRepositoryChanged(_)
            | GitStoreEvent::RepositoryUpdated(..)
            | GitStoreEvent::RepositoryAdded
            | GitStoreEvent::RepositoryRemoved(_) => this.update_entries(cx),
            _ => {}
        });

        let mut this = Self {
            focus_handle: cx.focus_handle(),
            position: DockPosition::Left,
            workspace: workspace.weak_handle(),
            git_store,
            collapsed_sections: HashSet::new(),
            entries: Vec::new(),
            tracked_repository_id: None,
            contributors: Vec::new(),
            _subscriptions: vec![subscription],
            _repository_subscriptions: Vec::new(),
            _contributors_task: None,
        };
        this.update_entries(cx);
        this
    }

    pub async fn load(
        workspace: WeakEntity<Workspace>,
        mut cx: AsyncWindowContext,
    ) -> Result<Entity<Self>> {
        workspace.update(&mut cx, |workspace, cx| cx.new(|cx| Self::new(workspace, cx)))
    }

    fn update_entries(&mut self, cx: &mut Context<Self>) {
        let active_repository = self.git_store.read(cx).active_repository();
        let active_id = active_repository
            .as_ref()
            .map(|repository| repository.read(cx).id);

        // The commit log streams in chunks and its per-commit metadata loads
        // lazily; both signal through the repository entity, so follow the
        // active repository directly.
        if self.tracked_repository_id != active_id {
            self.tracked_repository_id = active_id;
            self._repository_subscriptions.clear();
            if let Some(repository) = active_repository.as_ref() {
                self._repository_subscriptions.push(cx.subscribe(
                    repository,
                    |this, _repository, event: &RepositoryEvent, cx| {
                        if matches!(event, RepositoryEvent::GraphEvent(..)) {
                            this.update_entries(cx);
                        }
                    },
                ));
                self._repository_subscriptions
                    .push(cx.observe(repository, |_this, _repository, cx| cx.notify()));
            }
            self.fetch_contributors(cx);
        }

        let mut repositories: Vec<Entity<Repository>> = self
            .git_store
            .read(cx)
            .repositories()
            .values()
            .cloned()
            .collect();
        repositories.sort_by_key(|repository| repository.read(cx).display_name());

        let repository_rows: Vec<ManifestEntry> = repositories
            .iter()
            .map(|repository| {
                let snapshot = repository.read(cx);
                ManifestEntry::Repository {
                    repository: repository.clone(),
                    name: snapshot.display_name(),
                    branch: snapshot
                        .branch
                        .as_ref()
                        .map(|branch| SharedString::from(branch.name().to_string())),
                    is_active: Some(snapshot.id) == active_id,
                }
            })
            .collect();

        let mut branches: Vec<Branch> = active_repository
            .as_ref()
            .map(|repository| repository.read(cx).branch_list.to_vec())
            .unwrap_or_default();
        branches.sort_by(|a, b| {
            b.is_head
                .cmp(&a.is_head)
                .then(a.is_remote().cmp(&b.is_remote()))
                .then(b.priority_key().1.cmp(&a.priority_key().1))
        });

        let commit_shas: Vec<Oid> = active_repository
            .as_ref()
            .and_then(|repository| {
                let branch_name = repository.read(cx).branch.as_ref()?.name().to_string();
                Some(repository.update(cx, |repository, cx| {
                    let response = repository.graph_data(
                        LogSource::Branch(branch_name.into()),
                        LogOrder::DateOrder,
                        0..COMMIT_ROW_LIMIT,
                        cx,
                    );
                    response
                        .commits
                        .iter()
                        .take(COMMIT_ROW_LIMIT)
                        .map(|commit| commit.sha)
                        .collect()
                }))
            })
            .unwrap_or_default();

        let stash_rows: Vec<ManifestEntry> = active_repository
            .as_ref()
            .map(|repository| {
                let weak_repository = repository.downgrade();
                repository
                    .read(cx)
                    .stash_entries
                    .entries
                    .iter()
                    .cloned()
                    .map(|entry| ManifestEntry::Stash {
                        repository: weak_repository.clone(),
                        entry,
                    })
                    .collect()
            })
            .unwrap_or_default();

        self.entries.clear();

        self.entries.push(ManifestEntry::Header {
            section: Section::Repositories,
            count: repository_rows.len(),
        });
        if !self.collapsed_sections.contains(&Section::Repositories) {
            self.entries.extend(repository_rows);
        }

        self.entries.push(ManifestEntry::Header {
            section: Section::Branches,
            count: branches.len(),
        });
        if !self.collapsed_sections.contains(&Section::Branches) {
            self.entries
                .extend(branches.into_iter().map(ManifestEntry::Branch));
        }

        self.entries.push(ManifestEntry::Header {
            section: Section::Commits,
            count: commit_shas.len(),
        });
        if !self.collapsed_sections.contains(&Section::Commits) {
            if let Some(repository) = active_repository.as_ref() {
                let weak_repository = repository.downgrade();
                self.entries
                    .extend(commit_shas.into_iter().map(|sha| ManifestEntry::Commit {
                        repository: weak_repository.clone(),
                        sha,
                    }));
            }
        }

        self.entries.push(ManifestEntry::Header {
            section: Section::Stashes,
            count: stash_rows.len(),
        });
        if !self.collapsed_sections.contains(&Section::Stashes) {
            self.entries.extend(stash_rows);
        }

        self.entries.push(ManifestEntry::Header {
            section: Section::Contributors,
            count: self.contributors.len(),
        });
        if !self.collapsed_sections.contains(&Section::Contributors) {
            self.entries.extend(
                self.contributors
                    .iter()
                    .cloned()
                    .map(ManifestEntry::Contributor),
            );
        }

        cx.notify();
    }

    fn fetch_contributors(&mut self, cx: &mut Context<Self>) {
        let Some(repository) = self.git_store.read(cx).active_repository() else {
            self.contributors.clear();
            self._contributors_task = None;
            return;
        };
        let receiver = repository.update(cx, |repository, _| repository.contributors());
        self._contributors_task = Some(cx.spawn(async move |this, cx| {
            if let Ok(result) = receiver.await
                && let Some(contributors) = result.log_err()
            {
                this.update(cx, |this, cx| {
                    this.contributors = contributors;
                    this.update_entries(cx);
                })
                .ok();
            }
        }));
    }

    fn toggle_section(&mut self, section: Section, cx: &mut Context<Self>) {
        if !self.collapsed_sections.remove(&section) {
            self.collapsed_sections.insert(section);
        }
        self.update_entries(cx);
    }

    fn activate_repository(&self, repository: &Entity<Repository>, cx: &mut Context<Self>) {
        repository.update(cx, |repository, cx| repository.set_as_active_repository(cx));
    }

    fn open_commit(
        &self,
        repository: WeakEntity<Repository>,
        sha: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        CommitView::open(sha, repository, self.workspace.clone(), None, None, window, cx);
    }

    fn open_stash(
        &self,
        repository: WeakEntity<Repository>,
        entry: &StashEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        CommitView::open(
            entry.oid.to_string(),
            repository,
            self.workspace.clone(),
            Some(entry.index),
            None,
            window,
            cx,
        );
    }

    fn render_entry(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(entry) = self.entries.get(ix) else {
            return gpui::Empty.into_any_element();
        };

        match entry {
            ManifestEntry::Header { section, count } => {
                let section = *section;
                let expanded = !self.collapsed_sections.contains(&section);
                ListItem::new(("manifest-header", ix))
                    .toggle(Some(expanded))
                    .on_toggle(cx.listener(move |this, _, _window, cx| {
                        this.toggle_section(section, cx)
                    }))
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        this.toggle_section(section, cx)
                    }))
                    .child(
                        h_flex()
                            .gap_1()
                            .child(Label::new(section.title()).size(LabelSize::Small))
                            .child(
                                Label::new(count.to_string())
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            ),
                    )
                    .into_any_element()
            }
            ManifestEntry::Repository {
                repository,
                name,
                branch,
                is_active,
            } => {
                let repository = repository.clone();
                let is_active = *is_active;
                ListItem::new(("manifest-repo", ix))
                    .indent_level(1)
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        this.activate_repository(&repository, cx)
                    }))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Icon::new(IconName::Sailboat)
                                    .size(IconSize::Small)
                                    .color(if is_active {
                                        Color::Accent
                                    } else {
                                        Color::Muted
                                    }),
                            )
                            .child(Label::new(name.clone()).size(LabelSize::Small))
                            .when_some(branch.clone(), |this, branch| {
                                this.child(
                                    Label::new(branch)
                                        .size(LabelSize::XSmall)
                                        .color(Color::Muted),
                                )
                            })
                            .when(is_active, |this| {
                                this.child(
                                    Icon::new(IconName::Check)
                                        .size(IconSize::XSmall)
                                        .color(Color::Success),
                                )
                            }),
                    )
                    .into_any_element()
            }
            ManifestEntry::Branch(branch) => {
                let name = SharedString::from(branch.name().to_string());
                let tracking = branch.tracking_status();
                let is_head = branch.is_head;
                let is_remote = branch.is_remote();
                ListItem::new(("manifest-branch", ix))
                    .indent_level(1)
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Icon::new(IconName::GitBranch)
                                    .size(IconSize::Small)
                                    .color(if is_head { Color::Accent } else { Color::Muted }),
                            )
                            .child(Label::new(name).size(LabelSize::Small).color(
                                if is_remote { Color::Muted } else { Color::Default },
                            ))
                            .when_some(tracking, |this, tracking| {
                                this.when(tracking.ahead > 0, |this| {
                                    this.child(
                                        Label::new(format!("↑{}", tracking.ahead))
                                            .size(LabelSize::XSmall)
                                            .color(Color::Muted),
                                    )
                                })
                                .when(tracking.behind > 0, |this| {
                                    this.child(
                                        Label::new(format!("↓{}", tracking.behind))
                                            .size(LabelSize::XSmall)
                                            .color(Color::Muted),
                                    )
                                })
                            }),
                    )
                    .into_any_element()
            }
            ManifestEntry::Commit { repository, sha } => {
                let sha = *sha;
                let commit_data = repository.upgrade().and_then(|repository| {
                    repository.update(cx, |repository, cx| {
                        match repository.fetch_commit_data(sha, false, cx) {
                            CommitDataState::Loaded(data) => Some(data.clone()),
                            CommitDataState::Loading(_) => None,
                        }
                    })
                });
                let subject: SharedString = commit_data
                    .map(|data| data.subject.clone())
                    .unwrap_or_else(|| "Loading…".into());
                let sha_string = sha.to_string();
                let short_sha: SharedString =
                    sha_string[..7.min(sha_string.len())].to_string().into();
                let repository = repository.clone();
                ListItem::new(("manifest-commit", ix))
                    .indent_level(1)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_commit(repository.clone(), sha_string.clone(), window, cx)
                    }))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Icon::new(IconName::GitCommit)
                                    .size(IconSize::Small)
                                    .color(Color::Muted),
                            )
                            .child(Label::new(subject).size(LabelSize::Small))
                            .child(
                                Label::new(short_sha)
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            ),
                    )
                    .into_any_element()
            }
            ManifestEntry::Stash { repository, entry } => {
                let repository = repository.clone();
                let stash_entry = entry.clone();
                ListItem::new(("manifest-stash", ix))
                    .indent_level(1)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_stash(repository.clone(), &stash_entry, window, cx)
                    }))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Icon::new(IconName::Archive)
                                    .size(IconSize::Small)
                                    .color(Color::Muted),
                            )
                            .child(
                                Label::new(format!("stash@{{{}}}", entry.index))
                                    .size(LabelSize::Small),
                            )
                            .child(
                                Label::new(entry.message.clone())
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            ),
                    )
                    .into_any_element()
            }
            ManifestEntry::Contributor(contributor) => {
                let commit_count_label = if contributor.commit_count == 1 {
                    "1 commit".to_string()
                } else {
                    format!("{} commits", contributor.commit_count)
                };
                ListItem::new(("manifest-contributor", ix))
                    .indent_level(1)
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Icon::new(IconName::Person)
                                    .size(IconSize::Small)
                                    .color(Color::Muted),
                            )
                            .child(Label::new(contributor.name.clone()).size(LabelSize::Small))
                            .child(
                                Label::new(commit_count_label)
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            ),
                    )
                    .into_any_element()
            }
        }
    }
}

impl EventEmitter<PanelEvent> for ManifestPanel {}

impl Focusable for ManifestPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for ManifestPanel {
    fn persistent_name() -> &'static str {
        "ManifestPanel"
    }

    fn panel_key() -> &'static str {
        MANIFEST_PANEL_KEY
    }

    fn position(&self, _window: &Window, _cx: &App) -> DockPosition {
        self.position
    }

    fn position_is_valid(&self, position: DockPosition) -> bool {
        matches!(position, DockPosition::Left | DockPosition::Right)
    }

    fn set_position(
        &mut self,
        position: DockPosition,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.position = position;
        cx.notify();
    }

    fn default_size(&self, _window: &Window, _cx: &App) -> Pixels {
        px(300.0)
    }

    fn icon(&self, _window: &Window, cx: &App) -> Option<IconName> {
        // PaddleBoard: ListTodo (not ListTree) — the orchestration panel already
        // uses ListTree, and two identical dock glyphs are indistinguishable.
        Some(IconName::ListTodo)
            .filter(|_| paddleboard_ui::PaddleboardUiSettings::get(cx).manifest_button)
    }

    fn hide_button_setting(&self, _: &App) -> Option<workspace::HideStatusItem> {
        Some(workspace::HideStatusItem::new(|settings| {
            settings.paddleboard_ui.get_or_insert_default().manifest_button = Some(false);
        }))
    }

    fn icon_tooltip(&self, _window: &Window, _cx: &App) -> Option<&'static str> {
        Some("Manifest")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(ToggleFocus)
    }

    fn activation_priority(&self) -> u32 {
        // PaddleBoard: slot next to the Git panel (3) rather than the end of
        // the rail — Manifest is a git surface.
        4
    }
}

impl Render for ManifestPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let entry_count = self.entries.len();
        let has_repositories = !self.git_store.read(cx).repositories().is_empty();

        v_flex()
            .id("manifest-panel")
            .key_context("ManifestPanel")
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(colors.panel_background)
            .map(|this| {
                if has_repositories {
                    this.child(
                        uniform_list(
                            "manifest-entries",
                            entry_count,
                            cx.processor(|this, range: Range<usize>, _window, cx| {
                                range.map(|ix| this.render_entry(ix, cx)).collect()
                            }),
                        )
                        .flex_grow(1.0),
                    )
                } else {
                    this.child(
                        v_flex().p_3().child(
                            Label::new("No Git repositories in this project.")
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        ),
                    )
                }
            })
    }
}

pub fn init(cx: &mut App) {
    cx.observe_new(
        |workspace: &mut Workspace,
         _window: Option<&mut Window>,
         _cx: &mut Context<Workspace>| {
            workspace.register_action(|workspace, _: &ToggleFocus, window, cx| {
                workspace.toggle_panel_focus::<ManifestPanel>(window, cx);
            });
        },
    )
    .detach();
}
