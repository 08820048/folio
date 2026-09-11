//! `.editorconfig`, for the projects that ship one.
//!
//! Settings hold the indentation an editor uses when a file does not say; a
//! project with an `.editorconfig` says, per file. This reads those files and
//! nothing else.
//!
//! Only the properties that change how text is written are taken — the three
//! that decide indentation. `end_of_line`, `trim_trailing_whitespace` and
//! `insert_final_newline` describe edits to make when saving, which this editor
//! does not make, and are ignored rather than half-done.
//!
//! The format is the one EditorConfig defines: sections of glob patterns with
//! `key = value` pairs beneath them, the nearest file winning, and a file whose
//! `root` is true ending the search upward.

use std::{
    fs,
    path::{Path, PathBuf},
};

/// Indentation, as an editor wants it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Indent {
    pub columns: usize,
    pub hard_tabs: bool,
}

/// What the `.editorconfig` files above a path say about it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Rules {
    style: Option<Style>,
    size: Option<Size>,
    tab_width: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Style {
    Tabs,
    Spaces,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Size {
    Columns(usize),
    /// `indent_size = tab`: as wide as a tab, which is `tab_width`.
    Tab,
}

impl Rules {
    /// Read the `.editorconfig` files above `path`, nearest file winning.
    ///
    /// Nothing is cached: this runs when a file is opened, and it costs a
    /// handful of small reads up the directory tree.
    pub fn for_path(path: &Path) -> Rules {
        let Some(name) = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
        else {
            return Rules::default();
        };

        // Collected nearest first, then applied the other way round, which is
        // what makes the nearest file the one that wins.
        let mut configs: Vec<(PathBuf, Config)> = Vec::new();
        let mut directory = path.parent();
        while let Some(current) = directory {
            if let Ok(text) = fs::read_to_string(current.join(".editorconfig")) {
                let config = Config::parse(&text);
                let root = config.root;
                configs.push((current.to_path_buf(), config));
                if root {
                    break;
                }
            }
            directory = current.parent();
        }

        let mut rules = Rules::default();
        for (directory, config) in configs.iter().rev() {
            let relative = path.strip_prefix(directory).unwrap_or(path);
            let relative = relative.to_string_lossy().replace('\\', "/");
            config.apply(&mut rules, &relative, &name);
        }
        rules
    }

    /// The indentation to edit with: what the file asks for, and the settings
    /// for whatever it does not say.
    pub fn indent(&self, settings: Indent) -> Indent {
        let hard_tabs = match self.style {
            Some(Style::Tabs) => true,
            Some(Style::Spaces) => false,
            None => settings.hard_tabs,
        };
        let columns = match (self.size, self.tab_width) {
            (Some(Size::Columns(columns)), _) => columns,
            (Some(Size::Tab), Some(width)) => width,
            (Some(Size::Tab), None) | (None, _) => settings.columns,
        };
        // The bound the settings file is held to as well: a project asking for
        // a twenty-column indent is describing something this editor cannot
        // show, and the settings window's own range is the honest answer.
        let (min, max) = crate::settings::TAB_SIZE;
        Indent {
            columns: columns.clamp(min, max),
            hard_tabs,
        }
    }
}

/// One section of a file: the patterns it matches, and the properties written
/// beneath them.
type Section = (Vec<String>, Vec<(String, String)>);

/// One `.editorconfig` file.
#[derive(Debug, Default)]
struct Config {
    root: bool,
    /// Sections in the order they were written.
    sections: Vec<Section>,
}

impl Config {
    fn parse(text: &str) -> Config {
        let mut config = Config::default();

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }

            if let Some(section) = line
                .strip_prefix('[')
                .and_then(|line| line.strip_suffix(']'))
            {
                let patterns = section
                    .split(',')
                    .map(|pattern| pattern.trim().to_string())
                    .filter(|pattern| !pattern.is_empty())
                    .collect();
                config.sections.push((patterns, Vec::new()));
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim().to_ascii_lowercase();

            // Before any section, and about the file itself rather than about
            // the files it matches.
            if config.sections.is_empty() {
                if key == "root" {
                    config.root = value == "true";
                }
                continue;
            }
            if let Some((_, properties)) = config.sections.last_mut() {
                properties.push((key, value));
            }
        }

        config
    }

    /// Apply every section whose patterns match, in order: within one file the
    /// last matching property wins, which is what the order is for.
    fn apply(&self, rules: &mut Rules, relative: &str, basename: &str) {
        for (patterns, properties) in &self.sections {
            let matched = patterns.iter().any(|pattern| {
                // A pattern with no separator in it is about the file's name,
                // wherever the file is; one with a separator is about its path
                // from the directory the `.editorconfig` sits in.
                if pattern.contains('/') {
                    matches(pattern, relative)
                } else {
                    matches(pattern, basename)
                }
            });
            if !matched {
                continue;
            }

            for (key, value) in properties {
                match key.as_str() {
                    "indent_style" => match value.as_str() {
                        "tab" => rules.style = Some(Style::Tabs),
                        "space" => rules.style = Some(Style::Spaces),
                        // Removes the property, a nearer file's value included.
                        "unset" => rules.style = None,
                        _ => {}
                    },
                    "indent_size" => match value.as_str() {
                        "tab" => rules.size = Some(Size::Tab),
                        "unset" => rules.size = None,
                        value => {
                            if let Ok(columns) = value.parse() {
                                rules.size = Some(Size::Columns(columns));
                            }
                        }
                    },
                    "tab_width" => match value.as_str() {
                        "unset" => rules.tab_width = None,
                        value => {
                            if let Ok(width) = value.parse() {
                                rules.tab_width = Some(width);
                            }
                        }
                    },
                    _ => {}
                }
            }
        }
    }
}

/// Whether a glob pattern matches a path.
///
/// `*` is any run of characters within one segment, `**` any run at all, `?`
/// one character within a segment, `[...]` one character from a set — or
/// outside it, with a leading `!` — and `{a,b}` several patterns in one.
fn matches(pattern: &str, path: &str) -> bool {
    expand_braces(pattern)
        .iter()
        .any(|pattern| matches_one(pattern, path))
}

fn matches_one(pattern: &str, path: &str) -> bool {
    let Some(head) = pattern.chars().next() else {
        return path.is_empty();
    };

    match head {
        '*' if pattern.starts_with("**") => {
            let rest = &pattern[2..];
            (0..=path.len())
                .filter(|end| path.is_char_boundary(*end))
                .any(|end| matches_one(rest, &path[end..]))
        }
        '*' => {
            let rest = &pattern[1..];
            (0..=path.len())
                .filter(|end| path.is_char_boundary(*end))
                .take_while(|end| !path[..*end].contains('/'))
                .any(|end| matches_one(rest, &path[end..]))
        }
        '?' => match path.chars().next() {
            Some(next) if next != '/' => matches_one(&pattern[1..], &path[next.len_utf8()..]),
            _ => false,
        },
        '[' => match parse_class(pattern) {
            Some((accepted, negated, end)) => match path.chars().next() {
                Some(next) if next != '/' && accepted.contains(&next) != negated => {
                    matches_one(&pattern[end..], &path[next.len_utf8()..])
                }
                _ => false,
            },
            // An unclosed class is the character it looks like.
            None => path.starts_with('[') && matches_one(&pattern[1..], &path[1..]),
        },
        head => {
            path.starts_with(head)
                && matches_one(&pattern[head.len_utf8()..], &path[head.len_utf8()..])
        }
    }
}

/// The characters a `[...]` class accepts, whether it is negated, and where the
/// class ends in the pattern.
///
/// `a-z` is a range, a `-` anywhere else is itself, and a class cannot hold a
/// `]` of its own — the first one closes it.
fn parse_class(pattern: &str) -> Option<(Vec<char>, bool, usize)> {
    let characters: Vec<(usize, char)> = pattern.char_indices().skip(1).collect();
    let mut accepted = Vec::new();
    let mut negated = false;
    let mut at = 0;

    if let Some(&(_, '!')) = characters.first() {
        negated = true;
        at = 1;
    }

    // One character is held back at a time: it may be the start of a range, and
    // only the character after it says so.
    let mut pending: Option<char> = None;
    while at < characters.len() {
        let (index, character) = characters[at];
        match character {
            ']' => {
                accepted.extend(pending.take());
                return Some((accepted, negated, index + 1));
            }
            '-' if pending.is_some()
                && at + 1 < characters.len()
                && characters[at + 1].1 != ']' =>
            {
                let start = pending.take().expect("checked above");
                accepted.extend(start..=characters[at + 1].1);
                at += 2;
                continue;
            }
            character => accepted.extend(pending.replace(character)),
        }
        at += 1;
    }
    None
}

/// Every pattern a brace group stands for.
///
/// `{a,b}` is two patterns, groups nest, and one pattern can hold several. A
/// group without a comma in it is left alone, which leaves the format's numeric
/// ranges — `{1..3}` — matching as literally as they were written.
fn expand_braces(pattern: &str) -> Vec<String> {
    let Some(open) = pattern.find('{') else {
        return vec![pattern.to_string()];
    };

    let mut depth = 0;
    let mut close = None;
    let mut commas = Vec::new();
    for (at, character) in pattern[open..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + at);
                    break;
                }
            }
            ',' if depth == 1 => commas.push(open + at),
            _ => {}
        }
    }
    let Some(close) = close else {
        return vec![pattern.to_string()];
    };
    if commas.is_empty() {
        return vec![pattern.to_string()];
    }

    let mut expanded = Vec::new();
    let mut start = open + 1;
    for comma in commas.iter().copied().chain(std::iter::once(close)) {
        let combined = format!(
            "{}{}{}",
            &pattern[..open],
            &pattern[start..comma],
            &pattern[close + 1..]
        );
        expanded.extend(expand_braces(&combined));
        start = comma + 1;
    }
    expanded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(text: &str, path: &str) -> Rules {
        let config = Config::parse(text);
        let mut rules = Rules::default();
        let path = Path::new(path);
        let relative = path.to_string_lossy().replace('\\', "/");
        let basename = path.file_name().unwrap().to_string_lossy().to_string();
        config.apply(&mut rules, &relative, &basename);
        rules
    }

    fn settings() -> Indent {
        Indent {
            columns: 4,
            hard_tabs: false,
        }
    }

    #[test]
    fn a_pattern_without_a_separator_is_about_the_name() {
        // Which of the two a pattern is matched against is the section's
        // business, so these go through one.
        let size = |text: &str, path: &str| rules(text, path).indent(settings()).columns;
        assert_eq!(size("[*.rs]\nindent_size = 2\n", "src/deep/main.rs"), 2);
        assert_eq!(size("[*.rs]\nindent_size = 2\n", "main.toml"), 4);
        // A pattern with one is about the path, and `*` stops at a separator.
        assert_eq!(size("[src/*.rs]\nindent_size = 2\n", "src/main.rs"), 2);
        assert_eq!(size("[src/*.rs]\nindent_size = 2\n", "src/deep/main.rs"), 4);
        assert_eq!(
            size("[src/**.rs]\nindent_size = 2\n", "src/deep/main.rs"),
            2
        );
    }

    #[test]
    fn globs_match_the_way_the_format_says() {
        assert!(matches("main.rs", "main.rs"));
        assert!(!matches("main.rs", "main.toml"));
        assert!(matches("*.rs", "main.rs"));
        assert!(matches("**/*.rs", "src/deep/main.rs"));
        assert!(!matches("src/*.rs", "src/deep/main.rs"));
        assert!(matches("?ain.rs", "main.rs"));
        assert!(!matches("?ain.rs", "ain.rs"));
        assert!(matches("[mM]ain.rs", "main.rs"));
        assert!(!matches("[!m]ain.rs", "main.rs"));
        assert!(matches("[a-z]ain.rs", "main.rs"));
        assert!(matches("{*.rs,*.toml}", "main.toml"));
        assert!(!matches("{*.rs,*.toml}", "main.md"));
        // A group with nothing to choose between is not an expansion.
        assert!(!matches("v{1..3}.rs", "v1.rs"));
        assert!(matches("v{1..3}.rs", "v{1..3}.rs"));
    }

    #[test]
    fn sections_apply_in_order_and_unsets_remove() {
        let parsed = rules(
            "[*]\nindent_style = space\nindent_size = 4\n\n[*.rs]\nindent_size = 2\n",
            "src/main.rs",
        );
        assert_eq!(parsed.indent(settings()).columns, 2);
        assert!(!parsed.indent(settings()).hard_tabs);

        let parsed = rules(
            "[*]\nindent_style = tab\n\n[Makefile]\nindent_style = unset\n",
            "Makefile",
        );
        assert!(!parsed.indent(settings()).hard_tabs);
    }

    #[test]
    fn what_a_file_does_not_say_comes_from_the_settings() {
        let parsed = rules("[*]\nindent_size = 2\n", "main.rs");
        assert_eq!(
            parsed.indent(Indent {
                columns: 4,
                hard_tabs: true
            }),
            Indent {
                columns: 2,
                hard_tabs: true
            }
        );

        // `indent_size = tab` is as wide as a tab, which is `tab_width`.
        let parsed = rules(
            "[*]\nindent_style = tab\nindent_size = tab\ntab_width = 8\n",
            "main.rs",
        );
        assert_eq!(
            parsed.indent(settings()),
            Indent {
                columns: 8,
                hard_tabs: true
            }
        );
        // Without a `tab_width` there is nothing to measure a tab with.
        let parsed = rules("[*]\nindent_size = tab\n", "main.rs");
        assert_eq!(parsed.indent(settings()).columns, 4);
    }

    #[test]
    fn a_range_a_project_cannot_mean_is_clamped() {
        let parsed = rules("[*]\nindent_size = 20\n", "main.rs");
        assert_eq!(parsed.indent(settings()).columns, 8);
        let parsed = rules("[*]\nindent_size = 0\n", "main.rs");
        assert_eq!(parsed.indent(settings()).columns, 1);
    }

    #[test]
    fn root_ends_the_search_and_the_nearest_file_wins() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("src/deep")).unwrap();
        std::fs::write(
            root.join(".editorconfig"),
            "root = true\n[*]\nindent_size = 3\n[*.md]\nindent_size = 6\n",
        )
        .unwrap();
        std::fs::write(root.join("src/.editorconfig"), "[*]\nindent_size = 2\n").unwrap();

        let near = Rules::for_path(&root.join("src/deep/main.rs"));
        assert_eq!(near.indent(settings()).columns, 2);
        // The nearer file says nothing about markdown, so the outer one does.
        let markdown = Rules::for_path(&root.join("src/deep/notes.md"));
        assert_eq!(markdown.indent(settings()).columns, 2);
        // Outside `src` only the outer file applies, and it covers markdown.
        assert_eq!(
            Rules::for_path(&root.join("notes.md"))
                .indent(settings())
                .columns,
            6
        );
        assert_eq!(
            Rules::for_path(&root.join("main.rs"))
                .indent(settings())
                .columns,
            3
        );
    }

    #[test]
    fn a_file_with_no_editorconfig_above_it_says_nothing() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("main.rs");
        std::fs::write(&file, "").unwrap();
        assert_eq!(Rules::for_path(&file), Rules::default());
    }
}
