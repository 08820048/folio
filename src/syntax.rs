//! Extra Tree-sitter grammars for the editor.
//!
//! gpui-component registers 37 grammars; every other file fell back to plain
//! text. Highlighting is resolved by name through `LanguageRegistry`, and that
//! registry is public, so more grammars can be added here without patching the
//! component. This table adds 23 more.
//!
//! `folio::buffer::language` returns exactly the names registered below, and the
//! test at the bottom proves each one actually compiles its highlight query.
//!
//! Adding a grammar is only possible when all of the following hold, so the
//! candidates were checked individually:
//!
//! - the grammar speaks the `tree-sitter-language` 0.1 ABI of the `tree-sitter`
//!   0.26 that gpui-component pins. Grammars still built against tree-sitter
//!   0.20 (`tree-sitter-dockerfile`, `tree-sitter-vue`, `tree-sitter-wgsl`,
//!   `tree-sitter-cue`) return a foreign `Language` type and cannot be used;
//! - the crate exports its highlight query as a `&str` const. Plenty of packages
//!   ship `queries/highlights.scm` without re-exporting it (`tree-sitter-glsl`,
//!   `tree-sitter-nickel`), leave the `pub const` commented out
//!   (`tree-sitter-groovy`, `tree-sitter-nix`'s locals) or ship no query at all
//!   (`tree-sitter-perl`), and would need the query vendored into this
//!   repository;
//! - the crate's build script actually enables the cfg that gates the const.
//!   `tree-sitter-vue-next` looks correct but its build script probes
//!   `queries/highlights.scm` while the file lives at `queries/vue/highlights.scm`,
//!   so the const never exists.
//!
//! Grammar tables are linked into the binary as static data and are only parsed
//! when a matching file is opened, so this costs startup, memory and scroll
//! performance nothing. It does cost disk: the release binary grows from ~21 MB
//! to ~73 MB. Measured per grammar (object code, KB):
//!
//! | grammar | KB | grammar | KB | grammar | KB |
//! | --- | ---: | --- | ---: | --- | ---: |
//! | ocaml | 13796 | dart | 3356 | r | 980 |
//! | objc | 11404 | vim | 3352 | erlang | 868 |
//! | haskell | 8064 | powershell | 2076 | gleam | 692 |
//! | gitcommit | 4152 | solidity | 1100 | rest | < 400 |
//!
//! Drop an entry here and its dependency in `Cargo.toml` to reclaim the space;
//! `folio::buffer::language` will then fall back to `text` for those extensions.
//! Known limitation: `.mli` is highlighted with the OCaml implementation grammar,
//! because the crate shares one query that does not compile against the
//! interface grammar.

use gpui_component::highlighter::{LanguageConfig, LanguageRegistry};

/// A grammar and the Tree-sitter queries that highlight it.
struct Grammar {
    /// Registered name, also returned by `folio::buffer::language`.
    name: &'static str,
    language: tree_sitter::Language,
    highlights: &'static str,
    injections: &'static str,
    locals: &'static str,
}

fn grammars() -> Vec<Grammar> {
    use Grammar as G;
    macro_rules! grammar {
        ($name:literal, $language:expr, $highlights:expr) => {
            grammar!($name, $language, $highlights, "", "")
        };
        ($name:literal, $language:expr, $highlights:expr, $injections:expr) => {
            grammar!($name, $language, $highlights, $injections, "")
        };
        ($name:literal, $language:expr, $highlights:expr, $injections:expr, $locals:expr) => {
            G {
                name: $name,
                language: $language,
                highlights: $highlights,
                injections: $injections,
                locals: $locals,
            }
        };
    }
    vec![
        // Markup and data formats
        grammar!(
            "xml",
            tree_sitter_xml::LANGUAGE_XML.into(),
            tree_sitter_xml::XML_HIGHLIGHT_QUERY
        ),
        grammar!(
            "dtd",
            tree_sitter_xml::LANGUAGE_DTD.into(),
            tree_sitter_xml::DTD_HIGHLIGHT_QUERY
        ),
        grammar!(
            "ini",
            tree_sitter_ini::LANGUAGE.into(),
            tree_sitter_ini::HIGHLIGHTS_QUERY
        ),
        // Infrastructure and shells
        grammar!(
            "dockerfile",
            tree_sitter_containerfile::LANGUAGE.into(),
            tree_sitter_containerfile::HIGHLIGHTS_QUERY,
            tree_sitter_containerfile::INJECTIONS_QUERY
        ),
        grammar!(
            "nix",
            tree_sitter_nix::LANGUAGE.into(),
            tree_sitter_nix::HIGHLIGHTS_QUERY,
            tree_sitter_nix::INJECTIONS_QUERY
        ),
        grammar!(
            "powershell",
            tree_sitter_powershell::LANGUAGE.into(),
            tree_sitter_powershell::HIGHLIGHTS_QUERY
        ),
        grammar!(
            "fish",
            tree_sitter_fish::language(),
            tree_sitter_fish::HIGHLIGHTS_QUERY
        ),
        grammar!(
            "vim",
            tree_sitter_vim::language(),
            tree_sitter_vim::HIGHLIGHTS_QUERY,
            tree_sitter_vim::INJECTIONS_QUERY
        ),
        grammar!(
            "gitcommit",
            tree_sitter_gitcommit::LANGUAGE.into(),
            tree_sitter_gitcommit::HIGHLIGHTS_QUERY,
            tree_sitter_gitcommit::INJECTIONS_QUERY
        ),
        grammar!(
            "regex",
            tree_sitter_regex::LANGUAGE.into(),
            tree_sitter_regex::HIGHLIGHTS_QUERY
        ),
        // Application languages
        grammar!(
            "vue",
            tree_sitter_vue_updated::language(),
            tree_sitter_vue_updated::HIGHLIGHTS_QUERY,
            tree_sitter_vue_updated::INJECTIONS_QUERY
        ),
        grammar!(
            "dart",
            tree_sitter_dart::LANGUAGE.into(),
            tree_sitter_dart::HIGHLIGHTS_QUERY,
            "",
            tree_sitter_dart::LOCALS_QUERY
        ),
        grammar!(
            "r",
            tree_sitter_r::LANGUAGE.into(),
            tree_sitter_r::HIGHLIGHTS_QUERY,
            "",
            tree_sitter_r::LOCALS_QUERY
        ),
        grammar!(
            "objc",
            tree_sitter_objc::LANGUAGE.into(),
            tree_sitter_objc::HIGHLIGHTS_QUERY,
            tree_sitter_objc::INJECTIONS_QUERY,
            tree_sitter_objc::LOCALS_QUERY
        ),
        grammar!(
            "asm",
            tree_sitter_asm::LANGUAGE.into(),
            tree_sitter_asm::HIGHLIGHTS_QUERY
        ),
        grammar!(
            "solidity",
            tree_sitter_solidity::LANGUAGE.into(),
            tree_sitter_solidity::HIGHLIGHT_QUERY,
            "",
            tree_sitter_solidity::LOCALS_QUERY
        ),
        // Functionally-flavoured languages
        grammar!(
            "haskell",
            tree_sitter_haskell::LANGUAGE.into(),
            tree_sitter_haskell::HIGHLIGHTS_QUERY,
            tree_sitter_haskell::INJECTIONS_QUERY,
            tree_sitter_haskell::LOCALS_QUERY
        ),
        grammar!(
            "ocaml",
            tree_sitter_ocaml::LANGUAGE_OCAML.into(),
            tree_sitter_ocaml::HIGHLIGHTS_QUERY,
            "",
            tree_sitter_ocaml::LOCALS_QUERY
        ),
        // `ocaml_interface` / `ocaml_type` cannot be registered: the crate ships a
        // single `HIGHLIGHTS_QUERY` that matches on `shebang`, a node only the
        // implementation grammar has, so `Query::new` rejects it for the others.
        grammar!(
            "erlang",
            tree_sitter_erlang::LANGUAGE.into(),
            tree_sitter_erlang::HIGHLIGHTS_QUERY
        ),
        grammar!(
            "elm",
            tree_sitter_elm::LANGUAGE.into(),
            tree_sitter_elm::HIGHLIGHTS_QUERY,
            tree_sitter_elm::INJECTIONS_QUERY,
            tree_sitter_elm::LOCALS_QUERY
        ),
        grammar!(
            "gleam",
            tree_sitter_gleam::LANGUAGE.into(),
            tree_sitter_gleam::HIGHLIGHT_QUERY,
            "",
            tree_sitter_gleam::LOCALS_QUERY
        ),
        // Stylesheets
        grammar!(
            "scss",
            tree_sitter_scss::language(),
            tree_sitter_scss::HIGHLIGHTS_QUERY
        ),
        grammar!(
            "less",
            tree_sitter_less::language(),
            tree_sitter_less::HIGHLIGHTS_QUERY
        ),
    ]
}

/// Registers every extra grammar. Safe to call more than once.
pub fn register_extra_languages() {
    let registry = LanguageRegistry::singleton();
    for grammar in grammars() {
        registry.register(
            grammar.name,
            &LanguageConfig::new(
                grammar.name,
                grammar.language,
                // Injections resolve their own language by name at parse time,
                // so this list is informational only.
                vec![],
                grammar.highlights,
                grammar.injections,
                grammar.locals,
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_component::highlighter::SyntaxHighlighter;
    use std::path::Path;

    /// `SyntaxHighlighter::new` silently falls back to `text` when a query does
    /// not compile, which would leave a language looking "supported" but
    /// unhighlighted. Assert the round trip instead.
    #[test]
    fn every_registered_grammar_compiles_its_highlight_query() {
        register_extra_languages();
        // Registering twice must not disturb the registry.
        register_extra_languages();
        for grammar in grammars() {
            let highlighter = SyntaxHighlighter::new(grammar.name);
            assert_eq!(
                highlighter.language().as_ref(),
                grammar.name,
                "`{}` fell back to plain text, so its highlight query did not compile",
                grammar.name
            );
        }
    }

    /// Every name `buffer::language` can return must resolve in the registry,
    /// which is what keeps `src/buffer.rs` and this table in sync.
    #[test]
    fn buffer_mappings_resolve_to_a_registered_grammar() {
        register_extra_languages();
        for path in [
            "a.xml",
            "a.xsd",
            "a.plist",
            "a.csproj",
            "a.xaml",
            "a.dtd",
            "a.ini",
            "a.cfg",
            "a.properties",
            "a.service",
            "Dockerfile",
            "Dockerfile.dev",
            "Containerfile",
            "a.dockerfile",
            "a.nix",
            "a.ps1",
            "config.fish",
            "a.vim",
            ".vimrc",
            "COMMIT_EDITMSG",
            "a.regex",
            "a.vue",
            "a.dart",
            "a.r",
            ".Rprofile",
            "a.m",
            "a.mm",
            "a.s",
            "a.asm",
            "a.sol",
            "a.hs",
            "a.ml",
            "a.mli",
            "a.erl",
            "a.elm",
            "a.gleam",
            "a.scss",
            "a.sass",
            "a.less",
            "a.ipynb",
            "a.json5",
            "Makefile",
            "CMakeLists.txt",
            "Gemfile",
            ".bashrc",
            ".zshrc",
            ".editorconfig",
            ".gitconfig",
            "Cargo.lock",
            "Pipfile",
            ".env",
            ".env.local",
            "a.unknown-extension",
        ] {
            let name = folio::buffer::language(Path::new(path));
            assert!(
                LanguageRegistry::singleton().language(name).is_some(),
                "`{path}` maps to `{name}`, which is not a registered grammar"
            );
        }
    }
}
