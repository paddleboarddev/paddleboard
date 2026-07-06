// PaddleBoard: "Set Sail" — deploy the current project to a serverless
// platform, powered by the s8sskills catalog (s8sskills.com): the modal
// installs the provider skill pack into the project's `.agents/skills/`, then
// seeds a native-agent thread that follows the skills to perform the deploy.
// The vendor-neutral layer is the skill catalog, not a Rust abstraction —
// adding a platform below is a matter of listing its s8sskills pack and CLI
// checks, and most future platforms should arrive as packs alone.

use agent_ui::AgentPanel;
use anyhow::{Context as _, Result, anyhow};
use futures::AsyncReadExt as _;
use gpui::{App, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, WeakEntity};
use http_client::{AsyncBody, HttpClientWithUrl};
use std::path::PathBuf;
use std::sync::Arc;
use ui::{
    Button, Checkbox, KeyBinding, Modal, ModalFooter, ModalHeader, ToggleButtonGroup,
    ToggleButtonGroupStyle, ToggleButtonSimple, ToggleState, prelude::*,
};
use gpui::Action as _;
use ui_input::InputField;
use workspace::{ModalView, StatusItemView, Workspace};

/// The deploy targets Set Sail knows how to skipper. Each maps to an
/// s8sskills provider repo plus the platform-specific prompt scaffolding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    CloudRun,
    AwsLambda,
    Vercel,
}

pub const PLATFORMS: &[Platform] = &[Platform::CloudRun, Platform::AwsLambda, Platform::Vercel];

/// Set Sail's two modes. Quick deploy pushes the current source live once; Rig
/// the pipeline sets up the vendor-agnostic cloud-side rigging (deploy identity
/// + resource + deploy command) so the user can wire up any CI/CD tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    QuickDeploy,
    RigPipeline,
}

impl Mode {
    fn label(&self) -> &'static str {
        match self {
            Mode::QuickDeploy => "Quick deploy",
            Mode::RigPipeline => "Rig the pipeline",
        }
    }
}

pub const MODES: &[Mode] = &[Mode::QuickDeploy, Mode::RigPipeline];

impl Platform {
    pub fn label(&self) -> &'static str {
        match self {
            Platform::CloudRun => "Cloud Run",
            Platform::AwsLambda => "AWS Lambda",
            Platform::Vercel => "Vercel",
        }
    }

    /// The s8sskills provider repo (github.com/s8sskills/<repo>).
    fn s8s_repo(&self) -> &'static str {
        match self {
            Platform::CloudRun => "gcp",
            Platform::AwsLambda => "aws",
            Platform::Vercel => "vercel",
        }
    }

    /// Skills the deploy prompt depends on, in the order the agent should
    /// read them (setup first, then deploy).
    fn required_skills(&self) -> &'static [&'static str] {
        match self {
            Platform::CloudRun => &["gcloud-project-setup", "cloud-run-deploy"],
            Platform::AwsLambda => &["aws-project-setup", "lambda-deploy"],
            Platform::Vercel => &["vercel-project-setup", "vercel-deploy"],
        }
    }

    /// Skills the "Rig the pipeline" prompt depends on (setup first, then the
    /// CI/CD rigging pack). Mirrors `required_skills` but swaps the one-shot
    /// deploy pack for the platform's `<p>-pipeline` pack.
    fn pipeline_skills(&self) -> &'static [&'static str] {
        match self {
            Platform::CloudRun => &["gcloud-project-setup", "cloud-run-pipeline"],
            Platform::AwsLambda => &["aws-project-setup", "lambda-pipeline"],
            Platform::Vercel => &["vercel-project-setup", "vercel-pipeline"],
        }
    }

    /// Whether the platform's s8sskills `<p>-pipeline` pack exists yet. All three
    /// launch platforms now have one; a future platform added without a pipeline
    /// pack would return false here so the modal shows a "coming soon" note.
    fn pipeline_ready(&self) -> bool {
        matches!(
            self,
            Platform::CloudRun | Platform::AwsLambda | Platform::Vercel
        )
    }

    /// Default region, or None when the platform doesn't take one (Vercel
    /// manages placement itself).
    fn default_region(&self) -> Option<&'static str> {
        match self {
            Platform::CloudRun => Some("us-central1"),
            Platform::AwsLambda => Some("us-east-1"),
            Platform::Vercel => None,
        }
    }

    /// Whether the public/private toggle applies. Vercel production deploys
    /// are public by nature, so the toggle is hidden there.
    fn supports_public_toggle(&self) -> bool {
        !matches!(self, Platform::Vercel)
    }

    /// Vendor name for user-facing copy ("Not set up on X?").
    fn vendor_name(&self) -> &'static str {
        match self {
            Platform::CloudRun => "Google Cloud",
            Platform::AwsLambda => "AWS",
            Platform::Vercel => "Vercel",
        }
    }

    /// Where to send someone who doesn't have a vendor account yet.
    fn getting_started_url(&self) -> &'static str {
        match self {
            Platform::CloudRun => "https://cloud.google.com/run",
            Platform::AwsLambda => "https://aws.amazon.com/lambda/getting-started/",
            Platform::Vercel => "https://vercel.com/signup",
        }
    }

    /// Terminal checks the agent should run before deploying, and the exact
    /// command to hand the user when auth is missing.
    fn prereq_checks(&self) -> &'static str {
        match self {
            Platform::CloudRun => {
                "`gcloud --version`, an authenticated account (`gcloud auth list`), and an \
                 active project (`gcloud config get-value project`). If auth is missing, hand \
                 me `gcloud auth login`"
            }
            Platform::AwsLambda => {
                "`aws --version` and a working identity (`aws sts get-caller-identity`). If \
                 auth is missing, hand me `aws configure` (or `aws sso login` if this machine \
                 uses SSO)"
            }
            Platform::Vercel => {
                "`vercel --version` and an authenticated session (`vercel whoami`). If auth \
                 is missing, hand me `vercel login`"
            }
        }
    }

    /// The default deploy move, subordinate to whatever the skills prescribe.
    fn deploy_hint(&self, service: &str, region: &str, public: bool) -> String {
        match self {
            Platform::CloudRun => {
                let visibility = if public {
                    " --allow-unauthenticated"
                } else {
                    ""
                };
                format!(
                    "Deploy from source: `gcloud run deploy {service} --source . --region \
                     {region}{visibility}`"
                )
            }
            Platform::AwsLambda => {
                let url_auth = if public {
                    "create a public Function URL (auth type NONE)"
                } else {
                    "create a Function URL with auth type AWS_IAM (private)"
                };
                format!(
                    "Follow the lambda-deploy skill to pick the packaging that fits this \
                     project (zip, container image, or SAM). Name the function `{service}`, \
                     use region `{region}`, and {url_auth} so there is a URL to report"
                )
            }
            Platform::Vercel => format!(
                "Link the project as `{service}` per the vercel-project-setup skill, then \
                 deploy to production: `vercel deploy --prod --yes`"
            ),
        }
    }
}

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

/// Status-bar sailboat: one click opens the Set Sail modal, so the deploy
/// path doesn't hide behind the command palette.
pub struct SetSailStatusItem;

impl SetSailStatusItem {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for SetSailStatusItem {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        IconButton::new("set-sail-status", IconName::Sailboat)
            .icon_size(IconSize::Small)
            .tooltip(ui::Tooltip::text("Set Sail: deploy this project"))
            .on_click(|_, window, cx| {
                window.dispatch_action(
                    paddleboard_actions::set_sail::Deploy.boxed_clone(),
                    cx,
                );
            })
    }
}

impl StatusItemView for SetSailStatusItem {
    fn set_active_pane_item(
        &mut self,
        _active_pane_item: Option<&dyn workspace::ItemHandle>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    fn hide_setting(&self, _cx: &App) -> Option<workspace::HideStatusItem> {
        None
    }
}

/// "Set Sail" — picks a platform and service details, then hands the deploy
/// to the native agent, guided by the platform's s8sskills pack.
pub struct SetSailModal {
    workspace: WeakEntity<Workspace>,
    mode: Mode,
    platform: Platform,
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
            InputField::new(window, cx, Platform::CloudRun.default_region().unwrap_or(""))
                .label("Region")
                .tab_index(2)
                .tab_stop(true)
        });

        Self {
            workspace,
            mode: Mode::QuickDeploy,
            platform: Platform::CloudRun,
            service_input,
            region_input,
            allow_unauthenticated: true,
            default_service,
            focus_handle: cx.focus_handle(),
        }
    }

    fn select_platform(&mut self, platform: Platform, cx: &mut Context<Self>) {
        if self.platform == platform {
            return;
        }
        self.platform = platform;
        cx.notify();
    }

    fn select_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        cx.notify();
    }

    fn confirm(&mut self, _: &menu::Confirm, window: &mut Window, cx: &mut Context<Self>) {
        let platform = self.platform;
        let mode = self.mode;
        let mut service = self.service_input.read(cx).text(cx).trim().to_string();
        if service.is_empty() {
            service = self.default_service.clone();
        }
        let mut region = self.region_input.read(cx).text(cx).trim().to_string();
        if region.is_empty() {
            region = platform.default_region().unwrap_or_default().to_string();
        }

        if !is_valid_service_name(&service) {
            self.show_error(
                anyhow!(
                    "Invalid service name {service:?}: use lowercase letters, digits, and \
                     hyphens, starting with a letter."
                ),
                cx,
            );
            return;
        }
        if platform.default_region().is_some()
            && !region
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            self.show_error(anyhow!("Invalid region {region:?}."), cx);
            return;
        }

        // Phase 2 rigging lands one platform at a time; refuse a mode/platform
        // combo whose s8sskills pipeline pack isn't authored yet.
        if mode == Mode::RigPipeline && !platform.pipeline_ready() {
            self.show_error(
                anyhow!(
                    "Rig the pipeline isn't available for {} yet — it's rolling out per \
                     platform. Vercel is ready now.",
                    platform.label()
                ),
                cx,
            );
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

        let allow_unauthenticated = self.allow_unauthenticated && platform.supports_public_toggle();
        let weak_workspace = self.workspace.clone();
        cx.spawn_in(window, async move |_this, cx| {
            // Install the s8sskills pack pieces the prompt depends on. Files
            // already present are left untouched (version-pinned by commit).
            let skills = match mode {
                Mode::QuickDeploy => platform.required_skills(),
                Mode::RigPipeline => platform.pipeline_skills(),
            };
            for skill in skills {
                if let Err(error) = ensure_skill_installed(
                    fs.clone(),
                    http_client.clone(),
                    &project_root,
                    platform.s8s_repo(),
                    skill,
                )
                .await
                {
                    log::error!("set_sail: failed to install skill {skill}: {error:#}");
                    weak_workspace
                        .update(cx, |workspace, cx| {
                            workspace.show_error(
                                anyhow!(
                                    "Set Sail needs the s8sskills `{skill}` skill but couldn't \
                                     fetch it (offline?): {error}"
                                ),
                                cx,
                            );
                        })
                        .ok();
                    return;
                }
            }

            let (prompt, title): (String, SharedString) = match mode {
                Mode::QuickDeploy => (
                    quick_deploy_prompt(platform, &service, &region, allow_unauthenticated),
                    format!("Set Sail: {service} → {}", platform.label()).into(),
                ),
                Mode::RigPipeline => (
                    rig_pipeline_prompt(platform, &service, &region, allow_unauthenticated),
                    format!("Rig the pipeline: {service} → {}", platform.label()).into(),
                ),
            };

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
    // Cloud Run (the strictest target) requires a leading letter.
    let slug = slug.trim_start_matches(|c: char| c.is_ascii_digit() || c == '-');
    slug.chars().take(63).collect()
}

/// One validation for all platforms: Cloud Run's rules are the strictest and
/// the resulting names are valid on Lambda and Vercel too.
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
    repo: &str,
    skill: &str,
) -> Result<bool> {
    let skill_dir = project_root.join(".agents").join("skills").join(skill);
    let skill_file = skill_dir.join("SKILL.md");
    if fs.is_file(&skill_file).await {
        return Ok(false);
    }

    let url =
        format!("https://raw.githubusercontent.com/s8sskills/{repo}/main/skills/{skill}/SKILL.md");
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
fn quick_deploy_prompt(platform: Platform, service: &str, region: &str, public: bool) -> String {
    let label = platform.label();
    let skills = platform.required_skills();
    let skill_lines: String = skills
        .iter()
        .map(|skill| format!("- .agents/skills/{skill}/SKILL.md\n"))
        .collect();
    let region_line = if platform.default_region().is_some() {
        format!("Region: {region}\n")
    } else {
        String::new()
    };
    let visibility_line = if platform.supports_public_toggle() {
        if public {
            "The service URL should be PUBLIC.\n"
        } else {
            "The service should require authentication (private).\n"
        }
    } else {
        ""
    };
    let deploy_hint = platform.deploy_hint(service, region, public);
    let prereqs = platform.prereq_checks();

    format!(
        "Set Sail: deploy this project to {label} (quick deploy).\n\n\
         Service name: {service}\n\
         {region_line}\
         {visibility_line}\n\
         These s8sskills skill files are installed in this project. Read them ALL first, in \
         order — they are the authoritative playbook:\n\
         {skill_lines}\n\
         Steps:\n\
         1. Read the skill files above and follow them wherever they are more specific than \
         these steps.\n\
         2. Check prerequisites in the terminal: {prereqs} — never run interactive auth flows \
         yourself; STOP, give me the exact command to run in a terminal, and wait for me to \
         confirm before continuing.\n\
         3. Identify the project type (look at the files present) and make sure it is \
         deployable; fix only what blocks the deploy.\n\
         4. {deploy_hint}, unless the skills prescribe something better for this project type.\n\
         5. On success, report the live URL and what got created, so I can find (and later \
         clean up) every resource.\n\
         6. On failure, diagnose from the error output and retry sensibly (a few attempts at \
         most), keeping me informed.\n\n\
         This is a one-off quick deploy — do NOT set up CI/CD pipelines or create git \
         infrastructure in this run."
    )
}

/// Self-contained "Rig the pipeline" prompt. Sets up the vendor-agnostic
/// cloud-side rigging (deployable target + least-privilege deploy identity +
/// exact deploy command) so the user can wire ANY CI/CD tool, then optionally
/// scaffolds a starter config with every secret left as a TODO placeholder.
/// The hard guardrail — the agent never fills real auth/secrets — is spelled
/// out inline so it survives even if the skill pack is terse.
fn rig_pipeline_prompt(platform: Platform, service: &str, region: &str, public: bool) -> String {
    let label = platform.label();
    let skills = platform.pipeline_skills();
    let skill_lines: String = skills
        .iter()
        .map(|skill| format!("- .agents/skills/{skill}/SKILL.md\n"))
        .collect();
    let region_line = if platform.default_region().is_some() {
        format!("Region: {region}\n")
    } else {
        String::new()
    };
    let visibility_line = if platform.supports_public_toggle() {
        if public {
            "The deployed URL should be PUBLIC.\n"
        } else {
            "The deployed service should require authentication (private).\n"
        }
    } else {
        ""
    };
    let deploy_hint = platform.deploy_hint(service, region, public);
    let prereqs = platform.prereq_checks();

    format!(
        "Set Sail: rig a CI/CD pipeline for deploying this project to {label}.\n\n\
         Service name: {service}\n\
         {region_line}\
         {visibility_line}\n\
         GOAL — do the vendor-agnostic \"basic rigging\" so I can plug in ANY CI/CD tool \
         (GitHub Actions, GitLab CI, Jenkins, Buildkite, …). You set up the cloud side; I own \
         the CI tool and all secrets. Stay tool-agnostic — do NOT assume or pick a CI tool \
         unless I name one.\n\n\
         These s8sskills skill files are installed in this project. Read them ALL first, in \
         order — they are the authoritative playbook:\n\
         {skill_lines}\n\
         Do the rigging:\n\
         1. Read the skill files above and follow them wherever they are more specific than \
         these steps.\n\
         2. Check prerequisites in the terminal: {prereqs} — never run interactive auth flows \
         yourself; STOP, give me the exact command to run in a terminal, and wait for me to \
         confirm before continuing.\n\
         3. Ensure the deploy target exists and is deployable ({deploy_hint}).\n\
         4. Create a least-privilege deploy identity for CI to use (a dedicated service \
         account / IAM role / deploy token per the skill). Prefer KEYLESS OIDC / workload-\
         identity federation over long-lived keys wherever the skill supports it.\n\
         5. Produce the exact deploy COMMAND a CI job should run, and tell me precisely which \
         credential/secret to hand my CI tool and what to name it.\n\n\
         Then OFFER (ask me first) to scaffold a starter CI config for whichever tool I name — \
         e.g. .github/workflows/deploy.yml, .gitlab-ci.yml, a Jenkinsfile, or \
         .buildkite/pipeline.yml — with the deploy command wired in. Leave EVERY secret, token, \
         login, and OAuth value as a clearly-labeled TODO placeholder for me to fill.\n\n\
         HARD RULES: never put a real secret, credential, token, or auth value in any file or \
         run a login/OAuth flow on my behalf; do NOT commit or push anything without asking; do \
         NOT choose my CI/CD tool for me.\n\n\
         Finally, summarize the \"deploy contract\": the deploy command, the credential/identity \
         you created, and exactly where I plug it into my CI tool."
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
        let platform = self.platform;
        let mode = self.mode;
        let selected_index = PLATFORMS
            .iter()
            .position(|p| *p == platform)
            .unwrap_or(0);
        let selected_mode_index = MODES.iter().position(|m| *m == mode).unwrap_or(0);
        let pipeline_unavailable = mode == Mode::RigPipeline && !platform.pipeline_ready();
        let description = match mode {
            Mode::QuickDeploy => {
                "Quick-deploy this project to a serverless platform. PaddleBoard's agent \
                 follows the s8sskills playbook: it checks your CLI setup, then deploys \
                 straight from source."
            }
            Mode::RigPipeline => {
                "Rig a CI/CD pipeline. The agent sets up the vendor-agnostic cloud side — a \
                 deploy identity, the resource, and the exact deploy command — so you can plug \
                 in any CI tool (GitHub Actions, GitLab, Jenkins, Buildkite). You keep control \
                 of secrets."
            }
        };
        let confirm_label = match mode {
            Mode::QuickDeploy => "Set Sail",
            Mode::RigPipeline => "Rig the pipeline",
        };

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
                        ModalHeader::new()
                            .headline("Set Sail ⛵")
                            .description(description),
                    )
                    .child(
                        v_flex()
                            .px_3()
                            .pb_2()
                            .gap_2()
                            .child(
                                ToggleButtonGroup::single_row(
                                    "set-sail-mode",
                                    [
                                        ToggleButtonSimple::new(
                                            Mode::QuickDeploy.label(),
                                            cx.listener(|this, _, _window, cx| {
                                                this.select_mode(Mode::QuickDeploy, cx);
                                            }),
                                        ),
                                        ToggleButtonSimple::new(
                                            Mode::RigPipeline.label(),
                                            cx.listener(|this, _, _window, cx| {
                                                this.select_mode(Mode::RigPipeline, cx);
                                            }),
                                        ),
                                    ],
                                )
                                .style(ToggleButtonGroupStyle::Outlined)
                                .selected_index(selected_mode_index),
                            )
                            .child(
                                ToggleButtonGroup::single_row(
                                    "set-sail-platform",
                                    [
                                        ToggleButtonSimple::new(
                                            Platform::CloudRun.label(),
                                            cx.listener(|this, _, _window, cx| {
                                                this.select_platform(Platform::CloudRun, cx);
                                            }),
                                        ),
                                        ToggleButtonSimple::new(
                                            Platform::AwsLambda.label(),
                                            cx.listener(|this, _, _window, cx| {
                                                this.select_platform(Platform::AwsLambda, cx);
                                            }),
                                        ),
                                        ToggleButtonSimple::new(
                                            Platform::Vercel.label(),
                                            cx.listener(|this, _, _window, cx| {
                                                this.select_platform(Platform::Vercel, cx);
                                            }),
                                        ),
                                    ],
                                )
                                .style(ToggleButtonGroupStyle::Outlined)
                                .selected_index(selected_index),
                            )
                            .when(pipeline_unavailable, |this| {
                                this.child(
                                    Label::new(format!(
                                        "Pipeline rigging for {} is coming soon — Vercel is \
                                         available now.",
                                        platform.label()
                                    ))
                                    .size(LabelSize::Small)
                                    .color(Color::Warning),
                                )
                            })
                            .child(self.service_input.clone())
                            .when(platform.default_region().is_some(), |this| {
                                this.child(self.region_input.clone())
                            })
                            .when(platform.supports_public_toggle(), |this| {
                                this.child(
                                    Checkbox::new(
                                        "set-sail-public",
                                        if self.allow_unauthenticated {
                                            ToggleState::Selected
                                        } else {
                                            ToggleState::Unselected
                                        },
                                    )
                                    .label("Public URL")
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.allow_unauthenticated = !this.allow_unauthenticated;
                                        cx.notify();
                                    })),
                                )
                            })
                            .child(
                                h_flex()
                                    .gap_0p5()
                                    .child(
                                        Label::new(format!(
                                            "Not set up on {}?",
                                            platform.vendor_name()
                                        ))
                                        .size(LabelSize::Small)
                                        .color(Color::Muted),
                                    )
                                    .child(
                                        Button::new("set-sail-get-started", "Get started!")
                                            .style(ButtonStyle::Transparent)
                                            .label_size(LabelSize::Small)
                                            .color(Color::Accent)
                                            .on_click(move |_, _window, cx| {
                                                cx.open_url(platform.getting_started_url());
                                            }),
                                    ),
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
                                    Button::new("set-sail-confirm", confirm_label)
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
    fn cloud_run_prompt_carries_service_region_and_visibility() {
        let prompt = quick_deploy_prompt(Platform::CloudRun, "boaty", "europe-west1", true);
        assert!(prompt.contains(
            "gcloud run deploy boaty --source . --region europe-west1 --allow-unauthenticated"
        ));
        assert!(prompt.contains("cloud-run-deploy/SKILL.md"));
        assert!(prompt.contains("gcloud-project-setup/SKILL.md"));
        let private_prompt = quick_deploy_prompt(Platform::CloudRun, "boaty", "us-central1", false);
        assert!(private_prompt.contains("require authentication"));
        assert!(!private_prompt.contains("--allow-unauthenticated"));
    }

    #[test]
    fn lambda_prompt_carries_function_url_choice_and_aws_checks() {
        let prompt = quick_deploy_prompt(Platform::AwsLambda, "boaty", "us-east-1", true);
        assert!(prompt.contains("lambda-deploy/SKILL.md"));
        assert!(prompt.contains("aws-project-setup/SKILL.md"));
        assert!(prompt.contains("aws sts get-caller-identity"));
        assert!(prompt.contains("public Function URL (auth type NONE)"));
        let private_prompt = quick_deploy_prompt(Platform::AwsLambda, "boaty", "us-east-1", false);
        assert!(private_prompt.contains("AWS_IAM"));
    }

    #[test]
    fn vercel_prompt_omits_region_and_visibility() {
        let prompt = quick_deploy_prompt(Platform::Vercel, "boaty", "", true);
        assert!(prompt.contains("vercel-deploy/SKILL.md"));
        assert!(prompt.contains("vercel-project-setup/SKILL.md"));
        assert!(prompt.contains("vercel deploy --prod --yes"));
        assert!(!prompt.contains("Region:"));
        assert!(!prompt.contains("PUBLIC"));
    }

    #[test]
    fn every_platform_declares_setup_then_deploy_skills() {
        for platform in PLATFORMS {
            let skills = platform.required_skills();
            assert_eq!(skills.len(), 2, "{platform:?} should have setup + deploy");
            assert!(
                skills[0].contains("setup") && skills[1].contains("deploy"),
                "{platform:?} skills out of order: {skills:?}"
            );
        }
    }

    #[test]
    fn every_platform_declares_setup_then_pipeline_skills() {
        for platform in PLATFORMS {
            let skills = platform.pipeline_skills();
            assert_eq!(skills.len(), 2, "{platform:?} should have setup + pipeline");
            assert!(
                skills[0].contains("setup") && skills[1].contains("pipeline"),
                "{platform:?} pipeline skills out of order: {skills:?}"
            );
        }
    }

    #[test]
    fn all_launch_platforms_are_pipeline_ready() {
        for platform in PLATFORMS {
            assert!(
                platform.pipeline_ready(),
                "{platform:?} should be pipeline-ready"
            );
        }
    }

    #[test]
    fn rig_pipeline_prompt_is_vendor_agnostic_and_guards_secrets() {
        let prompt = rig_pipeline_prompt(Platform::Vercel, "boaty", "", true);
        // Points at the pipeline pack, not the one-shot deploy pack.
        assert!(prompt.contains("vercel-pipeline/SKILL.md"));
        assert!(!prompt.contains("vercel-deploy/SKILL.md"));
        // Vendor-agnostic: names multiple CI tools and refuses to pick one.
        assert!(prompt.contains("GitHub Actions") && prompt.contains("Buildkite"));
        assert!(prompt.contains("do NOT choose my CI/CD tool"));
        // The hard secret-handling guardrail is spelled out inline.
        assert!(prompt.contains("TODO placeholder"));
        assert!(prompt.contains("never put a real secret"));
        // Still carries the concrete deploy command from the platform hint.
        assert!(prompt.contains("vercel deploy --prod --yes"));
    }
}
