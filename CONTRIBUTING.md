# Contributing to PaddleBoard

PaddleBoard is a small, alpha-stage fork of [Zed](https://github.com/zed-industries/zed). Issues and PRs are welcome, but please scope-check anything bigger than a small fix before you spend a lot of time on it.

## Before you start

A few conventions encode most of how the project works:

- **Code style.** Prioritize correctness and clarity. Avoid panics (`unwrap()`, out-of-bounds indexing) — propagate errors with `?`, and never silently discard them with `let _ =`. Use full words for names, and don't add comments that just restate the code.
- **Fork hygiene.** Net-new features go in their own `paddleboard_*` crate so upstream can't conflict on a file it doesn't have. When you must edit a shared/upstream-shaped file, keep the diff minimal and tag the change with a `// PaddleBoard: <reason>` comment so future weekly merges stay mechanical. Watch for silent drift in `assets/` (settings, keymaps, icons).
- [`WELCOME.md`](./WELCOME.md) — what features PaddleBoard adds on top of Zed, and which subsystems are fork-specific vs. inherited from upstream.

<<<<<<< HEAD
## What's likely to land

- Bug fixes in PaddleBoard-specific code (any `paddleboard_*` crate, or any shared file with a `// PaddleBoard:` tag).
- Small enhancements to the sandbox, browser panel, orchestration panel, sandboxed MCP transport, step-through mode, or LLM picker.
- Docs improvements — especially anything that makes the fork hygiene rules clearer.
- Tightening the upstream merge process (`.github/workflows/merge_upstream_zed.yml`).
=======
In particular **we love PRs that are**:

- Fixing or extending the **docs**.
- Fixing **bugs**.
- **Small** enhancements to existing features to **make them work for more people** (making things work on more platforms/modes/whatever).
- **Small** extra features, like keybindings or actions you miss from other editors or extensions.
- Part of a **Community Program** like [Let's Git Together](https://github.com/zed-industries/zed/issues/41541) or [The Guild](https://zed.dev/community/guild).
- Features we **explicitly called out as open to community contributions**
>>>>>>> zed/main

## What probably won't land

<<<<<<< HEAD
- Anything that would belong upstream. File the PR against [`zed-industries/zed`](https://github.com/zed-industries/zed/pulls) instead — we'd just inherit it on the next weekly merge.
- New themes, language extensions, or icon themes — those go through Zed's extension system.
- Telemetry, sponsorship, or upsell code, even if upstream re-introduces it — those are deliberately inert.
- AI-generated code where the author can't explain what it does.

## Sending a PR
=======
- [Docs issues](https://github.com/zed-industries/zed/issues?q=is%3Aissue%20state%3Aopen%20type%3ADocs)
- Issues suitable for [first-time contributors](https://github.com/zed-industries/zed/issues?q=is%3Aissue%20state%3Aopen%20label%3A%22.contrib%2Fgood%20first%20issue%22), [returning contributors](https://github.com/zed-industries/zed/issues?q=is%3Aissue%20state%3Aopen%20label%3A%22.contrib%2Fgood%20second%20issue%22), and [expert contributors](https://github.com/zed-industries/zed/issues?q=is%3Aissue%20state%3Aopen%20label%3A%22.contrib%2Fgood%20expert%20issue%22)
- [Triaged bugs with confirmed steps to reproduce](https://github.com/zed-industries/zed/issues?q=is%3Aissue%20state%3Aopen%20type%3ABug%20label%3Astate%3Areproducible)
- [Area labels](https://github.com/zed-industries/zed/labels?q=area%3A*) to browse bugs in a specific part of the product you care about (after clicking on an area label, add type:Bug to the search)
- [The board with the features](https://github.com/orgs/zed-industries/projects/78/views/4) we explicitly invited the community's contributions for.

**Thinking about proposing or building a larger feature? Don't start with a PR**, start with reading the [Zed Feature Process](./docs/src/development/feature-process.md) for how we think about feature design — what context to provide, what integration points to consider, and how to put together a strong proposal. The right place for the proposals is [GitHub discussions](https://github.com/zed-industries/zed/discussions) (not GitHub issues).
>>>>>>> zed/main

- Branch off `main`, push to a topic branch, open a PR. Title is a clear imperative sentence — no `feat:` / `fix:` prefixes, no trailing punctuation.
- Include a `Release Notes:` section at the bottom of the PR body. Either one bullet (`- Added …` / `- Fixed …` / `- Improved …`) for user-facing changes, or `- N/A` for everything else.
- For UI changes, attach a screenshot or short screen recording.

## Working on the upstream surface area

<<<<<<< HEAD
The bulk of the codebase — editor, LSP, debugger, GPUI, terminal, vim mode, language tooling, extensions — is inherited from Zed and merged in weekly. For anything in that surface area, [Zed's documentation](https://zed.dev/docs) and the [upstream `CONTRIBUTING.md`](https://github.com/zed-industries/zed/blob/main/CONTRIBUTING.md) are the canonical references. PaddleBoard inherits Zed's UI/UX standards by default; the upstream UI/UX checklist applies here too.

When you edit a shared file, keep the diff minimal and tag the change with a `// PaddleBoard:` comment so future merge resolution stays mechanical.
=======
The best way to get us to take a look at a proposed change (excluding new features) is to send a pull request. We will get back to you (though this sometimes takes longer than we'd like, sorry). **Pinging the maintainers by their username or writing them emails spends their time but does not bump the priority of a particular PR.**

If you need more feedback from us: the best way is to be responsive to
GitHub comments, or to offer up time to pair with us.

If you need help deciding how to fix a bug, or finish implementing a feature
that we've agreed we want, please open a PR early so we can discuss how to make
the change with code in hand.

Although we will take a look, we tend to only merge about half the PRs that are
submitted. **If you'd like your PR to have the best chance of being merged**:

- Make sure the change is **desired**: we're always happy to accept bugfixes,
  but **features should be confirmed with us first** if you aim to avoid wasted
  effort. If there isn't already a GitHub issue for your feature with staff
  confirmation that we want it, start with a [GitHub discussion](https://github.com/zed-industries/zed/discussions) rather than a PR.
  - This especially applies to any changes proposed to the Zed Extension API.
- Include a clear description of **what you're solving**, and why it's important.
- Include **tests**. For UI changes, consider updating visual regression tests (see [Building Zed for macOS](./docs/src/development/macos.md#visual-regression-tests)).
- If the change is visible in the UI, attach **screenshots or screen recordings**.
- Make the PR about **one thing only**, e.g. if it's a bugfix, don't add two
  features and a refactoring on top of that.
- Keep AI assistance under your judgement and responsibility: it's unlikely
  we'll merge a vibe-coded PR that the author doesn't understand.


## Things we will (probably) not merge

Although there are few hard and fast rules, **typically we don't merge**:

- Anything that can be provided by an extension. For example a new language, or theme. For adding themes or support for a new language to Zed, check out our [docs on developing extensions](https://zed.dev/docs/extensions/developing-extensions).
- Changes to the Zed Extension API submitted without prior discussion involving Zed staff.
- New file icons. Zed's default icon theme consists of icons that are hand-designed to fit together in a cohesive manner, please don't submit PRs with off-the-shelf SVGs.
- Features where (in our subjective opinion) the extra complexity isn't worth it for the number of people who will benefit.
- Giant refactorings.
- Non-trivial changes with no tests.
- Stylistic code changes that do not alter any app logic. Reducing allocations, removing `.unwrap()`s, fixing typos is great; making code "more readable" — maybe not so much.
- Anything that seems AI-generated without understanding the output.

### AI Policy

We welcome the use of LLMs for coding, but we hold a high bar for all contributions, and **we expect a human in the loop who genuinely understands the work an LLM produces** on their behalf. For that reason, we **don't accept contributions from autonomous agents**. Pull requests that appear to violate this may be closed, sometimes without notice.

**Don't rely on LLMs to write the whole thing for you when communicating with the maintainers** (meaning replies to comments, PR descriptions, and alike). The readers are humans, and we'd like to hear from you, not from a model (we have models at home). If you're a non-native English speaker using an LLM to thoroughly edit or translate your messages to the maintainers, we'd encourage you to **put the machine translation in a quote block and include the original text in your native language after it**.

If you think it's helpful/necessary to **share context from a chat with an LLM**, please put the **relevant part of it** in a quote block (e.g., using `>`), **disclose it as AI-generated**, and add your own commentary explaining **why it's relevant and what you take from it**.

This policy was adapted from [ripgrep's AI policy](https://github.com/BurntSushi/ripgrep/blob/f0cec341ab95c25c691ad3d5754d4bd9eedde21f/AI_POLICY.md).

### Internal advice for reviewers

- If the fix/feature is obviously great, and the code is great. Hit merge.
- If the fix/feature is obviously great, and the code is nearly great. Send PR comments, or offer to pair to get things perfect.
- If the fix/feature is not obviously great, or the code needs rewriting from scratch. Close the PR with a thank you and some explanation.

### UI/UX checklist

When your changes affect UI, consult this checklist:

**Accessibility / Ergonomics**

- Do all keyboard shortcuts work as intended?
- Are shortcuts discoverable (tooltips, menus, docs)?
- Is it usable without a mouse (keyboard-only navigation)?
- Do all mouse actions work (drag, context menus, resizing, scrolling)?
- Does the feature look great in light and dark mode themes?
- Are hover states and focus indicators clear and consistent?

**Responsiveness**

- Does the UI scale gracefully on:
  - Narrow panes (e.g., side-by-side split views)?
  - Short panes (e.g., laptops with 13" displays)?
  - High-DPI / Retina displays?
- Does resizing panes or windows keep the UI usable and attractive?
- Do dialogs or modals stay centered and within viewport bounds?

**Platform Consistency**

- Is the feature fully usable on Windows, Linux, and macOS?
- Does it respect system-level settings (fonts, scaling, input methods)?

**Performance**

- All user interactions must have instant feedback.
  - If the user requests something slow (e.g. an LLM generation) there should be some indication of the work in progress.
- Does it handle large files, big projects, or heavy workloads without degrading?
- Frames must take no more than 8ms (120fps)

**Consistency**

- Does it match Zed’s design language (spacing, typography, icons)?
  - Make sure to visit [the icon design guidelines](https://github.com/zed-industries/zed/blob/main/crates/icons/README.md)
- Are terminology, labels, and tone consistent with the rest of Zed?
- Are interactions consistent (e.g., how tabs close, how modals dismiss, how errors show)?

**Internationalization & Text**

- Are strings concise, clear, and unambiguous?
- Do we avoid internal Zed jargon that only insiders would know?

**User Paths & Edge Cases**

- What does the happy path look like?
- What does the unhappy path look like? (errors, rejections, invalid states)
- How does it work in offline vs. online states?
- How does it work in unauthenticated vs. authenticated states?
- How does it behave if data is missing, corrupted, or delayed?
- Are error messages actionable and consistent with Zed’s voice?

**Discoverability & Learning**

- Can a first-time user figure it out without docs?
- Is there an intuitive way to undo/redo actions?
- Are power features discoverable but not intrusive?
- Is there a path from beginner → expert usage (progressive disclosure)?

## Bird's-eye view of Zed

We suggest you keep the [Zed glossary](docs/src/development/glossary.md) at your side when starting out. It lists and explains some of the structures and terms you will see throughout the codebase.

Zed is made up of several smaller crates - let's go over those you're most likely to interact with:

- [`gpui`](/crates/gpui) is a GPU-accelerated UI framework which provides all of the building blocks for Zed. **We recommend familiarizing yourself with the root level GPUI documentation.**
- [`editor`](/crates/editor) contains the core `Editor` type that drives both the code editor and all various input fields within Zed. It also handles a display layer for LSP features such as Inlay Hints or code completions.
- [`project`](/crates/project) manages files and navigation within the filetree. It is also Zed's side of communication with LSP.
- [`workspace`](/crates/workspace) handles local state serialization and groups projects together.
- [`vim`](/crates/vim) is a thin implementation of Vim workflow over `editor`.
- [`lsp`](/crates/lsp) handles communication with external LSP server.
- [`language`](/crates/language) drives `editor`'s understanding of language - from providing a list of symbols to the syntax map.
- [`collab`](/crates/collab) is the collaboration server itself, driving the collaboration features such as project sharing.
- [`rpc`](/crates/rpc) defines messages to be exchanged with collaboration server.
- [`theme`](/crates/theme) defines the theme system and provides a default theme.
- [`ui`](/crates/ui) is a collection of UI components and common patterns used throughout Zed.
- [`cli`](/crates/cli) is the CLI crate which invokes the Zed binary.
- [`zed`](/crates/zed) is where all things come together, and the `main` entry point for Zed.

## Packaging Zed

Check our [notes for packaging Zed](https://zed.dev/docs/development/linux#notes-for-packaging-zed).
>>>>>>> zed/main
