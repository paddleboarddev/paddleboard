# Welcome aboard PaddleBoard! 🏄‍♂️

PaddleBoard is an agentic, high-performance IDE built for AI-driven development —
code natively, let the agent act on your workspace, browse the web, and run
tests in secure sandboxes, all from one window.

This is a short guided tour: **six stops** to get you productive. The full
feature list lives in `WELCOME.md` and at [docs.paddleboard.dev](https://docs.paddleboard.dev).

---

## 1. The Agent — and a model to run it 🤖

The agent panel is where you talk to your AI. If you set up a provider during
onboarding, you're ready; if not, no problem.

- Open the agent panel from the right dock, then type a request.
- **No model yet?** Onboarding's **AI Providers** section gets you connected —
  run a model locally with **no API key** (Local Models), or paste your own key
  for OpenAI, Anthropic, Google, and more.
- Switch models any time from the **LLM Provider** picker without opening settings.

## 2. Manifest — your git state at a glance 🗂️

One dockable tree for the whole git picture — the ship's manifest.

- Open it with the tree icon in the dock, or `Cmd-Shift-P` → **`manifest: Toggle Focus`**.
- See **Repositories**, **Branches** (ahead/behind), **Commits** (click → diff),
  **Stashes**, and **Contributors** — all read from state PaddleBoard already
  tracks, so it's instant.
- The **Git Graph** stays the deep-history view; Manifest is the overview.

## 3. Secure Sandboxing — the agent runs code safely 🛡️

When the agent runs untrusted code, compiles, or runs tests, it uses the
integrated **Sandbox** — your project mounts in, permission prompts still gate
every command, and the sandbox is discarded when done.

- Click the **shield icon** in the status bar to pick your backend tier.
- **Native** is zero-install (Apple `container`, bundled libkrun microVM, or
  KVM on Linux); **Podman + gVisor** is the strongest tier.
- Each option stages its install in a fresh Terminal — nothing runs inside the app.

## 4. Set Sail — deploy to serverless ⛵

Quick-deploy the current project to Cloud Run, AWS Lambda, Vercel, Azure,
Cloudflare, or Netlify — no YAML safari.

- Run it: `Cmd-Shift-P` → **`set sail: Deploy`**, pick a platform, and the agent
  takes the helm using the open-source [s8sskills](https://s8sskills.com) playbook.
- Interactive auth steps (`gcloud auth login`, `vercel login`) are handed to you
  in the terminal — the agent never runs auth flows itself.
- Flip to **Rig the Pipeline** mode to set up CI/CD groundwork instead.

## 5. Personas — tell the agent who to be 🎭

A **persona** describes who the agent should mimic — a Senior Developer, an SRE,
a QA Engineer.

- Drop a `PERSONA.md` at your project root and new threads adopt it automatically.
- Grab starter roles in **AI Dock → Personas**, switch per-thread with the persona
  picker, or just ask mid-conversation: *"be my QA tester."*
- Works with every provider, and with container-isolated **Scion** agents too.

## 6. The AI Dock — your next stop 🛟

One place to browse and install everything the agent talks to — the marina where
every external collaborator ties up.

- Open it: `Cmd-Shift-P` → **`ai_dock: Open`**, or **Open the AI Dock** on the
  Welcome screen.
- Tabs for **Agents**, **Skills**, **Personas**, **MCP Servers**, and **Usage**.
- Installed items show a green badge; missing ones get a one-click
  **Install / Sign In / Set Up**.

**Head to the AI Dock now** to add your first agent or skill — it's the best
place to start exploring.

---

*There's much more — the embedded browser, sandboxed MCP servers, Scion parallel
agents, local semantic search, Git Login, and more. See `WELCOME.md` or
[docs.paddleboard.dev](https://docs.paddleboard.dev) for the full list.*

*Reopen this tour any time: Command Palette (`Cmd-Shift-P`) →
**`workspace: Open Paddle Board Tour`**.*
