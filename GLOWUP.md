# GLOWUP — the v0.2.0 UI polish pass

Design doc for the Phase 1 Glowup (gates v0.2.0). Produced from a four-part code
audit on 2026-07-11: chrome clutter, surface styling, theme system, first-launch.
Phase 2 (deeper work, by v1.0.0 GA) items are marked ⏭.

## Principles

1. **Declutter first.** PaddleBoard's own additions are the crowding: 4 always-on
   status items and 4 unhideable dock buttons. Everything PB adds must be at
   least as polite as upstream chrome (hideable, sensibly prioritized).
2. **One house style.** The `ui` crate already ships the kit (`Modal`, `Section`,
   `Callout`, `Chip`, `ListItem`); PB surfaces adopt it instead of hand-rolling.
   The AI Dock *sub*-modals (AddAgent/AddSkill/BuildMcp) are the de-facto
   reference style: `ui::Modal` + `ModalHeader`/`ModalFooter`, body
   `px_3 pb_2 gap_2`, buttons `.label_size(Small)` with explicit styles.
3. **Own the look.** Ship "PaddleBoard Dark/Light" as the default theme, derived
   from the site's palette, so app and brand read as one product.
4. **First launch must land a working agentic IDE.** A new user should finish
   onboarding with a model configured and a rendered, skimmable tour.
5. **Naming:** decided per-surface as waves reach them. Default stance: nautical
   for memorable surfaces (Set Sail ⛵, AI Dock, Manifest — done), plain names
   for config/utility areas.

## Brand palette (from paddleboard.dev — Catppuccin-Mocha-derived)

| Role | Dark | Light |
|---|---|---|
| Deepest bg (crust) | `#181825` | page `#ffffff` |
| Base bg | `#1e1e2e` | page-alt `#f4f6fc` |
| Surface | `#2d3250` | — |
| Overlay/border | `#45475a` | `#e3e8f4` |
| Text | `#cdd6f4` | ink `#1e1e2e` / soft `#4c4f69` |
| Accents | sky `#89dceb`, blue `#89b4fa`, yellow `#f9e2af`, pink `#f38ba8` | same |
| Signature gradients | board: sky→blue · paddle: yellow→pink | |

Primary accent proposal: **blue `#89b4fa`** (matches site links/CTAs), with sky
for secondary accents.

---

## Wave 1 — Declutter mechanics (small, mechanical, ship first)

Status bar today: 12+ items; PB added 4 right-side items (sandbox shield, MCP
count, usage gauge, sailboat) — none hideable. Dock: 10 panels; PB's 4
(Browser, LlmPicker, Orchestration, Manifest) lack the `button` setting +
`hide_button_setting` pattern every upstream panel implements, and their
activation priorities (8/9/9/10) push them past the entire upstream band (max 7).

- [x] Add `button`-style hide settings for all 4 PB panels (pattern:
      `git_panel.rs:7474`) and wire `hide_button_setting`.
- [x] Add hide settings for the 4 PB status items (pattern:
      `StatusBarSettings`, `workspace_settings.rs:158-166`); currently all
      return `hide_setting → None` (`paddleboard_sandbox_prereqs_ui.rs:102`,
      `mcp_servers_ui.rs:1057`, `paddleboard_set_sail.rs:298`).
- [x] Contextualize the always-on trio: sailboat + shield could show only when
      a project is open; MCP count already self-hides at zero.
- [x] Re-band activation priorities so PB panels interleave sensibly instead of
      clustering at the rail's end (proposal: Manifest near GitPanel=3+,
      LlmPicker/Orchestration near AgentPanel=0, Browser stays last).
- [x] **Fix the icon collision:** OrchestrationPanel and ManifestPanel both use
      `IconName::ListTree`. Manifest gets its own glyph (hand-drawn scroll/
      manifest icon, Sailboat-style precedent) — or Orchestration moves to a
      threads-ish glyph.

## Wave 2 — House style kit (adopt `ui` components, kill the forks)

From the styling audit (worst offenders → fix):

- [x] **Modal shell**: AI Dock (`ai_dock.rs:248`) and Sandbox Prereqs
      (`paddleboard_sandbox_prereqs_ui.rs:443`) hand-roll their shells — move
      both onto `ui::Modal`/`ModalHeader`/`ModalFooter`. Pick ONE modal-title
      treatment (ModalHeader's `Headline Small` default) — today it's
      Small/Medium/Large across three surfaces.
- [x] **Callout adoption**: every notice/warning/banner is bespoke. Convert:
      sandbox active-tier banner + coverage notes (`:404,308,318`), Set Sail
      warning (`:1037`), Usage disabled/empty notices (`usage_tab.rs:85,98`),
      Scion install prompt (`orchestration_panel.rs`).
- [x] **Extract `SelectableRow`** (the Icon(Check/Circle) radio-row hand-rolled
      identically in Set Sail `:988`, Scion persona `:232` + template `:308`,
      Sandbox `:280`) into the `ui` kit or a `paddleboard_ui` helper.
- [x] **One card style**: pick `p_3 gap_2 rounded_md border_1 border_variant`
      + `elevated_surface_background.opacity(0.5)` (AI Dock rows) and apply to
      Usage cards (`element_background` today) and MCP catalog rows (no bg).
- [x] **One row padding token** (today: 6 variants incl. raw `pl(px(8.0))` in
      `orchestration_panel.rs:641`).
- [x] **Button discipline**: `.label_size(Small)` + explicit style everywhere
      (Git Login, Languages, Sandbox currently default-sized).
- [x] **Width scale for modals**: 28/34/48 rems (S/M/L) instead of today's
      28/30/32/34/38 free-for-all.
- [x] Kill raw `FontWeight` on Labels (`sandbox:288,351`) — emphasis = Headline.
- [x] One loading affordance (shared spinner treatment) and one inline-link
      convention.

## Wave 3 — The PaddleBoard theme

The pipeline is name/JSON-driven end to end; no loader changes needed.

- [ ] Author `assets/themes/paddleboard/paddleboard.json` — family
      "PaddleBoard", themes "PaddleBoard Dark" + "PaddleBoard Light" from the
      brand palette (structural template: `assets/themes/one/one.json`).
      Original palette → no license carry-over needed.
- [ ] Defaults: `DEFAULT_LIGHT/DARK_THEME` (`settings_content/src/theme.rs:289`)
      + `assets/settings/default.json:13-14` → PaddleBoard names.
- [ ] Onboarding picker: widen the `[_; 3]` arrays in
      `onboarding/src/basics_page.rs:21-27` to 4 families, PaddleBoard first +
      preselected.
- [ ] ⏭ Optional: recolor the compiled-in fallback (`fallback_themes.rs:60-109`).
- [ ] Syntax colors: derive from Catppuccin Mocha/Latte conventions (MIT;
      attribute if copied rather than inspired).

## Wave 4 — First launch

Today: onboarding (theme/keymap/AI-Dock-button/vim) → welcome page → the tour
auto-opens as a RAW markdown buffer, 154 lines / 20 sections. No provider setup
anywhere — a new user finishes onboarding unable to talk to any model.

- [ ] **Add a provider step to onboarding** — the single worst gap. Reuse the
      LlmPicker's provider list + the API-key configuration views; include
      Local Models as the zero-key path.
- [ ] **Render the tour** as a markdown *preview* (not a raw buffer).
- [ ] **Slim the tour**: 20 sections → ~6 guided stops (Agent+provider, Manifest
      /git, Sandbox, Set Sail, Personas, AI Dock) with "everything else" linking
      to WELCOME.md/docs. The full feature list already lives in WELCOME/docs.
- [ ] **Thread the narrative**: onboarding "Finish Setup" → welcome page card
      pointing at the tour; tour closes pointing at the AI Dock.
- [ ] Re-surface updated tours: content-hash in the `.tour_seen` marker + a
      toast ("The tour has new sections") instead of silent rewrite.
- [ ] Dedupe the tour-materialization logic (`main.rs:1607` vs
      `workspace.rs:794`) into one helper.

## Wave 5 — Consolidation (decisions required)

The overlap map found real redundancy. Each needs a call (options ordered by
recommendation):

1. **AI provider surfaces (3 today):** LlmPicker panel vs settings LLM page vs
   AI Dock. → Recommend: keep the settings page as the deep config; make the
   LlmPicker panel the quick *switcher* only (slim it); AI Dock stays
   browse/install. ⏭ or fold LlmPicker into the AgentPanel header entirely.
2. **Agent thread lists (3):** OrchestrationPanel vs AgentPanel list vs AI Dock
   Agents tab. → Recommend: OrchestrationPanel is the canonical "what's
   running" view (it owns Scion too); AI Dock Agents tab becomes
   install/catalog only (drop its running-threads aspect).
3. **Git surfaces (3):** GitPanel History tab vs Git Graph vs Manifest.
   → Recommend: keep all three but sharpen roles — Manifest = overview
   (its cap already says so), Graph = deep history, History tab ⏭ candidate to
   become just a shortcut to the Graph.
4. **Naming calls** (per-surface, during each wave): LlmPicker panel rename if
   it survives as switcher ("Providers"? "Quiver"?); Orchestration panel
   ("Agents"? "Fleet"?). Decide when restyling touches each surface.

## Sequencing & release

Waves 1–2 are mechanical and can land immediately as normal PRs. Wave 3 is one
focused PR (the theme JSON). Wave 4 is the largest single UX change. Wave 5
lands per-decision. Suggested cut: v0.2.0 ships when Waves 1–4 are in and at
least decisions 1–2 of Wave 5 are resolved; remaining Wave 5 items may ride
v0.2.x. The `samples/demo` folder (separate project item) should follow the
Glowup so demo screenshots show the new look.

## Out of scope for Phase 1 (⏭ Phase 2 / GA)

Custom title bar treatments, editor-area theming beyond the theme JSON, the
custom terminal idea, animation/motion polish, upstream-Zed surface restyling
(fork hygiene: PB surfaces only), full onboarding wizard rebuild.
