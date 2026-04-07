# Welcome to PaddleBoard! 🏄‍♂️

PaddleBoard is your new agentic, highly-performant IDE fork designed specifically for AI-driven software development. You can code natively, let the AI act on your workspace, browse the web, and run tests in secure sandboxes all from one window!

## 🚀 Key Features

### 1. The Autonomous PaddleBoard Agent
Your AI companion isn't just a chatbot—it's a developer. Open the assistant panel, give it a goal, and watch as it:
- Reads and searches your codebase using fast grep tools.
- Executes multi-file edits automatically.
- Plans complex architectural changes using an iterative loop.

### 2. Secure Agent Sandboxing (Podman + gVisor)
Never worry about the AI accidentally breaking your host system.
When the AI needs to run untrusted code, compile new binaries, or run tests, it uses the integrated **Sandbox Tool**.
- All executions happen in an ephemeral `ubuntu:latest` container.
- It uses the `runsc` (gVisor) runtime for deep isolation.
- Your project directory is safely mounted so builds succeed without host contamination.

### 3. Integrated Native Chromium Browser
Need to check documentation or view a local dev server? 
PaddleBoard integrates a lightning-fast native Chromium/WebKit browser directly into the IDE.
- Press `Cmd-Shift-P` and search for **`workspace: Open Browser`** to test it out!

---

*You can always revisit this tour by opening the Command Palette (`Cmd-Shift-P`) and selecting **`workspace: Open Paddle Board Tour`**.*
