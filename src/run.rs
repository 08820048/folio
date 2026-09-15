//! What Folio can run from an open file, and the command it starts in a
//! dedicated Run tab — the child is the command, not the login shell.

use std::path::{Path, PathBuf};

/// One place in a file the user can start from: a `main`, a test, …
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runnable {
    /// 0-based line of the `fn` / `def` / `__main__` the icon sits on.
    pub line: usize,
    /// 0-based last line of the body. A caret anywhere in `line..=end`
    /// is "in" this runnable; `⌘R` from there runs it.
    pub end: usize,
    pub name: String,
    pub kind: Kind,
}

impl Runnable {
    fn contains(&self, line: usize) -> bool {
        self.line <= line && line <= self.end
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Main,
    Test,
}

/// A command ready to start in a dedicated Run tab. `cwd` is where the
/// pty starts the child, so `shell` itself never has to `cd`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub label: String,
    pub shell: String,
    pub cwd: PathBuf,
}

/// Every runnable in `text` for this path's language.
pub fn find(path: &Path, text: &str) -> Vec<Runnable> {
    match extension(path).as_deref() {
        Some("rs") => rust_runnables(text),
        Some("py" | "pyw") => python_runnables(text),
        Some("go") => go_runnables(text),
        Some("js" | "mjs" | "cjs") => javascript_runnables(text),
        Some("rb") => ruby_runnables(text),
        _ => Vec::new(),
    }
}

/// Whether this file has a command at all, before reading its text. This
/// is the only part that touches the filesystem (looking for `Cargo.toml`
/// and `go.mod`), so callers cache it per document instead of asking on
/// every keystroke.
pub fn can_run(root: &Path, file: &Path) -> bool {
    command(root, file, None).is_some()
}

/// The command for a caret on `line`, or the file/project default.
///
/// A caret picks a runnable only when it sits inside that runnable's body;
/// the innermost one wins when they nest. Anywhere else — between
/// functions, in a helper — runs the file or crate as a whole, which is
/// what the title-bar Play does too.
pub fn resolve(root: &Path, file: &Path, text: &str, line: Option<usize>) -> Option<Command> {
    let runnables = find(file, text);
    let target = line.and_then(|line| {
        runnables
            .iter()
            .filter(|runnable| runnable.contains(line))
            .max_by_key(|runnable| runnable.line)
    });
    command(root, file, target)
}

fn command(root: &Path, file: &Path, target: Option<&Runnable>) -> Option<Command> {
    match extension(file).as_deref() {
        Some("rs") => rust_command(root, file, target),
        Some("py" | "pyw") => python_command(root, file, target),
        Some("go") => go_command(root, file, target),
        Some("js" | "mjs" | "cjs") => javascript_command(root, file, target),
        Some("rb") => ruby_command(root, file, target),
        Some("sh" | "bash") => shell_command(root, file, "bash"),
        Some("zsh") => shell_command(root, file, "zsh"),
        _ => None,
    }
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
}

/// A brace-delimited body whose closing line is still ahead.
struct OpenBody {
    index: usize,
    base_depth: i32,
    /// The `{` has been seen; before that a `}` cannot close it.
    started: bool,
}

/// Close every body whose braces balanced out on this line.
fn close_bodies(open: &mut Vec<OpenBody>, out: &mut [Runnable], line: &str, depth: i32, at: usize) {
    for body in open.iter_mut() {
        if !body.started && line.contains('{') {
            body.started = true;
        }
    }
    open.retain(|body| {
        if body.started && depth <= body.base_depth {
            out[body.index].end = at;
            false
        } else {
            true
        }
    });
}

/// Anything left open at EOF ends there.
fn close_at_eof(open: Vec<OpenBody>, out: &mut [Runnable], last: usize) {
    for body in open {
        out[body.index].end = last;
    }
}

fn rust_runnables(text: &str) -> Vec<Runnable> {
    let mut out: Vec<Runnable> = Vec::new();
    let mut open: Vec<OpenBody> = Vec::new();
    let mut pending_test = false;
    let mut pending_mod: Option<String> = None;
    let mut modules: Vec<(String, i32)> = Vec::new();
    let mut depth = 0i32;
    let mut last = 0;
    for (index, line) in text.lines().enumerate() {
        last = index;
        let trimmed = line.trim();
        if is_rust_test_attr(trimmed) {
            pending_test = true;
        }
        if let Some(name) = rust_mod_name(trimmed) {
            if trimmed.contains('{') {
                modules.push((name, depth));
            } else if !trimmed.ends_with(';') {
                pending_mod = Some(name);
            }
        } else if let Some(name) = pending_mod.take() {
            if trimmed.contains('{') {
                modules.push((name, depth));
            } else if trimmed.is_empty() || trimmed.starts_with('#') {
                pending_mod = Some(name);
            }
        }
        if let Some((name, same_line_test)) = rust_fn_name(trimmed) {
            let inner = modules
                .iter()
                .map(|(module, _)| module.as_str())
                .collect::<Vec<_>>()
                .join("::");
            let full = if inner.is_empty() {
                name.clone()
            } else {
                format!("{inner}::{name}")
            };
            let found = if name == "main" && inner.is_empty() {
                Some(("main".to_string(), Kind::Main))
            } else if pending_test || same_line_test {
                Some((full, Kind::Test))
            } else {
                None
            };
            if let Some((name, kind)) = found {
                open.push(OpenBody {
                    index: out.len(),
                    base_depth: depth,
                    started: false,
                });
                out.push(Runnable {
                    line: index,
                    end: index,
                    name,
                    kind,
                });
            }
            pending_test = false;
        } else if !trimmed.is_empty()
            && !trimmed.starts_with('#')
            // `///` and `//` between the attribute and the `fn` are the
            // test's own docs, not something that ends the attribute.
            && !trimmed.starts_with("//")
            && !is_rust_test_attr(trimmed)
            && rust_mod_name(trimmed).is_none()
        {
            pending_test = false;
        }
        depth += brace_delta(line);
        close_bodies(&mut open, &mut out, line, depth, index);
        while modules.last().is_some_and(|(_, opened)| depth <= *opened) {
            modules.pop();
        }
    }
    close_at_eof(open, &mut out, last);
    out
}

fn is_rust_test_attr(line: &str) -> bool {
    let trimmed = line.trim();
    let Some(inner) = trimmed.strip_prefix("#[") else {
        return false;
    };
    let Some(inner) = inner.strip_suffix(']') else {
        return false;
    };
    rust_attr_is_test(inner)
}

/// `#[test]` and the attribute macros that stand in for it — `tokio::test`,
/// `rstest`, `test_case(...)` — all become a libtest case with the fn's name.
fn rust_attr_is_test(inner: &str) -> bool {
    let path = inner.split('(').next().unwrap_or(inner).trim();
    matches!(path, "test" | "rstest" | "test_case" | "quickcheck") || path.ends_with("::test")
}

fn rust_mod_name(line: &str) -> Option<String> {
    let mut rest = line.trim();
    if let Some(next) = rest.strip_prefix("pub") {
        rest = next.strip_prefix(' ')?;
        if rest.starts_with('(') {
            let end = rest.find(')')?;
            rest = rest[end + 1..].trim_start();
        }
    }
    let rest = rest.strip_prefix("mod")?;
    let rest = rest.strip_prefix(' ').unwrap_or(rest).trim_start();
    ident(rest)
}

fn rust_fn_name(line: &str) -> Option<(String, bool)> {
    let mut rest = line.trim();
    let mut same_line_test = false;
    loop {
        rest = rest.trim_start();
        if let Some(inner) = rest.strip_prefix("#[") {
            let end = inner.find(']')?;
            if rust_attr_is_test(&inner[..end]) {
                same_line_test = true;
            }
            rest = inner[end + 1..].trim_start();
            continue;
        }
        if let Some(next) = rest.strip_prefix("pub") {
            rest = next.strip_prefix(' ')?;
            if rest.starts_with('(') {
                let end = rest.find(')')?;
                rest = rest[end + 1..].trim_start();
            }
            continue;
        }
        if let Some(next) = rest.strip_prefix("async") {
            rest = next.strip_prefix(' ')?;
            continue;
        }
        if let Some(next) = rest.strip_prefix("unsafe") {
            rest = next.strip_prefix(' ')?;
            continue;
        }
        if let Some(next) = rest.strip_prefix("const") {
            rest = next.strip_prefix(' ')?;
            continue;
        }
        break;
    }
    let rest = rest.strip_prefix("fn")?;
    let rest = rest.strip_prefix(' ').unwrap_or(rest).trim_start();
    ident(rest).map(|name| (name, same_line_test))
}

fn python_runnables(text: &str) -> Vec<Runnable> {
    let mut out: Vec<Runnable> = Vec::new();
    // Open blocks as (runnable index, indent of the `def`/`if` line).
    let mut open: Vec<(usize, usize)> = Vec::new();
    let mut class: Option<(String, usize)> = None;
    let mut last_code = 0;
    for (index, line) in text.lines().enumerate() {
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // A line back at (or left of) the block's indent ends it on the
        // last code line before this one.
        open.retain(|(at, opened)| {
            if indent <= *opened {
                out[*at].end = last_code;
                false
            } else {
                true
            }
        });
        last_code = index;
        if let Some(name) = py_class_name(trimmed) {
            class = Some((name, indent));
            continue;
        }
        if class.as_ref().is_some_and(|(_, at)| indent <= *at) {
            class = None;
        }
        if trimmed == "if __name__ == \"__main__\":" || trimmed == "if __name__ == '__main__':" {
            open.push((out.len(), indent));
            out.push(Runnable {
                line: index,
                end: index,
                name: "__main__".into(),
                kind: Kind::Main,
            });
            continue;
        }
        let rest = trimmed.strip_prefix("async ").unwrap_or(trimmed);
        if let Some(rest) = rest.strip_prefix("def ")
            && let Some(name) = ident(rest)
            && name.starts_with("test_")
        {
            let full = match &class {
                Some((class, at)) if indent > *at => format!("{class}::{name}"),
                _ => name,
            };
            open.push((out.len(), indent));
            out.push(Runnable {
                line: index,
                end: index,
                name: full,
                kind: Kind::Test,
            });
        }
    }
    for (at, _) in open {
        out[at].end = last_code;
    }
    out
}

fn py_class_name(line: &str) -> Option<String> {
    ident(line.strip_prefix("class ")?)
}

fn go_runnables(text: &str) -> Vec<Runnable> {
    let mut out: Vec<Runnable> = Vec::new();
    let mut open: Vec<OpenBody> = Vec::new();
    let mut depth = 0i32;
    let mut last = 0;
    for (index, line) in text.lines().enumerate() {
        last = index;
        if let Some((name, kind)) = go_fn(line.trim()) {
            open.push(OpenBody {
                index: out.len(),
                base_depth: depth,
                started: false,
            });
            out.push(Runnable {
                line: index,
                end: index,
                name,
                kind,
            });
        }
        depth += brace_delta(line);
        close_bodies(&mut open, &mut out, line, depth, index);
    }
    close_at_eof(open, &mut out, last);
    out
}

fn go_fn(line: &str) -> Option<(String, Kind)> {
    let mut rest = line.strip_prefix("func ")?.trim_start();
    if rest.starts_with('(') {
        let end = rest.find(')')?;
        rest = rest[end + 1..].trim_start();
    }
    let name = ident(rest)?;
    if name == "main" {
        Some((name, Kind::Main))
    } else if is_go_test_name(&name) {
        Some((name, Kind::Test))
    } else {
        None
    }
}

fn is_go_test_name(name: &str) -> bool {
    if name == "TestMain" {
        return false;
    }
    let rest = match name.strip_prefix("Test") {
        Some(rest) => rest,
        None => return false,
    };
    rest.is_empty() || rest.starts_with(|c: char| c.is_ascii_uppercase())
}

fn javascript_runnables(text: &str) -> Vec<Runnable> {
    let mut out: Vec<Runnable> = Vec::new();
    let mut open: Vec<OpenBody> = Vec::new();
    let mut depth = 0i32;
    let mut last = 0;
    for (index, line) in text.lines().enumerate() {
        last = index;
        let trimmed = line.trim();
        if trimmed.contains("require.main === module") || trimmed.contains("import.meta.main") {
            open.push(OpenBody {
                index: out.len(),
                base_depth: depth,
                started: false,
            });
            out.push(Runnable {
                line: index,
                end: index,
                name: "main".into(),
                kind: Kind::Main,
            });
        }
        depth += brace_delta(line);
        close_bodies(&mut open, &mut out, line, depth, index);
    }
    close_at_eof(open, &mut out, last);
    out
}

fn ruby_runnables(text: &str) -> Vec<Runnable> {
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("if __FILE__") || trimmed.starts_with("if $PROGRAM_NAME") {
            out.push(Runnable {
                line: index,
                end: index,
                name: "__FILE__".into(),
                kind: Kind::Main,
            });
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("def ")
            && let Some(name) = ident(rest)
            && name.starts_with("test_")
        {
            out.push(Runnable {
                line: index,
                end: index,
                name,
                kind: Kind::Test,
            });
        }
    }
    out
}

fn rust_command(root: &Path, file: &Path, target: Option<&Runnable>) -> Option<Command> {
    let cargo = cargo_dir(file, root)?;
    match target {
        Some(runnable) if runnable.kind == Kind::Test => {
            let filter = rust_test_filter(&cargo, file, &runnable.name);
            let extra = rust_package_args(&cargo, file);
            let shell = if extra.is_empty() {
                format!("cargo test {} -- --exact", quote(&filter))
            } else {
                format!("cargo test {extra} {} -- --exact", quote(&filter))
            };
            Some(Command {
                label: format!("Test {}", runnable.name),
                shell,
                cwd: cargo,
            })
        }
        _ => {
            let extra = rust_package_args(&cargo, file);
            let shell = if extra.starts_with("--test ") {
                format!("cargo test {extra}")
            } else if extra.is_empty() {
                "cargo run".to_string()
            } else {
                format!("cargo run {extra}")
            };
            Some(Command {
                label: "Run".into(),
                shell,
                cwd: cargo,
            })
        }
    }
}

fn rust_test_filter(cargo: &Path, file: &Path, inner: &str) -> String {
    let module = rust_file_module(cargo, file);
    if module.is_empty() {
        inner.to_string()
    } else if inner.is_empty() {
        module
    } else {
        format!("{module}::{inner}")
    }
}

fn rust_package_args(cargo: &Path, file: &Path) -> String {
    let Ok(relative) = file.strip_prefix(cargo) else {
        return String::new();
    };
    let parts: Vec<&str> = relative.iter().filter_map(|part| part.to_str()).collect();
    match parts.as_slice() {
        ["src", "bin", name] if is_rust_file(name) => {
            format!("--bin {}", quote(file_stem(name)))
        }
        ["src", "bin", name, ..] => format!("--bin {}", quote(name)),
        ["examples", name] if is_rust_file(name) => {
            format!("--example {}", quote(file_stem(name)))
        }
        ["examples", name, ..] => format!("--example {}", quote(name)),
        ["tests", name] if is_rust_file(name) => {
            format!("--test {}", quote(file_stem(name)))
        }
        ["tests", name, ..] => format!("--test {}", quote(name)),
        _ => String::new(),
    }
}

fn rust_file_module(cargo: &Path, file: &Path) -> String {
    let Ok(relative) = file.strip_prefix(cargo) else {
        return String::new();
    };
    let parts: Vec<&str> = relative.iter().filter_map(|part| part.to_str()).collect();
    let rest = match parts.as_slice() {
        ["src", "lib.rs"] | ["src", "main.rs"] => return String::new(),
        ["src", "bin", name] if is_rust_file(name) => return String::new(),
        ["src", "bin", _, "main.rs"] => return String::new(),
        ["src", "bin", _, rest @ ..] => rest,
        ["src", rest @ ..] => rest,
        ["examples", name] if is_rust_file(name) => return String::new(),
        ["examples", _, "main.rs"] => return String::new(),
        ["examples", _, rest @ ..] => rest,
        ["tests", name] if is_rust_file(name) => return String::new(),
        ["tests", _, rest @ ..] => rest,
        _ => return String::new(),
    };
    module_from_rust_parts(rest)
}

fn module_from_rust_parts(parts: &[&str]) -> String {
    let mut segs = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        if index + 1 == parts.len() {
            if matches!(*part, "mod.rs" | "lib.rs" | "main.rs") {
                break;
            }
            if let Some(stem) = part.strip_suffix(".rs") {
                segs.push(stem.to_string());
            }
        } else {
            segs.push((*part).to_string());
        }
    }
    segs.join("::")
}

fn is_rust_file(name: &str) -> bool {
    name.ends_with(".rs")
}

fn file_stem(name: &str) -> &str {
    name.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(name)
}

/// The interpreter name. macOS and most Linux distributions ship `python3`
/// only; the Windows installer ships `python`.
#[cfg(windows)]
const PYTHON: &str = "python";
#[cfg(not(windows))]
const PYTHON: &str = "python3";

fn python_command(root: &Path, file: &Path, target: Option<&Runnable>) -> Option<Command> {
    let path = relative_or_abs(root, file);
    match target {
        Some(runnable) if runnable.kind == Kind::Test => Some(Command {
            label: format!("Test {}", runnable.name),
            shell: format!(
                "{PYTHON} -m pytest {} -q",
                quote(&format!("{}::{}", path, runnable.name))
            ),
            cwd: root.to_path_buf(),
        }),
        _ => Some(Command {
            label: "Run".into(),
            shell: format!("{PYTHON} {}", quote(&path)),
            cwd: root.to_path_buf(),
        }),
    }
}

fn go_command(root: &Path, file: &Path, target: Option<&Runnable>) -> Option<Command> {
    let dir = file.parent()?.to_path_buf();
    match target {
        Some(runnable) if runnable.kind == Kind::Test => Some(Command {
            label: format!("Test {}", runnable.name),
            shell: format!("go test -run {}", quote(&format!("^{}$", runnable.name))),
            cwd: dir,
        }),
        _ => {
            let shell = if go_mod_dir(file, root).is_some() {
                "go run .".to_string()
            } else {
                format!("go run {}", quote(&relative_or_abs(&dir, file)))
            };
            Some(Command {
                label: "Run".into(),
                shell,
                cwd: dir,
            })
        }
    }
}

fn javascript_command(root: &Path, file: &Path, target: Option<&Runnable>) -> Option<Command> {
    let path = quote(&relative_or_abs(root, file));
    let test_file = is_javascript_test_file(file);
    let shell = if target.is_some_and(|runnable| runnable.kind == Kind::Test) || test_file {
        format!("node --test {path}")
    } else {
        format!("node {path}")
    };
    Some(Command {
        label: if test_file {
            "Test".into()
        } else {
            "Run".into()
        },
        shell,
        cwd: root.to_path_buf(),
    })
}

fn is_javascript_test_file(file: &Path) -> bool {
    let name = file
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    name.contains(".test.") || name.contains(".spec.") || name.ends_with("_test.js")
}

fn ruby_command(root: &Path, file: &Path, _target: Option<&Runnable>) -> Option<Command> {
    let path = quote(&relative_or_abs(root, file));
    Some(Command {
        label: "Run".into(),
        shell: format!("ruby {path}"),
        cwd: root.to_path_buf(),
    })
}

fn shell_command(root: &Path, file: &Path, shell: &str) -> Option<Command> {
    let path = quote(&relative_or_abs(root, file));
    Some(Command {
        label: "Run".into(),
        shell: format!("{shell} {path}"),
        cwd: root.to_path_buf(),
    })
}

fn cargo_dir(file: &Path, root: &Path) -> Option<PathBuf> {
    nearest_file(file, root, "Cargo.toml")
}

fn go_mod_dir(file: &Path, root: &Path) -> Option<PathBuf> {
    nearest_file(file, root, "go.mod")
}

fn nearest_file(file: &Path, root: &Path, name: &str) -> Option<PathBuf> {
    let mut dir = file.parent()?;
    loop {
        if dir.join(name).is_file() {
            return Some(dir.to_path_buf());
        }
        if dir == root {
            return None;
        }
        dir = dir.parent()?;
    }
}

fn relative_or_abs(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| file.to_string_lossy().into_owned())
}

/// Quote one argument for the shell that runs the Run tab: `sh`-style
/// single quotes on Unix, `cmd.exe` double quotes on Windows.
fn quote(value: &str) -> String {
    if value.chars().all(|c| {
        c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | '@' | '+' | '\\' | ':')
    }) && !value.is_empty()
    {
        return value.to_string();
    }
    #[cfg(windows)]
    {
        format!("\"{}\"", value.replace('"', "\"\""))
    }
    #[cfg(not(windows))]
    {
        format!("'{}'", value.replace('\'', r"'\''"))
    }
}

fn ident(rest: &str) -> Option<String> {
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

fn brace_delta(line: &str) -> i32 {
    let mut delta = 0i32;
    let mut in_str = false;
    let mut in_char = false;
    let mut escape = false;
    for c in line.chars() {
        if escape {
            escape = false;
            continue;
        }
        if c == '\\' && (in_str || in_char) {
            escape = true;
            continue;
        }
        if !in_str && c == '\'' {
            in_char = !in_char;
            continue;
        }
        if !in_char && c == '"' {
            in_str = !in_str;
            continue;
        }
        if in_str || in_char {
            continue;
        }
        match c {
            '{' => delta += 1,
            '}' => delta -= 1,
            _ => {}
        }
    }
    delta
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_at(line: usize, name: &str) -> Runnable {
        Runnable {
            line,
            end: line,
            name: name.into(),
            kind: Kind::Test,
        }
    }

    #[test]
    fn rust_finds_main_and_tests() {
        let text = "\
fn helper() {}\n\
#[test]\n\
fn adds() {}\n\
pub async fn main() {}\n\
#[tokio::test]\n\
async fn waits() {}\n\
#[cfg(test)]\n\
fn not_a_test() {}\n\
#[test] fn same_line() {}\n";
        let found = rust_runnables(text);
        assert_eq!(
            found
                .iter()
                .map(|r| (r.line, r.name.as_str(), r.kind))
                .collect::<Vec<_>>(),
            vec![
                (2, "adds", Kind::Test),
                (3, "main", Kind::Main),
                (5, "waits", Kind::Test),
                (8, "same_line", Kind::Test),
            ]
        );
    }

    #[test]
    fn rust_names_tests_inside_modules() {
        let text = "\
#[cfg(test)]\n\
mod tests {\n\
    #[test]\n\
    fn adds() {}\n\
}\n";
        let found = rust_runnables(text);
        assert_eq!(found[0].name, "tests::adds");
        assert_eq!(found[0].kind, Kind::Test);
    }

    #[test]
    fn rust_docs_between_the_attribute_and_the_fn_keep_the_test() {
        let text = "\
#[test]\n\
/// Two and two.\n\
// and a plain comment\n\
fn adds() {}\n\
#[rstest]\n\
fn cases() {}\n\
#[test_case(1, 2)]\n\
fn table(a: i32, b: i32) {}\n";
        let found = rust_runnables(text);
        assert_eq!(
            found.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["adds", "cases", "table"]
        );
    }

    #[test]
    fn rust_bodies_span_their_braces() {
        let text = "\
fn main()\n\
{\n\
    let s = \"}\";\n\
    helper();\n\
}\n\
\n\
#[cfg(test)]\n\
mod tests {\n\
    #[test]\n\
    fn adds() {\n\
        assert_eq!(2 + 2, 4);\n\
    }\n\
\n\
    #[test] fn one_liner() {}\n\
}\n";
        let found = rust_runnables(text);
        assert_eq!(
            found
                .iter()
                .map(|r| (r.name.as_str(), r.line, r.end))
                .collect::<Vec<_>>(),
            vec![
                ("main", 0, 4),
                ("tests::adds", 9, 11),
                ("tests::one_liner", 13, 13)
            ]
        );
    }

    #[test]
    fn python_finds_main_and_tests() {
        let text = "\
def helper():\n    pass\n\
def test_ok():\n    pass\n\
class TestBox:\n    def test_in(self):\n        pass\n\
if __name__ == '__main__':\n    test_ok()\n";
        let found = python_runnables(text);
        assert_eq!(found[0].name, "test_ok");
        assert_eq!(found[0].kind, Kind::Test);
        assert_eq!((found[0].line, found[0].end), (2, 3));
        assert_eq!(found[1].name, "TestBox::test_in");
        assert_eq!(found[1].kind, Kind::Test);
        assert_eq!((found[1].line, found[1].end), (5, 6));
        assert_eq!(found[2].name, "__main__");
        assert_eq!(found[2].kind, Kind::Main);
        assert_eq!((found[2].line, found[2].end), (7, 8));
    }

    #[test]
    fn rust_command_uses_cargo_when_present() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname=\"x\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        let file = root.join("src/main.rs");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "fn main() {}\n#[test]\nfn adds() {}\n").unwrap();
        let runnables = find(&file, "fn main() {}\n#[test]\nfn adds() {}\n");
        let main = runnables.iter().find(|r| r.kind == Kind::Main).unwrap();
        let test = runnables.iter().find(|r| r.kind == Kind::Test).unwrap();
        let run = command(root, &file, Some(main)).unwrap();
        assert_eq!(run.shell, "cargo run");
        assert_eq!(run.cwd, root);
        assert_eq!(
            command(root, &file, Some(test)).unwrap().shell,
            "cargo test adds -- --exact"
        );
        assert_eq!(
            resolve(root, &file, "fn main() {}\n", Some(0))
                .unwrap()
                .shell,
            "cargo run"
        );
        assert!(can_run(root, &file));
    }

    #[test]
    fn the_caret_picks_the_test_it_sits_in_and_nothing_else() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname=\"x\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        let file = root.join("src/main.rs");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let text = "\
fn main() {\n\
    helper();\n\
}\n\
\n\
fn helper() {\n\
}\n\
\n\
#[test]\n\
fn adds() {\n\
    assert!(true);\n\
}\n\
\n\
// trailing\n";
        std::fs::write(&file, text).unwrap();
        let shell = |line| resolve(root, &file, text, Some(line)).unwrap().shell;
        assert_eq!(shell(1), "cargo run", "inside main");
        assert_eq!(
            shell(9),
            "cargo test adds -- --exact",
            "inside the test body"
        );
        assert_eq!(
            shell(10),
            "cargo test adds -- --exact",
            "on the closing brace"
        );
        assert_eq!(
            shell(5),
            "cargo run",
            "a helper below main is not 'the nearest runnable above'"
        );
        assert_eq!(shell(12), "cargo run", "past the last test runs the crate");
        assert_eq!(shell(7), "cargo run", "the attribute line is not the body");
    }

    #[test]
    fn rust_bin_and_example_files_name_the_target() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname=\"x\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        let bin = root.join("src/bin/cli.rs");
        std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
        std::fs::write(&bin, "fn main() {}\n#[test]\nfn adds() {}\n").unwrap();
        let example = root.join("examples/demo.rs");
        std::fs::create_dir_all(example.parent().unwrap()).unwrap();
        std::fs::write(&example, "fn main() {}\n").unwrap();
        let nested = root.join("src/foo.rs");
        std::fs::write(&nested, "#[test]\nfn adds() {}\n").unwrap();
        assert_eq!(
            command(root, &bin, None).unwrap().shell,
            "cargo run --bin cli"
        );
        assert_eq!(
            command(root, &bin, Some(&test_at(1, "adds")))
                .unwrap()
                .shell,
            "cargo test --bin cli adds -- --exact"
        );
        assert_eq!(
            command(root, &example, None).unwrap().shell,
            "cargo run --example demo"
        );
        assert_eq!(
            command(root, &nested, Some(&test_at(1, "adds")))
                .unwrap()
                .shell,
            "cargo test foo::adds -- --exact"
        );
    }

    #[test]
    fn python_command_points_at_the_file() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let file = root.join("app.py");
        std::fs::write(&file, "def test_ok():\n    pass\n").unwrap();
        let run = command(root, &file, None).unwrap();
        assert_eq!(run.shell, format!("{PYTHON} app.py"));
        assert_eq!(run.cwd, root);
        let test = &find(&file, "def test_ok():\n    pass\n")[0];
        assert_eq!(
            command(root, &file, Some(test)).unwrap().shell,
            format!("{PYTHON} -m pytest app.py::test_ok -q")
        );
    }

    #[test]
    fn rust_without_cargo_is_not_runnable_as_a_project() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("lone.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        assert!(command(tmp.path(), &file, None).is_none());
        assert!(!can_run(tmp.path(), &file));
    }

    #[test]
    fn go_and_javascript_and_shell_have_file_commands() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let go = root.join("main.go");
        let go_text = "package main\nfunc main() {\n}\nfunc TestAdd(t *testing.T) {}\n";
        std::fs::write(&go, go_text).unwrap();
        std::fs::write(root.join("go.mod"), "module demo\n").unwrap();
        let found = find(&go, go_text);
        assert_eq!(found[0].kind, Kind::Main);
        assert_eq!((found[0].line, found[0].end), (1, 2));
        assert_eq!(found[1].name, "TestAdd");
        let run = command(root, &go, None).unwrap();
        assert_eq!(run.shell, "go run .");
        assert_eq!(run.cwd, root);
        assert_eq!(
            command(root, &go, Some(&found[1])).unwrap().shell,
            "go test -run '^TestAdd$'"
        );

        let js = root.join("app.js");
        std::fs::write(&js, "if (require.main === module) {}\n").unwrap();
        assert_eq!(command(root, &js, None).unwrap().shell, "node app.js");
        let spec = root.join("app.test.js");
        std::fs::write(&spec, "").unwrap();
        assert_eq!(
            command(root, &spec, None).unwrap().shell,
            "node --test app.test.js"
        );

        let sh = root.join("run.sh");
        std::fs::write(&sh, "echo hi\n").unwrap();
        assert_eq!(command(root, &sh, None).unwrap().shell, "bash run.sh");
    }

    #[test]
    fn nested_crate_runs_from_its_own_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let crate_dir = root.join("example/hello_rust");
        std::fs::create_dir_all(crate_dir.join("src")).unwrap();
        std::fs::write(
            crate_dir.join("Cargo.toml"),
            "[package]\nname=\"hello_rust\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        let file = crate_dir.join("src/main.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        let run = command(root, &file, None).unwrap();
        assert_eq!(run.shell, "cargo run");
        assert_eq!(
            run.cwd, crate_dir,
            "the pty starts in the crate, so the command never has to cd"
        );
    }

    #[test]
    fn quoting_keeps_plain_paths_plain() {
        assert_eq!(quote("src/main.rs"), "src/main.rs");
        assert_eq!(quote("foo::adds"), "foo::adds");
        assert_eq!(quote(r"C:\src\main.rs"), r"C:\src\main.rs");
        #[cfg(not(windows))]
        {
            assert_eq!(quote("it's"), r"'it'\''s'");
            assert_eq!(quote("^TestAdd$"), "'^TestAdd$'");
        }
        #[cfg(windows)]
        assert_eq!(quote("a b"), "\"a b\"");
    }
}
