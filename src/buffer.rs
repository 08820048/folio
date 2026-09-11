use std::{
    fs,
    io::{self, Read, Write},
    ops::Range,
    path::Path,
};

pub const HIGHLIGHT_LIMIT: usize = 5 * 1024 * 1024;
pub const FILE_LIMIT: u64 = 32 * 1024 * 1024;

/// Read losslessly. Unsupported encodings must never become replacement characters on save.
pub fn read(path: &Path) -> io::Result<String> {
    let bytes = read_bytes(path)?;
    if bytes.contains(&0) {
        return Err(io::Error::other("Cannot preview: binary file"));
    }
    String::from_utf8(bytes)
        .map_err(|_| io::Error::other("Cannot preview: only UTF-8 text is supported"))
}

pub fn read_bytes(path: &Path) -> io::Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > FILE_LIMIT {
        return Err(io::Error::other(
            "Cannot preview: not a regular file, or larger than 32 MB",
        ));
    }
    let mut bytes = Vec::new();
    file.take(FILE_LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > FILE_LIMIT {
        return Err(io::Error::other(
            "Cannot preview: file is larger than 32 MB",
        ));
    }
    Ok(bytes)
}

/// Save beside the original, then atomically replace it. A failed write keeps the original intact.
pub fn save(path: &Path, text: &str, expected: &str) -> io::Result<()> {
    if read(path)? != expected {
        return Err(io::Error::other(
            "File changed on disk; nothing was overwritten. Keep your edits and reopen the project",
        ));
    }
    let permissions = fs::metadata(path)?.permissions();
    if permissions.readonly() {
        return Err(io::Error::other("File is read-only; not saved"));
    }
    let mut temp = tempfile::NamedTempFile::new_in(
        path.parent()
            .ok_or_else(|| io::Error::other("Invalid file path"))?,
    )?;
    temp.as_file().set_permissions(permissions)?;
    temp.write_all(text.as_bytes())?;
    temp.as_file().sync_all()?;
    // Check again after writing the temporary file to narrow the external-editor race.
    if read(path)? != expected {
        return Err(io::Error::other(
            "File changed on disk; nothing was overwritten",
        ));
    }
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// Returns the grammar name for a file.
///
/// Every name must resolve in gpui-component's `LanguageRegistry`: either one of
/// its built-in languages or one added by `src/syntax.rs`. `"text"` means "no
/// grammar" and is rendered as unhighlighted text; it is registered by
/// gpui-component, so an unknown file never has to go through
/// `SyntaxHighlighter`'s error fallback.
///
/// `src/syntax.rs` has a test that walks a sample of these paths and asserts the
/// returned name actually resolves, so the two tables cannot drift apart.
pub fn language(path: &Path) -> &'static str {
    let filename = path.file_name().and_then(|x| x.to_str()).unwrap_or("");
    let filename = filename.to_ascii_lowercase();
    // Whole-name matches beat extensions: "Dockerfile" has no extension at all,
    // and "CMakeLists.txt" would otherwise be read as plain text.
    let named = match filename.as_str() {
        "makefile" | "gnumakefile" | "bsdmakefile" | "makefile.am" | "makefile.in" => Some("make"),
        "cmakelists.txt" => Some("cmake"),
        "gemfile" | "rakefile" | "guardfile" | "podfile" | "brewfile" | "vagrantfile"
        | "fastfile" | "appfile" | "dangerfile" | "capfile" | "berksfile" | "thorfile" => {
            Some("ruby")
        }
        ".bashrc" | ".bash_profile" | ".bash_login" | ".bash_logout" | ".bash_aliases"
        | ".profile" | ".envrc" | ".zshrc" | ".zprofile" | ".zshenv" | ".zlogin" | ".zlogout"
        | ".kshrc" | ".tmux.conf" => Some("bash"),
        ".editorconfig" | ".gitconfig" | ".npmrc" | ".yarnrc" | ".pylintrc" | ".flake8"
        | ".coveragerc" | ".hgrc" => Some("ini"),
        ".vimrc" | "vimrc" | ".gvimrc" | "gvimrc" | ".exrc" => Some("vim"),
        ".rprofile" | ".renviron" => Some("r"),
        "cargo.lock" | "pipfile" | "poetry.lock" | "gopkg.lock" => Some("toml"),
        "dockerfile" | "containerfile" => Some("dockerfile"),
        "commit_editmsg" | "merge_msg" | "tag_editmsg" | "git-rebase-todo" | ".gitmessage" => {
            Some("gitcommit")
        }
        _ => None,
    };
    if let Some(language) = named {
        return language;
    }
    // Variants carry a suffix instead of an extension: "Dockerfile.dev".
    if filename.starts_with("dockerfile.") || filename.starts_with("containerfile.") {
        return "dockerfile";
    }
    // ".env" / ".env.local" / ".env.example" are KEY=value files.
    if filename == ".env" || filename.starts_with(".env.") {
        return "ini";
    }
    let extension = path.extension().and_then(|x| x.to_str()).unwrap_or("");
    // Uppercase .C and .H conventionally denote C++, unlike lowercase .c / .h.
    if matches!(extension, "C" | "H") {
        return "cpp";
    }
    match extension.to_ascii_lowercase().as_str() {
        // Systems
        "rs" => "rust",
        "go" => "go",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "c++" | "hpp" | "hh" | "hxx" | "h++" | "ipp" | "tpp" => "cpp",
        "cs" | "csx" => "csharp",
        "swift" => "swift",
        "zig" | "zon" => "zig",
        "dart" => "dart",
        "nix" => "nix",
        "m" | "mm" => "objc",
        "s" | "asm" | "nasm" => "asm",
        // JVM
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "scala" | "sc" | "sbt" => "scala",
        // Scripting
        "py" | "pyi" | "pyw" => "python",
        "rb" | "rake" | "gemspec" | "ru" => "ruby",
        "php" | "phtml" => "php",
        "lua" => "lua",
        "sh" | "bash" | "zsh" | "ksh" => "bash",
        "fish" => "fish",
        "vim" => "vim",
        "ps1" | "psm1" | "psd1" => "powershell",
        "ex" | "exs" => "elixir",
        "erl" | "hrl" => "erlang",
        // Functional
        "hs" | "lhs" => "haskell",
        // The interface grammar shares the implementation query in the upstream
        // crate, and that query does not compile against it, so `.mli` reuses the
        // implementation grammar and is highlighted approximately.
        "ml" | "mli" => "ocaml",
        "elm" => "elm",
        "gleam" => "gleam",
        "r" => "r",
        // Data science and contracts
        "sol" => "solidity",
        // Web
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "vue" => "vue",
        "html" | "htm" => "html",
        "css" => "css",
        "scss" | "sass" => "scss",
        "less" => "less",
        "astro" => "astro",
        "svelte" => "svelte",
        "erb" => "erb",
        "ejs" => "ejs",
        // Data and configuration
        "json" | "jsonc" | "json5" | "ipynb" | "webmanifest" => "json",
        "toml" => "toml",
        "yml" | "yaml" => "yaml",
        "xml" | "xsd" | "xsl" | "xslt" | "wsdl" | "plist" | "csproj" | "fsproj" | "vbproj"
        | "props" | "targets" | "xaml" | "pom" | "nuspec" | "resx" | "storyboard" | "xib"
        | "rss" | "atom" => "xml",
        "dtd" => "dtd",
        "ini" | "cfg" | "cnf" | "conf" | "properties" | "desktop" | "prefs" | "service"
        | "socket" | "timer" => "ini",
        "md" | "mdx" | "markdown" => "markdown",
        "sql" => "sql",
        "graphql" | "gql" => "graphql",
        "proto" => "proto",
        "diff" | "patch" => "diff",
        "regex" => "regex",
        // Build systems and tools
        "cmake" => "cmake",
        "mk" | "mak" => "make",
        "dockerfile" | "containerfile" => "dockerfile",
        _ => "text",
    }
}

/// The marker that comments out one line of a language, or `None` where the
/// language has no line form. Plenty of what Folio highlights only has a block
/// form — HTML, XML, CSS, Markdown — and a shortcut that inserted a marker the
/// file cannot use would be worse than one that does nothing.
pub fn line_comment(language: &str) -> Option<&'static str> {
    Some(match language {
        "rust" | "c" | "cpp" | "csharp" | "go" | "zig" | "swift" | "java" | "kotlin" | "scala"
        | "dart" | "typescript" | "tsx" | "javascript" | "php" | "solidity" | "less" | "scss"
        | "sass" | "protobuf" => "//",
        "python" | "ruby" | "bash" | "fish" | "powershell" | "r" | "elixir" | "perl" | "toml"
        | "yaml" | "make" | "dockerfile" | "ini" | "nix" | "cmake" | "gitignore" | "gitcommit"
        | "graphql" => "#",
        "sql" | "lua" | "haskell" | "elm" => "--",
        "erlang" => "%",
        "asm" => ";",
        _ => return None,
    })
}

/// The byte range of the whole lines a selection touches, which is what a line
/// operation works on. A selection ending at the start of a line does not
/// reach into it, so a caret at a line's end affects only that line.
pub fn line_range(text: &str, selection: Range<usize>) -> Range<usize> {
    let start = selection.start.min(text.len());
    let end = selection.end.min(text.len());
    let first = text[..start].rfind('\n').map_or(0, |at| at + 1);
    let last = text[end..].find('\n').map_or(text.len(), |at| end + at);
    first..last
}

/// Split a line into its body and the newline it ended with, which the round
/// trip has to preserve exactly.
fn split_newline(line: &str) -> (&str, &str) {
    line.strip_suffix('\n')
        .map_or((line, ""), |body| (body, "\n"))
}

/// Whether every line in the block is already commented. A blank line counts:
/// it has nothing to comment out, and refusing the toggle over one would make
/// the shortcut unpredictable on a selection that ends with a newline.
fn all_commented(block: &str, marker: &str) -> bool {
    block.split_inclusive('\n').all(|line| {
        let (body, _) = split_newline(line);
        let body = body.trim_start();
        body.starts_with(marker) || body.is_empty()
    })
}

/// Add the marker to each line, after whatever indentation it carries.
fn comment(block: &str, marker: &str) -> String {
    block
        .split_inclusive('\n')
        .map(|line| {
            let (body, newline) = split_newline(line);
            let indent = body.len() - body.trim_start().len();
            let (indent, rest) = body.split_at(indent);
            if rest.is_empty() {
                format!("{indent}{marker}{newline}")
            } else {
                format!("{indent}{marker} {rest}{newline}")
            }
        })
        .collect()
}

/// Take the marker off each line that has one. A line that does not is left
/// alone rather than shifted.
fn uncomment(block: &str, marker: &str) -> String {
    block
        .split_inclusive('\n')
        .map(|line| {
            let (body, newline) = split_newline(line);
            let indent = body.len() - body.trim_start().len();
            let (indent, rest) = body.split_at(indent);
            let rest = rest
                .strip_prefix(marker)
                .map(|rest| rest.strip_prefix(' ').unwrap_or(rest))
                .unwrap_or(rest);
            format!("{indent}{rest}{newline}")
        })
        .collect()
}

/// Comment a block, or uncomment one that is already commented throughout.
pub fn toggle_comments(block: &str, marker: &str) -> String {
    if all_commented(block, marker) {
        uncomment(block, marker)
    } else {
        comment(block, marker)
    }
}

#[cfg(test)]
mod comment_tests {
    use super::*;

    #[test]
    fn the_marker_follows_the_language() {
        assert_eq!(line_comment("rust"), Some("//"));
        assert_eq!(line_comment("python"), Some("#"));
        assert_eq!(line_comment("sql"), Some("--"));
        assert_eq!(line_comment("erlang"), Some("%"));
        // Languages whose comments are only a block form say so rather than
        // guessing a marker the file cannot use.
        assert_eq!(line_comment("html"), None);
        assert_eq!(line_comment("css"), None);
        assert_eq!(line_comment("markdown"), None);
        assert_eq!(line_comment("json"), None);
        assert_eq!(line_comment("text"), None);
    }

    #[test]
    fn a_selection_expands_to_whole_lines() {
        let text = "one\ntwo\nthree\n";
        // A caret inside the second line takes that line alone.
        assert_eq!(line_range(text, 5..5), 4..7);
        // A selection from the middle of the first into the third takes all
        // three, and stops at the last line's newline.
        assert_eq!(line_range(text, 1..9), 0..13);
        // A caret at a line's end does not reach into the next one.
        assert_eq!(line_range(text, 7..7), 4..7);
        // The last line has no newline to stop at.
        assert_eq!(line_range("one\ntwo", 5..5), 4..7);
    }

    #[test]
    fn toggling_comments_and_back_is_a_round_trip() {
        let block = "let one = 1;\n  let two = 2;\n";
        let commented = toggle_comments(block, "//");
        // The marker goes after the indentation, and a space follows it.
        assert_eq!(commented, "// let one = 1;\n  // let two = 2;\n");
        assert_eq!(toggle_comments(&commented, "//"), block);
    }

    #[test]
    fn a_mixed_selection_is_commented_rather_than_uncommented() {
        let block = "// one\ntwo\n";
        assert_eq!(toggle_comments(block, "//"), "// // one\n// two\n");
    }

    #[test]
    fn blank_lines_do_not_block_the_toggle() {
        // A selection ending with a newline brings a blank line with it; the
        // block still reads as commented, so the toggle uncomments.
        let block = "// one\n\n";
        assert_eq!(toggle_comments(block, "//"), "one\n\n");
        // And commenting leaves a blank line's marker bare, with no trailing
        // space to show up in diffs.
        assert_eq!(toggle_comments("one\n\n", "//"), "// one\n//\n");
    }

    #[test]
    fn uncommenting_leaves_a_line_without_the_marker_alone() {
        assert_eq!(uncomment("// one\ntwo\n", "//"), "one\ntwo\n");
        // The marker may have been written without the space.
        assert_eq!(uncomment("//one\n", "//"), "one\n");
    }

    #[test]
    fn commenting_keeps_multibyte_lines_intact() {
        let block = "中文 = 1\n  中文 = 2\n";
        assert_eq!(toggle_comments(block, "#"), "# 中文 = 1\n  # 中文 = 2\n");
        assert_eq!(toggle_comments("# 中文 = 1\n", "#"), "中文 = 1\n");
    }
}
