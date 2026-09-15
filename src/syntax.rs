//! Extra Tree-sitter grammars for the editor.
//!
//! gpui-component registers 37 grammars; every other file fell back to plain
//! text. Highlighting is resolved by name through `LanguageRegistry`, and that
//! registry is public, so more grammars can be added here without patching the
//! component. This table adds 23 more.
//!
//! Two built-ins are also replaced: `.tsx` shipped with only the TypeScript
//! overlay query (no JS keywords, strings, or JSX tags), and `.json` treated
//! every bracket and comma as the same punctuation. The replacements live in
//! `src/queries/`.
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

use folio::buffer;
use gpui::HighlightStyle;
use gpui_component::{
    Rope,
    highlighter::{HighlightTheme, LanguageConfig, LanguageRegistry, SyntaxHighlighter},
};
use std::{collections::HashMap, ops::Range, path::Path};

/// Tree-sitter colors for each 1-based line of `text`, using the same
/// grammar the editor would pick for `path`. Empty when the file is plain
/// text, too large, or has no grammar.
pub fn line_styles(
    path: &Path,
    text: &str,
    theme: &HighlightTheme,
) -> HashMap<u32, Vec<(Range<usize>, HighlightStyle)>> {
    if text.is_empty() || text.len() > buffer::HIGHLIGHT_LIMIT {
        return HashMap::new();
    }
    let language = buffer::language(path);
    if language == "text" {
        return HashMap::new();
    }
    let mut highlighter = SyntaxHighlighter::new(language);
    highlighter.update(None, &Rope::from_str(text), None);
    assign_styles_to_lines(text, &highlighter.styles(&(0..text.len()), theme))
}

/// Map file-absolute highlight ranges onto each source line, so a diff
/// row can paint without knowing the rest of the file.
fn assign_styles_to_lines(
    text: &str,
    styles: &[(Range<usize>, HighlightStyle)],
) -> HashMap<u32, Vec<(Range<usize>, HighlightStyle)>> {
    let mut map: HashMap<u32, Vec<(Range<usize>, HighlightStyle)>> = HashMap::new();
    let mut byte = 0;
    let lines: Vec<_> = text
        .split_inclusive('\n')
        .map(|line| {
            let start = byte;
            byte += line.len();
            start..start + line.trim_end_matches(['\r', '\n']).len()
        })
        .collect();
    // Iterate each style only over the lines it intersects. Preserves style
    // order even when the highlighter returns overlapping or unsorted ranges.
    for (range, style) in styles {
        let first = lines.partition_point(|line| line.end <= range.start);
        for (index, line) in lines.iter().enumerate().skip(first) {
            if line.start >= range.end {
                break;
            }
            let lo = range.start.max(line.start);
            let hi = range.end.min(line.end);
            if lo < hi {
                map.entry(index as u32 + 1)
                    .or_default()
                    .push((lo - line.start..hi - line.start, *style));
            }
        }
    }
    map
}

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
    replace_queries(
        registry,
        "tsx",
        include_str!("queries/tsx.scm"),
        include_str!("queries/javascript-injections.scm"),
    );
    replace_queries(registry, "json", include_str!("queries/json.scm"), "");
}

/// Swap the highlight (and optionally injection) query of a built-in
/// language. The grammar stays the one gpui-component already registered.
fn replace_queries(registry: &LanguageRegistry, name: &str, highlights: &str, injections: &str) {
    let Some(mut config) = registry.language(name) else {
        return;
    };
    config.highlights = highlights.to_string().into();
    if !injections.is_empty() {
        config.injections = injections.to_string().into();
    }
    registry.register(name, &config);
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
    fn styles_land_on_the_line_they_cover() {
        let color = HighlightStyle {
            color: Some(gpui::hsla(0.5, 0.5, 0.5, 1.0)),
            ..Default::default()
        };
        // "ab\ncd" — paint the `b` and the `c`.
        let styles = assign_styles_to_lines("ab\ncd", &[(1..2, color), (3..4, color)]);
        assert_eq!(styles[&1], vec![(1..2, color)]);
        assert_eq!(styles[&2], vec![(0..1, color)]);
        let styles =
            assign_styles_to_lines("ab\r\n\ncd", &[(5..7, color), (1..6, color), (4..4, color)]);
        assert_eq!(styles[&1], vec![(1..2, color)]);
        assert!(!styles.contains_key(&2));
        assert_eq!(styles[&3], vec![(0..2, color), (0..1, color)]);

        // A realistic diff must not compare every highlight to every line.
        let text = "let x = 12;\n".repeat(20_000);
        let ranges: Vec<_> = (0..20_000)
            .flat_map(|line| {
                [0..3, 4..5, 6..7, 8..10].map(|r| (r.start + line * 12..r.end + line * 12, color))
            })
            .collect();
        let started = std::time::Instant::now();
        let styles = assign_styles_to_lines(&text, &ranges);
        eprintln!("20k lines / 80k highlights: {:?}", started.elapsed());
        assert_eq!(styles.len(), 20_000);
        assert!(styles.values().all(|line| line.len() == 4));
    }

    #[test]
    fn a_rust_file_gets_keyword_colors() {
        register_extra_languages();
        let styles = line_styles(
            Path::new("main.rs"),
            "fn main() {}\n",
            &HighlightTheme::default_dark(),
        );
        let line = &styles[&1];
        assert!(
            line.iter().any(|(range, style)| {
                range.start == 0 && range.end == 2 && style.color.is_some()
            }),
            "`fn` should be a colored keyword, got {line:?}"
        );
    }

    fn colored(styles: &[(std::ops::Range<usize>, HighlightStyle)], start: usize, end: usize) {
        assert!(
            styles.iter().any(|(range, style)| range.start == start
                && range.end == end
                && style.color.is_some()),
            "expected a color on {start}..{end}, got {styles:?}"
        );
    }

    #[test]
    fn a_tsx_file_highlights_keywords_and_tags() {
        register_extra_languages();
        assert_eq!(
            SyntaxHighlighter::new("tsx").language().as_ref(),
            "tsx",
            "the replacement query must compile"
        );
        let source = "const App = () => <div />;\n";
        let styles = line_styles(
            Path::new("app.tsx"),
            source,
            &HighlightTheme::default_dark(),
        );
        let line = &styles[&1];
        colored(line, 0, 5);
        let tag = source.find("div").expect("div");
        colored(line, tag, tag + 3);
        let ty = source.find("App").expect("App");
        colored(line, ty, ty + 3);
    }

    #[test]
    fn a_json_file_colors_keys_apart_from_values() {
        register_extra_languages();
        assert_eq!(
            SyntaxHighlighter::new("json").language().as_ref(),
            "json",
            "the replacement query must compile"
        );
        let source = "{\"n\": 1}\n";
        let styles = line_styles(Path::new("a.json"), source, &HighlightTheme::default_dark());
        let line = &styles[&1];
        colored(line, 1, 4);
        colored(line, 6, 7);
    }

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
            "a.json",
            "a.tsx",
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
