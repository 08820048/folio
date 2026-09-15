//! Per-type icons for files in the tree, the way Zed marks a `.rs` file with
//! the Rust gear rather than a generic page.
//!
//! Language logos come from Devicon (MIT, `assets/icons/file-types/
//! LICENSE-devicon.txt`); the generic shapes (lock, text, image, cog) are
//! Lucide. Every SVG is rendered as a mask, so its own colours are ignored
//! and the tint comes from [`FileIcon::tint`].

use gpui::{Hsla, SharedString, hsla};
use gpui_component::IconNamed;

macro_rules! file_icons {
    ($( $variant:ident => $file:literal ),* $(,)?) => {
        /// One icon in `assets/icons/file-types`.
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum FileIcon { $( $variant ),* }

        #[cfg(test)]
        impl FileIcon {
            pub const ALL: &[FileIcon] = &[ $( Self::$variant ),* ];
        }

        impl IconNamed for FileIcon {
            fn path(self) -> SharedString {
                match self {
                    $( Self::$variant => concat!("icons/file-types/", $file, ".svg").into() ),*
                }
            }
        }

        /// Asset path and bytes of every file-type icon.
        pub const FILE_TYPE_ICONS: &[(&str, &[u8])] = &[
            $((
                concat!("icons/file-types/", $file, ".svg"),
                include_bytes!(concat!("../assets/icons/file-types/", $file, ".svg")),
            ),)*
        ];
    };
}

file_icons! {
    Rust => "rust",
    Git => "git",
    Markdown => "markdown",
    Json => "json",
    Yaml => "yaml",
    Python => "python",
    JavaScript => "javascript",
    TypeScript => "typescript",
    Html => "html5",
    Css => "css3",
    Sass => "sass",
    Shell => "bash",
    Docker => "docker",
    Go => "go",
    C => "c",
    Cpp => "cplusplus",
    Java => "java",
    Lua => "lua",
    Swift => "swift",
    Kotlin => "kotlin",
    Ruby => "ruby",
    Php => "php",
    React => "react",
    Vue => "vuejs",
    Svelte => "svelte",
    Sql => "sqlite",
    Zig => "zig",
    CSharp => "csharp",
    Dart => "dart",
    Elixir => "elixir",
    Haskell => "haskell",
    OCaml => "ocaml",
    Nix => "nixos",
    GraphQl => "graphql",
    Lock => "lock",
    Text => "file-text",
    Image => "image",
    Config => "file-cog",
}

/// Hue families the tree uses. Kept few so the sidebar stays a palette,
/// not a rainbow; the exact colours are picked per theme in [`Tint::color`].
#[derive(Clone, Copy)]
enum Tint {
    Red,
    Orange,
    Yellow,
    Green,
    Teal,
    Blue,
    Indigo,
    Purple,
    Pink,
}

impl Tint {
    fn color(self, dark: bool) -> Hsla {
        let hue = match self {
            Self::Red => 0.01,
            Self::Orange => 0.07,
            Self::Yellow => 0.12,
            Self::Green => 0.36,
            Self::Teal => 0.50,
            Self::Blue => 0.60,
            Self::Indigo => 0.68,
            Self::Purple => 0.78,
            Self::Pink => 0.90,
        };
        // Same register as the bracket colours: readable on both themes
        // without turning saturated.
        if dark {
            hsla(hue, 0.42, 0.68, 1.)
        } else {
            hsla(hue, 0.48, 0.44, 1.)
        }
    }
}

impl FileIcon {
    /// Colour for the icon, or `None` for the generic shapes, which stay
    /// in the tree's muted foreground.
    pub fn tint(self, dark: bool) -> Option<Hsla> {
        let tint = match self {
            Self::Rust | Self::Html | Self::Java | Self::Swift | Self::Svelte | Self::Zig
            | Self::OCaml => Tint::Orange,
            Self::Git | Self::Ruby => Tint::Red,
            Self::Json | Self::JavaScript => Tint::Yellow,
            Self::Shell | Self::Vue => Tint::Green,
            Self::Go | Self::React | Self::Dart => Tint::Teal,
            Self::Markdown | Self::Python | Self::TypeScript | Self::Css | Self::Docker
            | Self::C | Self::Sql | Self::Nix => Tint::Blue,
            Self::Cpp | Self::Lua | Self::Php => Tint::Indigo,
            Self::Yaml | Self::Kotlin | Self::CSharp | Self::Elixir | Self::Haskell => {
                Tint::Purple
            }
            Self::Sass | Self::GraphQl => Tint::Pink,
            Self::Lock | Self::Text | Self::Image | Self::Config => return None,
        };
        Some(tint.color(dark))
    }
}

/// The icon for a file called `name`, or `None` for the plain page.
/// Whole names win over extensions, so `Cargo.lock` is a lock even
/// though `.lock` alone would be too, and `.gitignore` is git, not "no
/// extension".
pub fn file_icon(name: &str) -> Option<FileIcon> {
    let lower = name.to_ascii_lowercase();
    let by_name = match lower.as_str() {
        ".gitignore" | ".gitattributes" | ".gitmodules" | ".gitkeep" | ".gitconfig"
        | ".mailmap" => Some(FileIcon::Git),
        "cargo.lock" | "package-lock.json" | "yarn.lock" | "pnpm-lock.yaml" | "bun.lockb"
        | "bun.lock" | "flake.lock" | "poetry.lock" | "gemfile.lock" | "composer.lock"
        | "uv.lock" => Some(FileIcon::Lock),
        "dockerfile" | "containerfile" | ".dockerignore" => Some(FileIcon::Docker),
        "gemfile" | "rakefile" => Some(FileIcon::Ruby),
        ".editorconfig" | ".env" | ".npmrc" | ".prettierrc" | ".eslintrc" | ".babelrc"
        | "makefile" | "justfile" => Some(FileIcon::Config),
        "license" | "licence" | "copying" | "notice" | "authors" | "changelog" | "readme"
        | "todo" => Some(FileIcon::Text),
        _ => None,
    };
    if by_name.is_some() {
        return by_name;
    }
    let (_, ext) = lower.rsplit_once('.')?;
    Some(match ext {
        "rs" => FileIcon::Rust,
        "md" | "markdown" | "mdx" => FileIcon::Markdown,
        "json" | "jsonc" | "json5" => FileIcon::Json,
        "yaml" | "yml" => FileIcon::Yaml,
        "py" | "pyi" | "pyw" => FileIcon::Python,
        "js" | "mjs" | "cjs" => FileIcon::JavaScript,
        "ts" | "mts" | "cts" => FileIcon::TypeScript,
        "jsx" | "tsx" => FileIcon::React,
        "html" | "htm" | "xhtml" => FileIcon::Html,
        "css" => FileIcon::Css,
        "scss" | "sass" | "less" => FileIcon::Sass,
        "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat" | "cmd" => FileIcon::Shell,
        "go" => FileIcon::Go,
        "c" | "h" => FileIcon::C,
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => FileIcon::Cpp,
        "java" | "jar" | "class" => FileIcon::Java,
        "lua" => FileIcon::Lua,
        "swift" => FileIcon::Swift,
        "kt" | "kts" => FileIcon::Kotlin,
        "rb" | "erb" | "gemspec" => FileIcon::Ruby,
        "php" => FileIcon::Php,
        "vue" => FileIcon::Vue,
        "svelte" => FileIcon::Svelte,
        "sql" | "sqlite" | "sqlite3" | "db" => FileIcon::Sql,
        "zig" => FileIcon::Zig,
        "cs" | "csx" => FileIcon::CSharp,
        "dart" => FileIcon::Dart,
        "ex" | "exs" | "heex" => FileIcon::Elixir,
        "hs" | "lhs" => FileIcon::Haskell,
        "ml" | "mli" => FileIcon::OCaml,
        "nix" => FileIcon::Nix,
        "graphql" | "gql" => FileIcon::GraphQl,
        "lock" => FileIcon::Lock,
        "txt" | "log" | "rst" | "adoc" | "org" => FileIcon::Text,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" | "bmp" | "avif" | "heic" => {
            FileIcon::Image
        }
        "toml" | "ini" | "cfg" | "conf" | "config" | "env" | "properties" | "plist"
        | "xml" => FileIcon::Config,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_names_beat_extensions() {
        assert_eq!(file_icon("Cargo.lock"), Some(FileIcon::Lock));
        assert_eq!(file_icon("Cargo.toml"), Some(FileIcon::Config));
        assert_eq!(file_icon(".gitignore"), Some(FileIcon::Git));
        assert_eq!(file_icon("Dockerfile"), Some(FileIcon::Docker));
        assert_eq!(file_icon("LICENSE"), Some(FileIcon::Text));
    }

    #[test]
    fn extensions_are_case_insensitive_and_use_the_last_dot() {
        assert_eq!(file_icon("main.rs"), Some(FileIcon::Rust));
        assert_eq!(file_icon("README.MD"), Some(FileIcon::Markdown));
        assert_eq!(file_icon("app.test.tsx"), Some(FileIcon::React));
        assert_eq!(file_icon("archive.tar.gz"), None);
        assert_eq!(file_icon("Makefile.old"), None);
        assert_eq!(file_icon("noext"), None);
    }

    #[test]
    fn every_icon_has_a_path_under_file_types() {
        for icon in FileIcon::ALL {
            let path = icon.path();
            assert!(path.starts_with("icons/file-types/"), "{path}");
            assert!(
                FILE_TYPE_ICONS.iter().any(|(p, _)| *p == path.as_ref()),
                "`{path}` is not embedded"
            );
        }
    }

    #[test]
    fn generic_shapes_stay_muted_and_logos_are_tinted() {
        assert!(FileIcon::Lock.tint(false).is_none());
        assert!(FileIcon::Text.tint(true).is_none());
        assert!(FileIcon::Rust.tint(false).is_some());
        assert!(FileIcon::Git.tint(true).is_some());
    }
}
