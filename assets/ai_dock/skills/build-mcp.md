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
Call the **`install_mcp_server` tool** — do NOT try to edit `settings.json` yourself.
You build inside the sandbox, which can't reach host paths; the tool runs in the
PaddleBoard process, so it persists the server to the data dir and registers it for
you. Pass the final file contents (the tool writes them; you don't need to have
saved them to disk):

```
install_mcp_server({
  "id": "<slug>",
  "files": { "server.py": "<full server.py contents>", "requirements.txt": "<contents>" },
  "entry": "server.py",
  "requirements": "requirements.txt"
})
```

The server is installed as a plain host process (`uv run`), so it reads its API key
from the environment PaddleBoard was launched with — **never** put secret values in
the files or args.

### Fallback if `install_mcp_server` isn't available

The "Build an MCP" button forces the native agent, which always has this tool. But
if you're running under an external agent that lacks it, install by hand — **get the
paths and the entry shape exactly right**:

1. Write `server.py` + `requirements.txt` to the OS-correct data dir (NOT a worktree):
   - macOS: `~/Library/Application Support/PaddleBoard/mcp_servers/<slug>/`
   - Linux: `~/.local/share/PaddleBoard/mcp_servers/<slug>/`
   - Windows: `%LOCALAPPDATA%\PaddleBoard\mcp_servers\<slug>\`
2. Register a **plain Stdio** context server in `settings.json` under
   `"context_servers"`. Use `uv` so dependencies resolve from `requirements.txt`:
   ```json
   "context_servers": {
     "<slug>": {
       "source": "custom",
       "enabled": true,
       "command": "uv",
       "args": ["run", "--with-requirements",
                "<data-dir>/mcp_servers/<slug>/requirements.txt",
                "<data-dir>/mcp_servers/<slug>/server.py"]
     }
   }
   ```
   Expand `<data-dir>` to the absolute path for the current OS (no `~`). Do **NOT**
   add `forward_env` — that field is sandboxed-stdio only and is ignored here; the
   server inherits env from the shell that launched PaddleBoard, so the user just
   exports `<AUTH_ENV_VAR>` there.

## 6. Report
Tell the user: the server **id**, the **tools** it exposes, that they must export
`<AUTH_ENV_VAR>` in the shell that launches PaddleBoard, and that it now appears in
**AI Dock → MCP**. If a sandbox runtime was missing, mention they can enable
sandboxed execution via the status bar's Sandbox Prerequisites.
