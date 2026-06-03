//! Rust binding for the vendored Dockerfile grammar.
//!
//! Exposes the parser through `tree-sitter-language`'s [`LanguageFn`], which is
//! independent of any specific `tree-sitter` runtime version and converts into
//! the workspace `tree_sitter::Language` via `LANGUAGE.into()`.

use tree_sitter_language::LanguageFn;

unsafe extern "C" {
    fn tree_sitter_dockerfile() -> *const ();
}

/// The tree-sitter [`LanguageFn`] for Dockerfile.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_dockerfile) };
