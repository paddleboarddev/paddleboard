# SCION HUB — connecting PaddleBoard to a hosted Scion

Design doc for integrating [Scion Hub](https://googlecloudplatform.github.io/scion/hosted/single-node/hub-server/)
with PaddleBoard's existing Scion support. Produced from a research pass on
2026-07-30 against Scion `0.1.0-dev` (build 2026-05-26) and the shipped
`paddleboard_scion` crates.

**Scheduled after the v0.3.0 beta.** Not a launch item — see
[Why this waits](#why-this-waits).

---

## What Scion Hub is

The **control plane** for a hosted Scion deployment. Four responsibilities:

| Role | What it does |
|---|---|
| Central registry | Projects, runtime brokers, templates |
| Identity provider | OAuth login, scoped JWTs for agents and brokers |
| State store | Agent lifecycle, status, metadata (SQLite dev / Postgres prod) |
| Task dispatcher | Routes CLI and Dashboard commands to brokers over WebSocket |

**Runtime Brokers** are the execution hosts; the Hub is stateless coordination.
Brokers dial *out* to the Hub over WebSocket tunnels, so they work behind NAT
and firewalls. Agents run on brokers — **not on the user's machine**. A Web
Dashboard (default `:8080`) fronts the whole thing. "Combo mode" runs Hub +
Broker + Dashboard in one process on one box, which is the realistic shape for
a small team.

The short version: **Hub is the multi-user story for Scion.** Local mode is one
developer orchestrating containers on their own machine; Hub adds accounts,
shared visibility, persistent state, and remote execution.

---

## The finding that makes this cheap

PaddleBoard talks to Scion by **shelling out to the `scion` binary** —
`crates/paddleboard_scion/src/paddleboard_scion.rs` is `Command::new(binary)`
parsing `--json` output. There is no HTTP client anywhere in the integration.

The obvious conclusion is that Hub means writing a REST + WebSocket client
against a new API. **It does not.** Hub is a global flag on every `scion`
invocation:

```
--hub string   Hub API endpoint URL (overrides SCION_HUB_ENDPOINT)
--no-hub       Disable Hub integration for this invocation (local-only mode)
```

with a full subcommand surface behind `scion hub`:

```
allow-list  auth  brokers  disable  enable  env  invite
link  projects  secret  status  token  unlink
```

and three ways to configure the endpoint, in precedence order: the `--hub`
flag, the `SCION_HUB_ENDPOINT` environment variable, `hub.endpoint` in
`settings.yaml`.

Scion's own docs describe the local→hosted shift as "largely transparent to the
CLI user": the same commands work, the execution locus changes. So PaddleBoard's
existing `scion list --json` already returns Hub-managed agents when the CLI is
pointed at a Hub. **The work is UX and configuration, not protocol.**

> ⚠️ **Read `--help`, not the docs, for these names.** The published
> documentation and every search result point at `SCION_HUB_URL`. The actual
> binary reads **`SCION_HUB_ENDPOINT`**. The docs also omit the `--hub` and
> `--no-hub` flags entirely. Verify every flag and variable against the
> installed binary before writing code against it.

---

## Principles

1. **Connect to the user's Hub. Never run one.** PaddleBoard's shipped
   positioning is "the open-source AI IDE you actually own — no telemetry, no
   hosted plan." A PaddleBoard-operated control plane *is* a hosted plan, with
   accounts and infrastructure we'd have to run. Integrating with a Hub the
   user stands up themselves is the opposite: their infra, their auth, their
   keys. This principle decides most of the questions below.
2. **Read Hub state; don't own it.** The Web Dashboard already does project
   creation, invites, quotas, and allow-lists well. PaddleBoard's job is to
   show what's running and let you act on it from the editor.
3. **Degrade to today.** With no Hub configured, every surface behaves exactly
   as it does now. Hub is additive, never a new required step.
4. **Stay opt-in.** Scion is already gated behind `paddleboard_scion.enabled`
   and self-installed. Hub inherits that; it does not widen PaddleBoard's
   install-time surface. See the low-bloat principle.
5. **Remote is visible.** An agent running on someone else's broker is not the
   same as one running locally — different filesystem, different lifetime,
   possibly someone else's. The UI must never let those look identical.

---

## Phase 1 — Honor an existing Hub (the whole recommended scope)

The cheapest useful slice, and possibly the only one worth building.

**Premise:** the user has already run `scion hub auth login` and configured an
endpoint. PaddleBoard notices and reflects it.

- **Detect.** Call `scion hub status --json` alongside the existing version
  probe. No new config of our own — the CLI has already resolved
  flag/env/`settings.yaml` precedence for us, and reports which source won.

  Verified against `0.1.0-dev` with no Hub configured. It **exits 0 and returns
  well-formed JSON** rather than erroring, which is what makes the
  degrade-to-today path trivial — absence is a normal reading, not a failure to
  distinguish from a broken CLI:

  ```json
  {
    "configured": false,
    "enabled": false,
    "endpoint": "",
    "endpointSource": "none",
    "authMethod": "none",
    "hasToken": false,
    "isDevAuth": false,
    "cliOverride": false,
    "enabledScope": "default",
    "projectId": "51fa179a-…",
    "brokerId": "74116692-…",
    "scionVersionLocal": "unknown"
  }
  ```

  `configured` and `enabled` are the two booleans the UI keys off.
  `endpointSource` tells you whether the endpoint came from the flag, the
  environment, or settings — which is exactly what a user debugging "why is it
  pointed there" needs to see. `authMethod` + `hasToken` + `isDevAuth` cover
  the signed-in states, including the dev-token case.
- **Surface.** A row at the top of the Scion section in the orchestration
  panel: endpoint host, signed-in identity, connected/disconnected. Muted and
  quiet when absent, matching how Scion itself is presented today.
- **Distinguish remote agents.** Hub-managed agents get a marker in the agent
  list. `AgentInfo` already `#[serde(default)]`s every field including
  `container_id`, so remote payloads deserialize without schema work — but a
  remote agent with no local container must not render as though its files are
  on this machine.
- **Route actions correctly.** Existing agent actions keep working; they simply
  reach a broker instead of a local container. Verify each one against a live
  Hub rather than assuming transparency holds.
- **Hand off to the Dashboard.** A link out to the Web Dashboard for anything
  administrative. This is the deliberate boundary from principle 2.

**Not in Phase 1:** creating or linking projects, managing invites, allow-lists,
secrets, tokens, or brokers. All of it exists in `scion hub` subcommands and all
of it belongs in the Dashboard.

---

## Phase 2 — Only if Phase 1 proves it (⏭)

Revisit after Phase 1 has real use. Each of these needs a reason beyond "the
CLI can do it":

- ⏭ **Sign in from the editor** — wrap `scion hub auth login`. Precedent says
  hand off to a terminal rather than shell out in-app (the install-wizard UX
  rule); the browser flow may make this moot.
- ⏭ **Project link/unlink** — `scion hub link` from the project. Only if
  switching projects in the Dashboard proves genuinely annoying.
- ⏭ **Broker visibility** — `scion hub brokers` in the panel. Useful for
  debugging "why is nothing running"; noise otherwise.
- ⏭ **Crew convergence** — the parked multi-user profiles idea and Hub answer
  overlapping questions (whose agents, whose credentials, which project). If
  Crew is ever built, it should be designed *with* Hub, not beside it. Neither
  should be started without checking the other.

---

## Why this waits

- **Scion is `0.1.0-dev`.** One tag, built 2026-05-26. Hub is its newest
  surface. `paddleboard_scion`'s compat gate pins `TESTED_VERSION = "0.1.0"`
  and already warns on drift — building against a brand-new API on a pre-1.0
  dependency is the risk here, not the integration work.
- **The reachable audience is small.** Scion is opt-in *and* self-installed;
  Hub users are a subset of that subset, and each needs infrastructure stood up
  before any of this does anything. Low launch value against real timing cost.
- **Nothing about it is beta-blocking.** No Phase 1 item changes default
  behavior for a user without a Hub.

---

## Open questions

1. ~~**Does `scion hub status` emit `--json`?**~~ **RESOLVED 2026-07-30** — it
   does, and it exits 0 with valid JSON even when nothing is configured. Shape
   captured in Phase 1 above. This was the question gating the design; it came
   back favorable.
2. **What does the compat gate do about Hub?** `TESTED_VERSION` tracks the CLI,
   but Hub behavior could vary independently by server version. A Hub-capable
   scion may need its own floor, or the gate may need to distinguish
   client-tested from hub-tested.
3. **What does a remote agent's file context mean in the editor?** PaddleBoard
   is a local editor; a Hub agent edits a workspace on a broker. Whether that
   is browsable, or purely observable from here, is unresolved and is the
   deepest question in this doc — it may be the real reason Phase 1 stays
   read-only.
4. **Auth expiry.** Tokens live in `~/.scion/config.json`; PATs are
   `scion_pat_*`. Expired credentials must produce a clear "sign in again"
   state, not a generic command failure.

---

## Reference

- [Hub server](https://googlecloudplatform.github.io/scion/hosted/single-node/hub-server/) — architecture, combo mode, ports (hub `9810`, dashboard `8080`, broker `9800`), `~/.scion/settings.yaml`
- [Authentication](https://googlecloudplatform.github.io/scion/hosted/single-node/auth/) — `scion hub auth login`, `~/.scion/config.json`, `scion hub token create`, dev token at `~/.scion/dev-token`
- [Concepts](https://googlecloudplatform.github.io/scion/concepts/) — CLI/Hub/Broker/agent relationships, profiles
- Existing integration: `crates/paddleboard_scion`, `paddleboard_scion_settings`, `paddleboard_scion_ui`
