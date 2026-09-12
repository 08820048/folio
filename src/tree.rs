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
        fs::write(root.join("big.rs"), format!("// {}\n", "a".repeat(1024 * 1024))).unwrap();
        let ignored: Vec<String> = DEFAULT_IGNORED.iter().map(|name| (*name).to_string()).collect();
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
