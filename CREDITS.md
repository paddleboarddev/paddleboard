# Credits & Third-Party Attributions

PaddleBoard stands on a great deal of open-source work. This file credits
everything PaddleBoard **bundles, downloads, derives from, or drives** — the
projects that are *not* Cargo dependencies and so aren't captured by the
automatic Rust dependency manifest. Our thanks to all of these authors.

Sections are ordered by how close the work travels to you: things shipped inside
the app first, then what it downloads on request, then the tools it drives but
never installs.

## Built on Zed

PaddleBoard is a fork of the [Zed editor](https://github.com/zed-industries/zed)
by Zed Industries. See the [README](README.md) for the fork relationship and
upstream licensing.

## Rust dependencies

Every Rust crate PaddleBoard depends on is attributed in
[`assets/licenses.md`](assets/licenses.md), generated automatically by
[`cargo-about`](https://github.com/EmbarkStudios/cargo-about) on each build.

## Bundled inside PaddleBoard

Shipped in the application bundle — PaddleBoard **does** redistribute these, so
their licenses travel with every download:

| Project | Used for | Author | License |
|---------|----------|--------|---------|
| [llama.cpp](https://github.com/ggml-org/llama.cpp) (pinned `b9874`) | The inference server behind **Local Models** — the `llama-server` binary and its libraries | ggml.ai and contributors | MIT |
| [Lilex](https://github.com/mishamyrt/Lilex) | PaddleBoard's monospace UI/editor face (`.ZedMono` resolves to it) — `assets/fonts/lilex/` | Misha Myrt | [OFL-1.1](assets/fonts/lilex/OFL.txt) |
| [IBM Plex Sans](https://github.com/IBM/plex) | PaddleBoard's UI sans face (`.ZedSans` resolves to it) — `assets/fonts/ibm-plex-sans/` | IBM | [OFL-1.1](assets/fonts/ibm-plex-sans/license.txt) |
| [dugite-native](https://github.com/desktop/dugite-native) | The `git` binary shipped at `Contents/MacOS/git`, so git works without a system install | GitHub / the Git project | GPL-2.0 |

## Design

| Project | Used for | Author | License |
|---------|----------|--------|---------|
| [Catppuccin](https://github.com/catppuccin/catppuccin) | The **PaddleBoard Dark / Light** themes are derived from Catppuccin Mocha and Latte — the palette and syntax-token conventions carry through to the app, the website, and the docs | Catppuccin contributors | MIT — full text at [`assets/themes/paddleboard/LICENSE-catppuccin`](assets/themes/paddleboard/LICENSE-catppuccin), and the derivation is recorded in the theme's own `author` field |

## Models (downloaded on request)

**Local Models** and **semantic search** download these only when you ask for
them, from Hugging Face. PaddleBoard does not redistribute model weights.

⚠️ Gemma models are released under the [Gemma Terms of Use](https://ai.google.dev/gemma/terms)
rather than an OSI-approved open-source licence, and that licence carries use
restrictions. Read it before deploying anything built on them.

| Model | Used for | Author | Terms |
|-------|----------|--------|-------|
| [Gemma 3 4B / 1B](https://ai.google.dev/gemma) (`unsloth/gemma-3-*-it-GGUF`) | The managed local chat models | Google DeepMind; GGUF conversions by [Unsloth](https://github.com/unslothai/unsloth) | Gemma Terms of Use |
| [EmbeddingGemma 300M](https://ai.google.dev/gemma) (`ggml-org/embeddinggemma-300m-qat-q8_0-GGUF`) | On-device embeddings for local semantic search | Google DeepMind; GGUF conversion by ggml.ai | Gemma Terms of Use |

## Tools PaddleBoard drives (installed by you)

PaddleBoard detects, launches, or scaffolds these. It does **not** bundle or
install them — each stays on its own release cadence and you install it
yourself — but PaddleBoard's features are built on their work:

| Project | Used for | Author | License |
|---------|----------|--------|---------|
| [Podman](https://podman.io/) | Container engine backing the agent sandbox | Containers org | Apache-2.0 |
| [gVisor](https://gvisor.dev/) (`runsc`) | The kernel runtime that isolates sandboxed agent code | Google | Apache-2.0 |
| [libkrun](https://github.com/containers/libkrun) | The built-in microVM sandbox tier — loaded via `dlopen`, not linked | Containers org | Apache-2.0 |
| [Scion](https://github.com/GoogleCloudPlatform/scion) | Optional multi-agent orchestration (`paddleboard_scion.enabled`) | Google Cloud Platform | Apache-2.0 |
| [s8sskills](https://s8sskills.com) | The serverless-deploy skill packs behind **Set Sail** | s8sskills | Open catalog |
| [Google ADK](https://github.com/google/adk-python) | Agent-framework scaffolding and dev server | Google | Apache-2.0 |
| [LangGraph](https://github.com/langchain-ai/langgraph) | Agent-framework scaffolding | LangChain | MIT |
| [CrewAI](https://github.com/crewAIInc/crewAI) | Agent-framework scaffolding | crewAI Inc | MIT |
| [AutoGen](https://github.com/microsoft/autogen) / AutoGen Studio | Agent-framework scaffolding | Microsoft | MIT (code; docs CC-BY-4.0) |
| [Unsloth](https://github.com/unslothai/unsloth) | The fine-tuning environment behind **Open Unsloth** | Unsloth AI | Apache-2.0 |

## Language & prose servers (downloaded at runtime)

PaddleBoard fetches these from each project's own releases (or package registry)
the first time you open a matching file. PaddleBoard does **not** redistribute
them — it points you at the upstream artifacts — but we gratefully credit them:

| Tool | Used for | Author | License |
|------|----------|--------|---------|
| [Harper](https://github.com/Automattic/harper) (`harper-ls`) | Spell & grammar checking (Markdown, commit messages) | Automattic | Apache-2.0 |
| [dockerfile-language-server-nodejs](https://github.com/rcjsuen/dockerfile-language-server-nodejs) (`docker-langserver`) | Dockerfile language server | Remy Suen | MIT |
| [kotlin-language-server](https://github.com/fwcd/kotlin-language-server) | Kotlin language server | fwcd | MIT |
| [Eclipse JDT Language Server](https://github.com/eclipse-jdtls/eclipse.jdt.ls) (`jdtls`) | Java language server | Eclipse Foundation | EPL-2.0 |
| [SourceKit-LSP](https://github.com/swiftlang/sourcekit-lsp) | Swift language server | swiftlang | Apache-2.0 |
| [Roslyn](https://github.com/dotnet/roslyn) | C# language server | Microsoft | MIT |
| [clangd](https://github.com/llvm/llvm-project) | C / C++ language server | LLVM Project | Apache-2.0 WITH LLVM-exception |
| [Ruff](https://github.com/astral-sh/ruff) | Python linting / formatting | Astral | MIT |
| [ty](https://github.com/astral-sh/ty) | Python type checking | Astral | MIT |
| [basedpyright](https://github.com/DetachHead/basedpyright) | Python language server | DetachHead | MIT |
| [Pyright](https://github.com/microsoft/pyright) | Python language server | Microsoft | MIT |
| [intelephense](https://intelephense.com/) | PHP language server | Ben Mewburn | Proprietary (free tier; not redistributed) |

## Vendored source

Code we ship inside this repository, with its original license retained:

| Source | Used for | Author | License |
|--------|----------|--------|---------|
| [tree-sitter-dockerfile](https://github.com/camdencheek/tree-sitter-dockerfile) | Dockerfile grammar — C source vendored under `crates/tree-sitter-dockerfile/` | Camden Cheek | MIT (full text at [`crates/tree-sitter-dockerfile/LICENSE`](crates/tree-sitter-dockerfile/LICENSE)) |

---

Spotted a project we integrate but don't credit here, or an inaccurate license?
Please open an issue — we want this list complete and correct.
