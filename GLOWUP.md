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
**SHIPPED 2026-07-18** — `load_bundled_themes` globs `themes/**/*.json`, so the
new file auto-registers with zero loader changes (confirms the pure-JSON claim).

- [x] Author `assets/themes/paddleboard/paddleboard.json` — family
      "PaddleBoard", themes "PaddleBoard Dark" + "PaddleBoard Light" from the
      brand palette (structural template: `assets/themes/one/one.json`).
      Original palette → no license carry-over needed. Chrome surfaces use the
      site's own bg (`#181825`/`#1e1e2e`/`#2d3250`); accent = blue `#89b4fa`.
- [x] Defaults: `DEFAULT_LIGHT/DARK_THEME` (`settings_content/src/theme.rs:289`)
      + `assets/settings/default.json:13-14` → PaddleBoard names.
- [x] Onboarding picker: widened the `[_; 3]` arrays in
      `onboarding/src/basics_page.rs` to 4 families, PaddleBoard first +
      preselected (selection follows the new default theme name).
- [ ] ⏭ Optional: recolor the compiled-in fallback (`fallback_themes.rs:60-109`).
      Deliberately left "One Dark" — it's the emergency fallback + the JustBase
      test anchor (`theme::DEFAULT_DARK_THEME`), separate from the JSON default.
- [x] Syntax colors: Catppuccin Mocha (dark) / Latte (light) conventions —
      inspired, not copied verbatim (MIT palette; no license file needed).

## Wave 4 — First launch

Today: onboarding (theme/keymap/AI-Dock-button/vim) → welcome page → the tour
auto-opens as a RAW markdown buffer, 154 lines / 20 sections. No provider setup
anywhere — a new user finishes onboarding unable to talk to any model.

- [x] **Add a provider step to onboarding** — the single worst gap. **SHIPPED
      2026-07-18** (PR 1, branch `glowup/wave4-provider-step`). Onboarding was
      already a single scrolling page, so this is a new stateful *section*
      (`onboarding/src/ai_provider_section.rs`, `AiProviderSection` view) not a
      paginated step. Zero-key hero embeds `LocalModelsView` directly; BYO
      providers render inline `ui_input::InputField` (masked) + Save →
      `provider.set_api_key`, reusing `ui::ConfiguredApiCard` for the configured
      state. Threaded into `render_basics_page` after the AI Dock section.
**PR 2 (tour/welcome overhaul) SHIPPED 2026-07-18** — all five items below, on
the same branch. New `crates/paddleboard/src/tour.rs` consolidates everything;
registered via `tour::init` (per-workspace `observe_new`, mirroring
`markdown_preview::init`). The old blocks in `main.rs` and `workspace.rs` are
removed. ⚠️ **Behavior change:** first launch no longer *auto-opens* the tour —
the Welcome page's "Take the Tour" card is the entry point now.

- [x] **Render the tour** as a markdown *preview* — `tour::open_tour` resolves the
      tour file (outside any worktree) via `find_or_create_worktree(visible=false)`
      then `MarkdownPreviewView::open_for_project_path`. Had to live in the
      `paddleboard` crate: `markdown_preview` depends on `workspace`, so the old
      `workspace.rs` handler couldn't reach it (cycle). Handler relocated; the
      `OpenPaddleBoardTour` struct stays in `workspace`.
- [x] **Slim the tour**: 20 `###` sections → 6 `##` curated stops, decoupled from
      WELCOME.md, with intro + closing that point to WELCOME/docs for the rest.
- [x] **Thread the narrative**: Welcome page gains a prominent "Take the Tour"
      (Filled) button → `OpenPaddleBoardTour`; the tour's stop 6 hands off to the
      AI Dock.
- [x] Re-surface updated tours: `.tour_seen` now stores an FNV-1a fingerprint of
      the shipped tour; on a later launch where it differs, a "tour has new
      sections" toast (with an Open Tour button) fires instead of a silent rewrite.
- [x] Dedupe the tour-materialization logic — one `tour.rs`; both duplicated
      blocks deleted. Also updated the `/update-tour` skill + AI Dock skill for
      the decoupled curated model (no longer mirrors WELCOME.md).

## Wave 5 — Consolidation (decisions RESOLVED 2026-07-18; BUILT 2026-07-18)

The overlap map found real redundancy. Jay ruled on all four (each took the
recommended option); build status noted per item:

1. **AI provider surfaces (3 → sharpened roles):** ✅ Keep the settings LLM page
   as the deep config; slim the LlmPicker panel to a quick *switcher* only;
   AI Dock stays browse/install. (Rejected: folding LlmPicker into the
   AgentPanel header.) **BUILT** — `llm_picker.rs` slimmed: removed the
   per-provider config section (`ProviderConfigView`, `configuration_view_for`,
   the `configuration_view` field), keeping the provider list + a "Use as
   Default" switcher footer. Deep config = settings LLM page.
2. **Agent thread lists (3 → 2 roles):** ✅ OrchestrationPanel is the canonical
   "what's running" view (it owns Scion too); AI Dock Agents tab becomes
   install/catalog only. **NO-OP** — the AI Dock Agents tab was already
   catalog/install-status only; it never had a running-threads aspect (that
   lives solely in `agent_ui/orchestration_panel.rs`). Nothing to remove.
3. **Git surfaces (3, all kept):** ✅ Keep all three, sharpen roles — Manifest =
   overview, Graph = deep history, History tab stays as-is. (Rejected:
   collapsing the History tab into a jump-to-Graph shortcut.) **DOCS-ONLY** —
   positioning already reflected (tour stop 2, Manifest caption). No code.
4. **Naming:** ✅ Deferred. Keep plain "Providers"/"Agents" for now; revisit
   nautical renames (Quiver / Fleet) only when restyling actually touches each
   surface.

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
