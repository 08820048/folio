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
    pending: Arc<Mutex<HashMap<PathBuf, bool>>>,
}

impl Default for DiskWatch {
    fn default() -> Self {
        Self {
            _watcher: None,
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl DiskWatch {
    /// Watch exactly `roots`. An empty list drops the observer.
    pub fn sync(&mut self, roots: &[PathBuf], ignored: &[String]) {
        if roots.is_empty() {
            self._watcher = None;
            return;
        }
        let pending = self.pending.clone();
        let watched = roots.to_vec();
        let ignored = ignored.to_vec();
        let mut watcher = match RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res
                    && let Ok(mut queue) = pending.lock()
                {
                    collect_event(&mut queue, event, &watched, &ignored);
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

    pub fn take(&self) -> HashMap<PathBuf, bool> {
        self.pending
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default()
    }
}

/// The bool distinguishes tree changes from ordinary content/metadata writes.
fn collect_event(queue: &mut HashMap<PathBuf, bool>, event: Event, roots: &[PathBuf], ignored: &[String]) {
    use notify::{EventKind, event::ModifyKind};
    if matches!(event.kind, EventKind::Access(_)) { return; }
    if event.need_rescan() || queue.len() >= 4096 {
        // Bound event bursts without dropping work: a root means a full refresh.
        queue.clear();
        queue.extend(roots.iter().cloned().map(|root| (root, true)));
        return;
    }
    let structure = !matches!(event.kind, EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Metadata(_)));
    for path in event.paths {
        let Some(root) = roots.iter().filter(|root| path.starts_with(root)).max_by_key(|root| root.components().count()) else { continue; };
        let relative = path.strip_prefix(root).unwrap();
        let git = is_git_metadata(relative);
        if !git && under_ignored(relative, ignored) { continue; }
        let path = if git { root.join(".git") } else { path };
        queue.entry(path).and_modify(|changed| *changed |= structure).or_insert(structure);
        if queue.len() >= 4096 {
            queue.clear();
            queue.extend(roots.iter().cloned().map(|root| (root, true)));
            break;
        }
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

    #[test]
    fn event_bursts_are_filtered_coalesced_and_bounded() {
        use notify::{EventKind, event::{AccessKind, CreateKind, DataChange, ModifyKind}};
        let root = PathBuf::from("/repo");
        let roots = [root.clone()];
        let mut queue = HashMap::new();
        for (kind, path) in [
            (EventKind::Access(AccessKind::Read), "src/read.rs"),
            (EventKind::Create(CreateKind::File), "node_modules/pkg/index.js"),
            (EventKind::Modify(ModifyKind::Data(DataChange::Content)), "src/main.rs"),
            (EventKind::Modify(ModifyKind::Data(DataChange::Content)), ".git/HEAD"),
        ] {
            collect_event(&mut queue, Event::new(kind).add_path(root.join(path)), &roots, &ignored());
        }
        assert_eq!(queue.len(), 2);
        assert_eq!(queue[&root.join("src/main.rs")], false);
        assert!(queue.contains_key(&root.join(".git")));
        collect_event(&mut queue, Event::new(EventKind::Create(CreateKind::File)).add_path(root.join("src/main.rs")), &roots, &ignored());
        assert!(queue[&root.join("src/main.rs")]);
        for n in 0..10_000 {
            collect_event(&mut queue, Event::new(EventKind::Create(CreateKind::File)).add_path(root.join(n.to_string())), &roots, &ignored());
            assert!(queue.len() <= 4096);
        }
        assert_eq!(queue.get(&root), Some(&true), "overflow must request a complete refresh");
    }

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
