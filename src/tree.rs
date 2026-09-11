use std::{
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
/// everything named in `ignored` left out.
pub fn children(dir: &Path, ignored: &[String]) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if ignored.contains(&name) {
            continue;
        }
        let kind = entry.file_type()?;
        // ponytail: skip symlinks to avoid cycles and workspace escapes; add explicit link support later.
        if !kind.is_dir() && !kind.is_file() {
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

pub fn index(root: &Path, ignored: &[String]) -> io::Result<Vec<PathBuf>> {
    let mut stack = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(dir) = stack.pop() {
        for entry in children(&dir, ignored)? {
            match entry.kind {
                EntryKind::Directory => stack.push(entry.path),
                EntryKind::File => files.push(entry.path),
            }
        }
        if files.len() > 100_000 {
            return Err(io::Error::other(
                "Quick open index exceeds 100,000 files; the project tree still works",
            ));
        }
    }
    files.sort();
    Ok(files)
}

pub fn fuzzy_match(query: &str, candidate: &str) -> bool {
    let candidate = candidate.to_lowercase();
    let mut chars = candidate.chars();
    query
        .to_lowercase()
        .chars()
        .all(|wanted| chars.any(|c| c == wanted))
}
