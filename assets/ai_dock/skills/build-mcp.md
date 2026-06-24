---
description: Research a service's API, write a Python MCP server, test it in the sandbox, and install it into the AI Dock so the agent can use it.
allowed-tools: ["Bash", "Read", "Write", "Edit", "WebSearch", "WebFetch", "Monitor"]
---

# /build-mcp — Build and install an MCP server

Turn a service that has no native MCP server into one PaddleBoard can use. The
"Build an MCP" button in the AI Dock seeds a thread that runs this; you can also
invoke it directly with the service details.

Inputs (from the prompt that invoked you): **SERVICE_NAME**, optional **DOCS_URL**,
optional **AUTH_ENV_VAR** (e.g. `SUBSTACK_API_KEY`), and a **DESCRIPTION** of what
the server should do. If `$ARGUMENTS` is empty, ask the user for the service and
what they want before proceeding.

## 1. Research the API
- If `DOCS_URL` is given, `WebFetch` it; otherwise `WebSearch` for `"<SERVICE_NAME> API reference"`.
- Determine the **base URL**, the **auth scheme** (where `AUTH_ENV_VAR` is used — header? query param?), and the **2–5 endpoints** needed to satisfy `DESCRIPTION`.
- Summarize what you found before writing code.

## 2. Scaffold
- Pick a slug: lowercase, hyphenated `SERVICE_NAME` (e.g. `substack`).
- Create the server under the durable PaddleBoard data dir, outside any worktree:
  `~/.local/share/PaddleBoard/mcp_servers/<slug>/` (on macOS: `~/Library/Application Support/PaddleBoard/mcp_servers/<slug>/`).
- Write `server.py` (a **FastMCP** server over stdio, using the `mcp` SDK) and `requirements.txt` (`mcp`, `httpx`).

## 3. Implement the tools
- One `@mcp.tool()` per endpoint from step 1, named and documented for what `DESCRIPTION` asked.
- Read the API key from `os.environ["<AUTH_ENV_VAR>"]`. **Never hardcode secrets.** If the variable is unset, raise a clear error telling the user to export it.

## 4. Test it in the sandbox
- Prefer the sandbox: create a venv, `pip install -r requirements.txt`, and run a smoke test that imports `server.py` and confirms the tools register and the server starts.
- If sandbox prerequisites (Podman + gVisor) are missing, say so once and fall back to a host `python -m venv` smoke test.
- Fix and re-test until it runs clean.

## 5. Install it
Register the server so the AI Dock launches it. **Read** the existing settings,
**merge** the new key (don't clobber the file), **write** it back, then **re-read**
to confirm it still parses. Settings live at `~/.config/paddleboard/settings.json`,
under `context_servers`.

- **If Podman + gVisor are available**, install it sandboxed (the presence of `image` selects the sandboxed transport):
  ```jsonc
  "context_servers": { "<slug>": {
    "command": "sh",
    "args": ["-c", "pip install -q -r /workspace/requirements.txt && python /workspace/server.py"],
    "image": "python:3.12-slim",
    "forward_env": ["<AUTH_ENV_VAR>"],
    "mount_worktree": false,
    "enabled": true } }
  ```
- **Otherwise** (no sandbox runtime), keep the host venv from step 4 and install plain stdio:
  ```jsonc
  "context_servers": { "<slug>": {
    "command": "<data dir>/mcp_servers/<slug>/.venv/bin/python",
    "args": ["<data dir>/mcp_servers/<slug>/server.py"],
    "enabled": true } }
  ```
- In both cases the secret **value** is never written to settings — `forward_env` (or the inherited host env) resolves the name at launch.

## 6. Report
Tell the user: the server **id**, the **tools** it exposes, that they must export
`<AUTH_ENV_VAR>` in the shell that launches PaddleBoard, and that it now appears in
**AI Dock → MCP**. If a sandbox runtime was missing, mention they can enable
sandboxed execution via the status bar's Sandbox Prerequisites.
