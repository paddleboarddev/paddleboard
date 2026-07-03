// PaddleBoard: "Set Sail" — deploy the current project to a serverless
// platform. Phase 1: quick deploy to Google Cloud Run, powered by the
// s8sskills catalog (s8sskills.com): the modal installs the provider skill
// pack into the project's `.agents/skills/`, then seeds a native-agent thread
// that follows the skills to perform the deploy. The vendor-neutral layer is
// the skill catalog, not a Rust abstraction — adding a platform is publishing
// a skill pack, not writing code here.

use agent_ui::AgentPanel;
use anyhow::{Context as _, Result, anyhow};
use futures::AsyncReadExt as _;
use gpui::{App, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, WeakEntity};
use http_client::{AsyncBody, HttpClientWithUrl};
use std::path::PathBuf;
use std::sync::Arc;
use ui::{Button, Checkbox, KeyBinding, Modal, ModalFooter, ModalHeader, ToggleState, prelude::*};
use ui_input::InputField;
use workspace::{ModalView, Workspace};

const DEFAULT_REGION: &str = "us-central1";
/// The skills the quick-deploy prompt depends on, in the order the agent
/// should read them. Fetched from Jay's s8sskills catalog when missing.
const REQUIRED_SKILLS: &[&str] = &["gcloud-project-setup", "cloud-run-deploy"];
const S8SSKILLS_RAW_BASE: &str = "https://raw.githubusercontent.com/s8sskills/gcp/main/skills";

pub fn init(cx: &mut App) {
    cx.observe_new(|workspace: &mut Workspace, _window, _cx| {
        workspace.register_action(
            |workspace, _: &paddleboard_actions::set_sail::Deploy, window, cx| {
                // Derive the default service name HERE, where we already hold
                // the Workspace lease — the closure passed to `toggle_modal`
                // runs inside `Workspace::update`, so reading the workspace
                // from the modal constructor double-leases and panics.
                let default_service = {
                    let project = workspace.project().read(cx);
                    project
                        .visible_worktrees(cx)
                        .next()
                        .map(|worktree| slugify(worktree.read(cx).root_name_str()))
                }
                .filter(|slug| !slug.is_empty())
                .unwrap_or_else(|| "my-service".to_string());
                let weak = workspace.weak_handle();
                workspace.toggle_modal(window, cx, |window, cx| {
                    SetSailModal::new(weak, default_service, window, cx)
                });
            },
        );
    })
    .detach();
}

/// "Set Sail" — collects a service name/region and hands the deploy to the
/// native agent, guided by the s8sskills Cloud Run skills.
pub struct SetSailModal {
    workspace: WeakEntity<Workspace>,
    service_input: Entity<InputField>,
    region_input: Entity<InputField>,
    allow_unauthenticated: bool,
    default_service: String,
    focus_handle: FocusHandle,
}

impl SetSailModal {
    pub fn new(
        workspace: WeakEntity<Workspace>,
        default_service: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let service_input = cx.new(|cx| {
            InputField::new(window, cx, &default_service)
                .label("Service name")
                .tab_index(1)
                .tab_stop(true)
        });
        let region_input = cx.new(|cx| {
            InputField::new(window, cx, DEFAULT_REGION)
                .label("Region")
                .tab_index(2)
                .tab_stop(true)
        });

        Self {
            workspace,
            service_input,
            region_input,
            allow_unauthenticated: true,
            default_service,
            focus_handle: cx.focus_handle(),
        }
    }

    fn confirm(&mut self, _: &menu::Confirm, window: &mut Window, cx: &mut Context<Self>) {
        let mut service = self.service_input.read(cx).text(cx).trim().to_string();
        if service.is_empty() {
            service = self.default_service.clone();
        }
        let mut region = self.region_input.read(cx).text(cx).trim().to_string();
        if region.is_empty() {
            region = DEFAULT_REGION.to_string();
        }

        if !is_valid_service_name(&service) {
            self.show_error(
                anyhow!(
                    "Invalid service name {service:?}: use lowercase letters, digits, and \
                     hyphens, starting with a letter (Cloud Run naming rules)."
                ),
                cx,
            );
            return;
        }
        if !region.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            self.show_error(anyhow!("Invalid region {region:?}."), cx);
            return;
        }

        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let (fs, http_client, project_root) = {
            let project = workspace.read(cx).project().read(cx);
            let Some(worktree) = project.visible_worktrees(cx).next() else {
                self.show_error(
                    anyhow!("Open a project folder before setting sail — there is nothing to deploy."),
                    cx,
                );
                return;
            };
            (
                project.fs().clone(),
                project.client().http_client(),
                worktree.read(cx).abs_path().to_path_buf(),
            )
        };

        let allow_unauthenticated = self.allow_unauthenticated;
        let weak_workspace = self.workspace.clone();
        cx.spawn_in(window, async move |_this, cx| {
            // Install the s8sskills pack pieces the prompt depends on. Files
            // already present are left untouched (version-pinned by commit).
            let mut installed = Vec::new();
            for skill in REQUIRED_SKILLS {
                match ensure_skill_installed(fs.clone(), http_client.clone(), &project_root, skill)
                    .await
                {
                    Ok(freshly_fetched) => installed.push((*skill, freshly_fetched)),
                    Err(error) => {
                        log::error!("set_sail: failed to install skill {skill}: {error:#}");
                        weak_workspace
                            .update(cx, |workspace, cx| {
                                workspace.show_error(
                                    anyhow!(
                                        "Set Sail needs the s8sskills `{skill}` skill but \
                                         couldn't fetch it (offline?): {error}"
                                    ),
                                    cx,
                                );
                            })
                            .ok();
                        return;
                    }
                }
            }
            let fetched_count = installed.iter().filter(|(_, fresh)| *fresh).count();
            if fetched_count > 0 {
                log::info!(
                    "set_sail: installed {fetched_count} s8sskills skill(s) into {}",
                    project_root.join(".agents/skills").display()
                );
            }

            let prompt = quick_deploy_prompt(&service, &region, allow_unauthenticated);
            let title: SharedString = format!("Set Sail: {service} → Cloud Run").into();

            weak_workspace
                .update_in(cx, |workspace, window, cx| {
                    let Some(panel) = workspace.panel::<AgentPanel>(cx) else {
                        workspace.show_error(
                            anyhow!("Open the Agent panel to set sail."),
                            cx,
                        );
                        return;
                    };
                    workspace.focus_panel::<AgentPanel>(window, cx);
                    panel.update(cx, |panel, cx| {
                        // Force the native agent: the deploy relies on the
                        // terminal/sandbox tools and the seeded skills, which
                        // external agents can't be assumed to honor.
                        panel.seed_prompt_thread(title.clone(), prompt.clone(), true, window, cx);
                    });
                })
                .ok();
        })
        .detach();

        cx.emit(DismissEvent);
    }

    fn cancel(&mut self, _: &menu::Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }

    fn show_error(&self, error: anyhow::Error, cx: &mut Context<Self>) {
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, |workspace, cx| workspace.show_error(error, cx));
        }
    }
}

fn slugify(name: &str) -> String {
    let mut slug: String = name
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    let slug = slug.trim_matches('-');
    // Cloud Run names must start with a letter.
    let slug = slug.trim_start_matches(|c: char| c.is_ascii_digit() || c == '-');
    slug.chars().take(63).collect()
}

fn is_valid_service_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && name.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && !name.ends_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Fetch one s8sskills skill into `<project>/.agents/skills/<name>/SKILL.md`
/// unless it is already installed. Returns whether a fetch happened.
async fn ensure_skill_installed(
    fs: Arc<dyn fs::Fs>,
    http_client: Arc<HttpClientWithUrl>,
    project_root: &PathBuf,
    skill: &str,
) -> Result<bool> {
    let skill_dir = project_root.join(".agents").join("skills").join(skill);
    let skill_file = skill_dir.join("SKILL.md");
    if fs.is_file(&skill_file).await {
        return Ok(false);
    }

    let url = format!("{S8SSKILLS_RAW_BASE}/{skill}/SKILL.md");
    let mut response = http_client
        .get(&url, AsyncBody::default(), true)
        .await
        .with_context(|| format!("fetching {url}"))?;
    if !response.status().is_success() {
        return Err(anyhow!("{url} returned HTTP {}", response.status()));
    }
    let mut body = Vec::new();
    response
        .body_mut()
        .read_to_end(&mut body)
        .await
        .with_context(|| format!("reading {url}"))?;
    let text = String::from_utf8(body).with_context(|| format!("{url} was not UTF-8"))?;
    if !text.starts_with("---") {
        return Err(anyhow!(
            "{url} does not look like a SKILL.md (missing frontmatter)"
        ));
    }

    fs.create_dir(&skill_dir)
        .await
        .with_context(|| format!("creating {}", skill_dir.display()))?;
    fs.write(&skill_file, text.as_bytes())
        .await
        .with_context(|| format!("writing {}", skill_file.display()))?;
    Ok(true)
}

/// Self-contained quick-deploy prompt. Points the agent at the installed
/// skill files by path rather than relying on the skill catalog having
/// refreshed mid-session.
fn quick_deploy_prompt(service: &str, region: &str, allow_unauthenticated: bool) -> String {
    let visibility = if allow_unauthenticated {
        "The service should be PUBLIC: pass `--allow-unauthenticated`."
    } else {
        "The service should require authentication: do NOT pass `--allow-unauthenticated`."
    };
    format!(
        "Set Sail: deploy this project to Google Cloud Run (quick deploy).\n\n\
         Service name: {service}\n\
         Region: {region}\n\
         {visibility}\n\n\
         Two s8sskills skill files are installed in this project. Read BOTH first — they are \
         the authoritative playbook:\n\
         - .agents/skills/gcloud-project-setup/SKILL.md\n\
         - .agents/skills/cloud-run-deploy/SKILL.md\n\n\
         Steps:\n\
         1. Read the two skill files above and follow them wherever they are more specific \
         than these steps.\n\
         2. Check prerequisites in the terminal: `gcloud --version`, an authenticated account \
         (`gcloud auth list`), and an active project (`gcloud config get-value project`). If \
         any are missing, STOP and tell me the exact command to run myself in a terminal \
         (e.g. `gcloud auth login`) — never run interactive auth flows yourself — then wait \
         for me to confirm before continuing.\n\
         3. Identify the project type (look at the files present) and make sure it is \
         deployable from source; fix only what blocks the deploy.\n\
         4. Deploy from source: `gcloud run deploy {service} --source . --region {region}`, \
         plus the visibility flag above, unless the skills prescribe something better for \
         this project type.\n\
         5. On success, report the service URL, the gcloud project used, and what got created \
         (Artifact Registry repo, Cloud Run service).\n\
         6. On failure, diagnose from the error output and retry sensibly (a few attempts at \
         most), keeping me informed.\n\n\
         This is a one-off quick deploy — do NOT set up CI/CD pipelines or create git \
         infrastructure in this run."
    )
}

impl EventEmitter<DismissEvent> for SetSailModal {}

impl Focusable for SetSailModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ModalView for SetSailModal {}

impl Render for SetSailModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus_handle = self.focus_handle(cx);

        v_flex()
            .id("set-sail-modal")
            .key_context("SetSailModal")
            .w(rems(34.))
            .elevation_3(cx)
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::cancel))
            .capture_any_mouse_down(cx.listener(|this, _, window, cx| {
                this.focus_handle(cx).focus(window, cx);
            }))
            .child(
                Modal::new("set-sail", None)
                    .header(
                        ModalHeader::new().headline("Set Sail ⛵").description(
                            "Quick-deploy this project to Google Cloud Run. PaddleBoard's agent \
                             follows the s8sskills playbook: it checks your gcloud setup, then \
                             deploys straight from source. More platforms (Lambda, Vercel, \
                             Cloudflare…) are on the horizon.",
                        ),
                    )
                    .child(
                        v_flex()
                            .px_3()
                            .pb_2()
                            .gap_2()
                            .child(self.service_input.clone())
                            .child(self.region_input.clone())
                            .child(
                                Checkbox::new(
                                    "set-sail-public",
                                    if self.allow_unauthenticated {
                                        ToggleState::Selected
                                    } else {
                                        ToggleState::Unselected
                                    },
                                )
                                .label("Public URL (--allow-unauthenticated)")
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.allow_unauthenticated = !this.allow_unauthenticated;
                                    cx.notify();
                                })),
                            ),
                    )
                    .footer(
                        ModalFooter::new().end_slot(
                            h_flex()
                                .gap_1()
                                .child(
                                    Button::new("cancel", "Cancel")
                                        .key_binding(
                                            KeyBinding::for_action_in(
                                                &menu::Cancel,
                                                &focus_handle,
                                                cx,
                                            )
                                            .map(|kb| kb.size(rems_from_px(12.))),
                                        )
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.cancel(&menu::Cancel, window, cx);
                                        })),
                                )
                                .child(
                                    Button::new("set-sail-confirm", "Set Sail")
                                        .style(ButtonStyle::Filled)
                                        .key_binding(
                                            KeyBinding::for_action_in(
                                                &menu::Confirm,
                                                &focus_handle,
                                                cx,
                                            )
                                            .map(|kb| kb.size(rems_from_px(12.))),
                                        )
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.confirm(&menu::Confirm, window, cx);
                                        })),
                                ),
                        ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_produces_valid_service_names() {
        assert_eq!(slugify("PaddleBoard"), "paddleboard");
        assert_eq!(slugify("My Cool App!!"), "my-cool-app");
        assert_eq!(slugify("123-starts-with-digit"), "starts-with-digit");
        assert!(is_valid_service_name(&slugify("Persona Test_Project")));
    }

    #[test]
    fn service_name_validation_follows_cloud_run_rules() {
        assert!(is_valid_service_name("my-service"));
        assert!(!is_valid_service_name("My-Service"));
        assert!(!is_valid_service_name("9lives"));
        assert!(!is_valid_service_name("trailing-"));
        assert!(!is_valid_service_name(""));
    }

    #[test]
    fn prompt_carries_service_region_and_visibility() {
        let prompt = quick_deploy_prompt("boaty", "europe-west1", true);
        assert!(prompt.contains("gcloud run deploy boaty --source . --region europe-west1"));
        assert!(prompt.contains("--allow-unauthenticated"));
        assert!(prompt.contains("cloud-run-deploy/SKILL.md"));
        let private_prompt = quick_deploy_prompt("boaty", "us-central1", false);
        assert!(private_prompt.contains("do NOT pass"));
    }
}
