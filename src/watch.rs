//! Debounced filesystem events for open projects.
//!
//! Folio only needs two things from a watch: which cached folders to re-read,
//! and which open files may no longer match the disk. The watcher itself is
//! a thin `notify` wrapper; filtering lives here so tests can drive it
//! without standing up a real observer.

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// How long a Folio write is ignored so our own save does not look like
/// someone else's edit.
pub const QUIET_AFTER_WRITE: Duration = Duration::from_millis(800);

/// How long events sit before they are applied together.
pub const DEBOUNCE: Duration = Duration::from_millis(200);

/// A live recursive watch on the open project roots. Dropping it stops the
/// kernel observer. Events land in `pending` for the UI loop to drain.
pub struct DiskWatch {
    _watcher: Option<RecommendedWatcher>,
    pending: Arc<Mutex<Vec<PathBuf>>>,
}

impl Default for DiskWatch {
    fn default() -> Self {
        Self {
            _watcher: None,
            pending: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl DiskWatch {
    /// Watch exactly `roots`. An empty list drops the observer.
    pub fn sync(&mut self, roots: &[PathBuf]) {
        if roots.is_empty() {
            self._watcher = None;
            return;
        }
        let pending = self.pending.clone();
        let mut watcher = match RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res
                    && let Ok(mut queue) = pending.lock()
                {
                    queue.extend(event.paths);
                }
            },
            notify::Config::default(),
        ) {
            Ok(watcher) => watcher,
            Err(_) => {
                self._watcher = None;
                return;
            }
        };
        for root in roots {
            let _ = watcher.watch(root, RecursiveMode::Recursive);
        }
        self._watcher = Some(watcher);
    }

    pub fn take(&self) -> Vec<PathBuf> {
        self.pending
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default()
    }
}

/// A path Folio should not treat as a user-visible tree or buffer change:
/// anything under an ignored directory name. `.git` is ignored for the tree
/// but still asked for as a git-status refresh.
pub fn under_ignored(path: &Path, ignored: &[String]) -> bool {
    path.components().any(|component| {
        ignored
            .iter()
            .any(|name| component.as_os_str() == name.as_str())
    })
}

pub fn is_git_metadata(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == ".git")
}

pub fn is_quiet(path: &Path, quiet: &HashMap<PathBuf, Instant>, now: Instant) -> bool {
    quiet.get(path).is_some_and(|until| now < *until)
}

/// Collapse a burst of events into the work Folio has to do.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Batch {
    pub directories: Vec<PathBuf>,
    pub files: Vec<PathBuf>,
    pub git: bool,
}

/// `cached` is the set of folders Folio has listings for. `open` is the set
/// of buffers. Both are matched exactly; the caller normalizes paths first.
pub fn batch(
    paths: impl IntoIterator<Item = PathBuf>,
    ignored: &[String],
    quiet: &HashMap<PathBuf, Instant>,
    now: Instant,
    cached: &HashSet<PathBuf>,
    open: &HashSet<PathBuf>,
) -> Batch {
    let mut directories = HashSet::new();
    let mut files = HashSet::new();
    let mut git = false;
    for path in paths {
        if is_quiet(&path, quiet, now) {
            continue;
        }
        if is_git_metadata(&path) {
            git = true;
            continue;
        }
        if under_ignored(&path, ignored) {
            continue;
        }
        git = true;
        if open.contains(&path) {
            files.insert(path.clone());
        }
        if cached.contains(&path) {
            directories.insert(path.clone());
        }
        if let Some(parent) = path.parent()
            && cached.contains(parent)
        {
            directories.insert(parent.to_path_buf());
        }
    }
    let mut directories: Vec<_> = directories.into_iter().collect();
    let mut files: Vec<_> = files.into_iter().collect();
    directories.sort();
    files.sort();
    Batch {
        directories,
        files,
        git,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ignored() -> Vec<String> {
        crate::tree::DEFAULT_IGNORED
            .iter()
            .map(|name| (*name).to_string())
            .collect()
    }

    #[test]
    fn skips_ignored_trees_but_keeps_git_refresh() {
        let root = PathBuf::from("/repo");
        let cached = HashSet::from([root.clone()]);
        let open = HashSet::new();
        let now = Instant::now();
        let batch = batch(
            [
                root.join("node_modules/pkg/index.js"),
                root.join(".git/HEAD"),
                root.join("src/main.rs"),
            ],
            &ignored(),
            &HashMap::new(),
            now,
            &cached,
            &open,
        );
        assert!(batch.git);
        // `src` is not in the cache, so a file inside it does not refresh
        // the root listing — expanding `src` later reads the disk anyway.
        assert!(batch.directories.is_empty());
        assert!(batch.files.is_empty());
    }

    #[test]
    fn open_file_and_its_folder_are_both_reported() {
        let root = PathBuf::from("/repo");
        let file = root.join("src/main.rs");
        let cached = HashSet::from([root.clone(), root.join("src")]);
        let open = HashSet::from([file.clone()]);
        let batch = batch(
            [file.clone()],
            &ignored(),
            &HashMap::new(),
            Instant::now(),
            &cached,
            &open,
        );
        assert_eq!(batch.files, vec![file]);
        assert_eq!(batch.directories, vec![root.join("src")]);
        assert!(batch.git);
    }

    #[test]
    fn a_just_written_path_is_silent() {
        let file = PathBuf::from("/repo/src/main.rs");
        let mut quiet = HashMap::new();
        let now = Instant::now();
        quiet.insert(file.clone(), now + QUIET_AFTER_WRITE);
        let batch = batch(
            [file.clone()],
            &ignored(),
            &quiet,
            now,
            &HashSet::from([PathBuf::from("/repo/src")]),
            &HashSet::from([file]),
        );
        assert_eq!(batch, Batch::default());
    }
}
