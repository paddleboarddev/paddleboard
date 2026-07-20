use std::borrow::Cow;

use anyhow::Context as _;
use language_core::{LanguageConfig, LanguageQueries, QUERY_FILENAME_PREFIXES};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "src/"]
#[exclude = "*.rs"]
struct GrammarDir;

/// Register all built-in native tree-sitter grammars with the provided registration function.
///
/// Each grammar is registered as a `(&str, tree_sitter_language::LanguageFn)` pair.
/// This must be called before loading language configs/queries.
#[cfg(feature = "load-grammars")]
pub fn native_grammars() -> Vec<(&'static str, tree_sitter::Language)> {
    vec![
        ("bash", tree_sitter_bash::LANGUAGE.into()),
        ("c", tree_sitter_c::LANGUAGE.into()),
        ("cpp", tree_sitter_cpp::LANGUAGE.into()),
        ("css", tree_sitter_css::LANGUAGE.into()),
        ("diff", tree_sitter_diff::LANGUAGE.into()),
        // PaddleBoard: Dockerfile via vendored camdencheek/tree-sitter-dockerfile
        // (MIT). The published crate pins tree-sitter 0.24, so the C source is
        // vendored under crates/tree-sitter-dockerfile with a tree-sitter-language
        // binding that resolves against workspace tree-sitter 0.26.
        ("dockerfile", tree_sitter_dockerfile::LANGUAGE.into()),
        ("go", tree_sitter_go::LANGUAGE.into()),
        ("gomod", tree_sitter_go_mod::LANGUAGE.into()),
        ("gowork", tree_sitter_gowork::LANGUAGE.into()),
        // PaddleBoard: Java via canonical tree-sitter/tree-sitter-java (MIT).
        // The crate declares `tree-sitter ^0.24` only as a dev-dep (for
        // its own tests); its runtime API uses `tree-sitter-language`,
        // which resolves cleanly against workspace tree-sitter 0.26.
        ("java", tree_sitter_java::LANGUAGE.into()),
        ("jsdoc", tree_sitter_jsdoc::LANGUAGE.into()),
        ("json", tree_sitter_json::LANGUAGE.into()),
        ("jsonc", tree_sitter_json::LANGUAGE.into()),
        // PaddleBoard: Kotlin via `tree-sitter-kotlin-codanna` (MIT).
        // Picked specifically because (a) it's the only published Kotlin
        // grammar with a `tree-sitter >= 0.21` constraint open enough to
        // resolve against the workspace's `tree-sitter 0.26`, and
        // (b) it preserves the original `fwcd/tree-sitter-kotlin` node
        // shape (e.g. `simple_identifier`) that the upstream
        // `zed-extensions/kotlin` queries are written against — the
        // newer `tree-sitter-kotlin-ng` rewrite renamed those nodes
        // (`identifier`/`qualified_identifier`) and would require
        // rewriting every .scm we adopted. Uses the older `language()`
        // function API instead of the `LANGUAGE: LanguageFn` constant.
        ("kotlin", tree_sitter_kotlin_codanna::language()),
        ("markdown", tree_sitter_md::LANGUAGE.into()),
        ("markdown-inline", tree_sitter_md::INLINE_LANGUAGE.into()),
        // PaddleBoard: PHP via canonical tree-sitter/tree-sitter-php (MIT).
        // Like tree-sitter-java, its `tree-sitter ^0.25` dep is dev-only;
        // runtime uses `tree-sitter-language ^0.1` which resolves cleanly
        // against workspace `tree-sitter 0.26`. Uses `LANGUAGE_PHP` (the
        // mixed-mode `.php` grammar) — there's also a `LANGUAGE_PHP_ONLY`
        // for pure-PHP files but the standard `.php` registration uses
        // the mixed-mode grammar that handles inline HTML.
        ("php", tree_sitter_php::LANGUAGE_PHP.into()),
        ("python", tree_sitter_python::LANGUAGE.into()),
        ("regex", tree_sitter_regex::LANGUAGE.into()),
        ("rust", tree_sitter_rust::LANGUAGE.into()),
        // PaddleBoard: Swift via alex-pinkus/tree-sitter-swift (MIT).
        ("swift", tree_sitter_swift::LANGUAGE.into()),
        ("tsx", tree_sitter_typescript::LANGUAGE_TSX.into()),
        (
            "typescript",
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        ),
        ("yaml", tree_sitter_yaml::LANGUAGE.into()),
        ("gitcommit", tree_sitter_gitcommit::LANGUAGE.into()),
    ]
}

/// Load and parse the `config.toml` for a given language name.
pub fn load_config(name: &str) -> LanguageConfig {
    let config_toml = String::from_utf8(
        GrammarDir::get(&format!("{}/config.toml", name))
            .unwrap_or_else(|| panic!("missing config for language {:?}", name))
            .data
            .to_vec(),
    )
    .unwrap();

    let config: LanguageConfig = ::toml::from_str(&config_toml)
        .with_context(|| format!("failed to load config.toml for language {name:?}"))
        .unwrap();

    config
}

/// Load and parse the `config.toml` for a given language name, stripping fields
/// that require grammar support when grammars are not loaded.
pub fn load_config_for_feature(name: &str, grammars_loaded: bool) -> LanguageConfig {
    let config = load_config(name);

    if grammars_loaded {
        config
    } else {
        LanguageConfig {
            name: config.name,
            matcher: config.matcher,
            jsx_tag_auto_close: config.jsx_tag_auto_close,
            ..Default::default()
        }
    }
}

/// Get a raw embedded file by path (relative to `src/`).
///
/// Returns the file data as bytes, or `None` if the file does not exist.
pub fn get_file(path: &str) -> Option<rust_embed::EmbeddedFile> {
    GrammarDir::get(path)
}

// PaddleBoard: guard the vendored Dockerfile grammar — make sure the C source
// loads, the highlights query compiles against it (catches a node-name drift
// that would silently break highlighting), and a sample parses cleanly.
#[cfg(all(test, feature = "load-grammars"))]
mod dockerfile_tests {
    use super::*;

    #[test]
    fn dockerfile_grammar_and_highlights_load() {
        let language: tree_sitter::Language = tree_sitter_dockerfile::LANGUAGE.into();

        let queries = load_queries("dockerfile");
        let highlights = queries
            .highlights
            .expect("dockerfile highlights query should be present");
        tree_sitter::Query::new(&language, &highlights)
            .expect("dockerfile highlights query should compile against the grammar");

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&language)
            .expect("dockerfile grammar should load");
        let source = "FROM alpine:3.19 AS build\nRUN echo hi\nCOPY . /app\nCMD [\"/app/run\"]\n";
        let tree = parser.parse(source, None).expect("sample should parse");
        assert!(
            !tree.root_node().has_error(),
            "sample Dockerfile parsed with errors"
        );
    }
}

/// Load all `.scm` query files for a given language name into a `LanguageQueries`.
///
/// Multiple `.scm` files with the same prefix (e.g. `highlights.scm` and
/// `highlights_extra.scm`) are concatenated together with their contents appended.
pub fn load_queries(name: &str) -> LanguageQueries {
    let mut result = LanguageQueries::default();
    for path in GrammarDir::iter() {
        if let Some(remainder) = path.strip_prefix(name).and_then(|p| p.strip_prefix('/')) {
            if !remainder.ends_with(".scm") {
                continue;
            }
            for (prefix, query) in QUERY_FILENAME_PREFIXES {
                if remainder.starts_with(prefix) {
                    let contents = match GrammarDir::get(path.as_ref()).unwrap().data {
                        Cow::Borrowed(bytes) => Cow::Borrowed(std::str::from_utf8(bytes).unwrap()),
                        Cow::Owned(bytes) => Cow::Owned(String::from_utf8(bytes).unwrap()),
                    };
                    match query(&mut result) {
                        None => *query(&mut result) = Some(contents),
                        Some(existing) => existing.to_mut().push_str(contents.as_ref()),
                    }
                }
            }
        }
    }
    result
}
