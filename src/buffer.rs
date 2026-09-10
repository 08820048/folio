use std::{
    fs,
    io::{self, Read, Write},
    path::Path,
};

pub const HIGHLIGHT_LIMIT: usize = 5 * 1024 * 1024;
pub const FILE_LIMIT: u64 = 32 * 1024 * 1024;

/// Read losslessly. Unsupported encodings must never become replacement characters on save.
pub fn read(path: &Path) -> io::Result<String> {
    let bytes = read_bytes(path)?;
    if bytes.contains(&0) {
        return Err(io::Error::other("无法预览：二进制文件"));
    }
    String::from_utf8(bytes).map_err(|_| io::Error::other("无法预览：目前仅支持 UTF-8 文本"))
}

pub fn read_bytes(path: &Path) -> io::Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > FILE_LIMIT {
        return Err(io::Error::other("无法预览：不是普通文件，或文件超过 32 MB"));
    }
    let mut bytes = Vec::new();
    file.take(FILE_LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > FILE_LIMIT {
        return Err(io::Error::other("无法预览：文件超过 32 MB"));
    }
    Ok(bytes)
}

/// Save beside the original, then atomically replace it. A failed write keeps the original intact.
pub fn save(path: &Path, text: &str, expected: &str) -> io::Result<()> {
    if read(path)? != expected {
        return Err(io::Error::other(
            "文件已在磁盘变更，未覆盖。请保留当前修改并重新打开项目",
        ));
    }
    let permissions = fs::metadata(path)?.permissions();
    if permissions.readonly() {
        return Err(io::Error::other("文件为只读，未保存"));
    }
    let mut temp = tempfile::NamedTempFile::new_in(
        path.parent()
            .ok_or_else(|| io::Error::other("无效文件路径"))?,
    )?;
    temp.as_file().set_permissions(permissions)?;
    temp.write_all(text.as_bytes())?;
    temp.as_file().sync_all()?;
    // Check again after writing the temporary file to narrow the external-editor race.
    if read(path)? != expected {
        return Err(io::Error::other("文件已在磁盘变更，未覆盖"));
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
