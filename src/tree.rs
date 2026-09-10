use std::{
    fs, io,
    path::{Path, PathBuf},
};

const IGNORED: &[&str] = &[
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

pub fn children(dir: &Path) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if IGNORED.contains(&name.as_str()) {
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

pub fn index(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut stack = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(dir) = stack.pop() {
        for entry in children(&dir)? {
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
