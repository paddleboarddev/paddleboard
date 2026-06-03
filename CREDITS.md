# Credits & Third-Party Attributions

PaddleBoard stands on a great deal of open-source work. This file credits the
projects PaddleBoard **integrates or downloads at runtime** — the ones that are
*not* Cargo dependencies and so aren't captured by the automatic Rust dependency
manifest. Our thanks to all of these authors.

## Built on Zed

PaddleBoard is a fork of the [Zed editor](https://github.com/zed-industries/zed)
by Zed Industries. See the [README](README.md) for the fork relationship and
upstream licensing.

## Rust dependencies

Every Rust crate PaddleBoard depends on is attributed in
[`assets/licenses.md`](assets/licenses.md), generated automatically by
[`cargo-about`](https://github.com/EmbarkStudios/cargo-about) on each build.

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
