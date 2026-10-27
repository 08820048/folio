use ignore::{
    Match, WalkBuilder,
    gitignore::{Gitignore, GitignoreBuilder},
};
use std::{
    cell::RefCell,
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

/// Folder names a walk skips unless the user says otherwise. `settings` seeds
/// its own copy from this, so the default and the settings file cannot drift.
pub const DEFAULT_IGNORED: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    "vendor",
    ".DS_Store",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
}
#[derive(Debug, Clone)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub kind: EntryKind,
}

/// The entries directly inside `dir`, sorted folders-first then by name, with
/// everything named in `ignored` left out. `.gitignore` is resolved from `dir`
/// upward to `root`; use [`children_in`] with the project root when there is
/// one, and this wrapper when there is not.
pub fn children(dir: &Path, ignored: &[String]) -> io::Result<Vec<Entry>> {
    children_in(dir, dir, ignored)
}

/// [`children`], with `root` as the top of the `.gitignore` search and the
/// base of the ignore list.
pub fn children_in(root: &Path, dir: &Path, ignored: &[String]) -> io::Result<Vec<Entry>> {
    let rules = IgnoreRules::new(root, ignored);
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        // The settings list is the cheap path and also covers names that are
        // not in any `.gitignore`.
        if ignored.contains(&name) {
            continue;
        }
        let kind = entry.file_type()?;
        // ponytail: skip symlinks to avoid cycles and workspace escapes; add explicit link support later.
        if !kind.is_dir() && !kind.is_file() {
            continue;
        }
        if rules.is_ignored(&entry.path(), kind.is_dir()) {
            continue;
        }
        entries.push(Entry {
            path: entry.path(),
            name,
            kind: if kind.is_dir() {
                EntryKind::Directory
            } else {
                EntryKind::File
            },
        });
    }
    entries.sort_by_key(|e| (matches!(e.kind, EntryKind::File), e.name.to_lowercase()));
    Ok(entries)
}

/// `.gitignore` and `.git/info/exclude` for one project.
///
/// A pattern is relative to the directory holding the file it came from, so
/// each such file gets its own matcher. A path is checked from the deepest
/// directory upward and the first matcher with an opinion wins — which is what
/// lets a nested `!keep` re-include a file a parent pattern excluded.
///
/// Settings names still apply on top and always win. The cache lives for one
/// listing or one index walk; `.gitignore` changes are picked up by the next.
pub struct IgnoreRules {
    root: PathBuf,
    names: Vec<String>,
    matchers: RefCell<HashMap<PathBuf, Option<Gitignore>>>,
}

impl IgnoreRules {
    pub fn new(root: &Path, ignored: &[String]) -> Self {
        Self {
            root: root.to_path_buf(),
            names: ignored.to_vec(),
            matchers: RefCell::new(HashMap::new()),
        }
    }

    /// Whether `path` is excluded by the settings list or by a `.gitignore`
    /// between it and the project root.
    pub fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        let relative = path.strip_prefix(&self.root).unwrap_or(path);
        // `.git` is never a tree entry, whatever the settings say.
        if relative
            .components()
            .next()
            .is_some_and(|component| component.as_os_str() == ".git")
        {
            return true;
        }
        if relative.components().any(|component| {
            self.names
                .iter()
                .any(|name| component.as_os_str() == name.as_str())
        }) {
            return true;
        }
        if !path.starts_with(&self.root) {
            return false;
        }
        let mut dir = path.parent();
        while let Some(current) = dir {
            if !current.starts_with(&self.root) {
                break;
            }
            if let Some(ignored) = self.decision(current, path, is_dir) {
                return ignored;
            }
            if current == self.root {
                break;
            }
            dir = current.parent();
        }
        false
    }

    fn decision(&self, dir: &Path, path: &Path, is_dir: bool) -> Option<bool> {
        let mut matchers = self.matchers.borrow_mut();
        let matcher = matchers
            .entry(dir.to_path_buf())
            .or_insert_with(|| build_matcher(dir));
        let matcher = matcher.as_ref()?;
        match matcher.matched_path_or_any_parents(path, is_dir) {
            Match::Ignore(_) => Some(true),
            Match::Whitelist(_) => Some(false),
            Match::None => None,
        }
    }
}

fn build_matcher(dir: &Path) -> Option<Gitignore> {
    let gitignore = dir.join(".gitignore");
    let exclude = dir.join(".git/info/exclude");
    if !gitignore.is_file() && !exclude.is_file() {
        return None;
    }
    let mut builder = GitignoreBuilder::new(dir);
    if gitignore.is_file() {
        let _ = builder.add(&gitignore);
    }
    if exclude.is_file() {
        let _ = builder.add(&exclude);
    }
    builder.build().ok()
}

pub fn index(root: &Path, ignored: &[String]) -> io::Result<Vec<PathBuf>> {
    // `WalkBuilder` follows nested `.gitignore` files and skips ignored
    // directories before reading them, which the old hand-written stack could
    // not do. `ignore(false)`: only git's rules, not ripgrep's `.ignore` files.
    let ignored = ignored.to_vec();
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .parents(false)
        .ignore(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(true)
        .require_git(false)
        .follow_links(false)
        .filter_entry(move |entry| {
            if entry.depth() == 0 {
                return true;
            }
            let name = entry.file_name().to_string_lossy();
            name != ".git" && !ignored.iter().any(|ignored| ignored == name.as_ref())
        });

    let mut files = Vec::new();
    for entry in builder.build() {
        let Ok(entry) = entry else {
            continue;
        };
        if entry.depth() == 0 {
            continue;
        }
        if entry.file_type().is_some_and(|kind| kind.is_file()) {
            files.push(entry.path().to_path_buf());
            if files.len() > 100_000 {
                return Err(io::Error::other(
                    "Quick open index exceeds 100,000 files; the project tree still works",
                ));
            }
        }
    }
    files.sort();
    Ok(files)
}

pub fn fuzzy_match(query: &str, candidate: &str) -> bool {
    fuzzy_match_lower(&query.to_lowercase(), &candidate.to_lowercase())
}

/// [`fuzzy_match`] when both sides are already lowercase. Quick open keeps a
/// lowercased name per indexed file so a keystroke allocates only the query.
pub fn fuzzy_match_lower(query: &str, candidate: &str) -> bool {
    fuzzy_score(query, candidate).is_some()
}

/// Rank `query` against a file name. Both sides are expected to be lowercase.
///
/// The order is the one a person expects from a file picker: an exact name,
/// then a prefix, then a substring, then characters in order with a penalty
/// for the gaps between them. `None` means the characters are not all there in
/// order. The caller adds path depth and recency; this only judges the name.
pub fn fuzzy_score(query: &str, candidate: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let query_len = query.chars().count() as i32;
    let candidate_len = candidate.chars().count() as i32;
    if candidate == query {
        return Some(2_000);
    }
    if candidate.starts_with(query) {
        return Some(1_600 - (candidate_len - query_len));
    }
    if let Some(byte_index) = candidate.find(query) {
        let before = candidate[..byte_index].chars().count() as i32;
        return Some(1_200 - before * 8 - (candidate_len - query_len));
    }

    let mut rest = candidate.char_indices();
    let mut previous: Option<i32> = None;
    let mut gaps = 0;
    let mut adjacent = 0;
    for wanted in query.chars() {
        let mut found = None;
        for (byte_index, character) in rest.by_ref() {
            if character == wanted {
                found = Some(candidate[..byte_index].chars().count() as i32);
                break;
            }
        }
        let position = found?;
        if let Some(previous) = previous {
            let gap = position - previous - 1;
            gaps += gap;
            if gap == 0 {
                adjacent += 1;
            }
        }
        previous = Some(position);
    }
    Some(600 + adjacent * 12 - gaps * 3 - (candidate_len - query_len))
}

#[cfg(test)]
mod score_tests {
    use super::*;

    #[test]
    fn exact_then_prefix_then_substring_then_subsequence() {
        let exact = fuzzy_score("main", "main").unwrap();
        let prefix = fuzzy_score("main", "main.rs").unwrap();
        let longer_prefix = fuzzy_score("main", "main_test.rs").unwrap();
        let substring = fuzzy_score("main", "domain.rs").unwrap();
        let subsequence = fuzzy_score("main", "m_a_i_n.rs").unwrap();

        assert!(exact > prefix, "{exact} > {prefix}");
        assert!(prefix > longer_prefix, "{prefix} > {longer_prefix}");
        assert!(longer_prefix > substring, "{longer_prefix} > {substring}");
        assert!(substring > subsequence, "{substring} > {subsequence}");
        assert_eq!(fuzzy_score("rsm", "main.rs"), None);
        assert!(fuzzy_match_lower("mrs", "main.rs"));
        assert_eq!(fuzzy_score("", "anything"), Some(0));
    }
}

#[cfg(test)]
mod perf_tests {
    use super::*;
    use std::{fs, time::Instant};

    /// Run with `cargo test --release --features desktop-tests --lib records_index -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn records_index_and_read_times() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for i in 0..25 {
            let folder = root.join(format!("src{i}"));
            fs::create_dir(&folder).unwrap();
            for j in 0..100 {
                fs::write(folder.join(format!("f{j}.rs")), "fn x() {}\n").unwrap();
            }
        }
        fs::write(
            root.join("big.rs"),
            format!("// {}\n", "a".repeat(1024 * 1024)),
        )
        .unwrap();
        let ignored: Vec<String> = DEFAULT_IGNORED
            .iter()
            .map(|name| (*name).to_string())
            .collect();
        let started = Instant::now();
        let files = index(root, &ignored).unwrap();
        let index_ms = started.elapsed().as_secs_f64() * 1000.0;
        let big = files
            .iter()
            .find(|path| path.file_name().is_some_and(|name| name == "big.rs"))
            .unwrap();
        let started = Instant::now();
        let text = crate::buffer::read(big).unwrap();
        let read_ms = started.elapsed().as_secs_f64() * 1000.0;
        println!(
            "index_files={} index_ms={index_ms:.1} read_bytes={} read_ms={read_ms:.1}",
            files.len(),
            text.len()
        );
        assert!(files.len() >= 2500);
        assert!(text.len() > 1024 * 1024);
    }
}
