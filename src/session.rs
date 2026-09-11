//! What was open when the app last closed, and what had not been saved when it
//! stopped.
//!
//! Two files, both local and both plain JSON. The session is the projects that
//! were open and the files they had showing, written when any of that changes.
//! The recovery file is the unsaved buffers, written every few seconds while
//! there are any and removed on a clean exit — so what is left in it is exactly
//! what a crash, a force quit or a power cut took with it.
//!
//! Nothing here reaches the network, and a path that is no longer on disk is
//! dropped as the file is read rather than failing the whole thing: a session
//! written yesterday may name a folder that was deleted today.

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

/// A project that was open, and what it had showing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionProject {
    pub root: PathBuf,
    /// The open files, in strip order.
    #[serde(default)]
    pub tabs: Vec<PathBuf>,
    #[serde(default)]
    pub active: Option<PathBuf>,
}

/// Every project that was open, in the order they were opened.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    #[serde(default)]
    pub projects: Vec<SessionProject>,
}

impl Session {
    /// Read a session, dropping projects that are no longer folders and files
    /// that are no longer files.
    pub fn load(file: &Path) -> Session {
        let Ok(raw) = fs::read(file) else {
            return Session::default();
        };
        let Ok(mut session) = serde_json::from_slice::<Session>(&raw) else {
            // A session that cannot be read is not worth stopping for: the next
            // write replaces it.
            return Session::default();
        };
        session.projects.retain(|project| project.root.is_dir());
        for project in &mut session.projects {
            project.tabs.retain(|tab| tab.is_file());
            if project
                .active
                .as_ref()
                .is_some_and(|active| !active.is_file())
            {
                project.active = None;
            }
        }
        session
    }

    pub fn save(&self, file: &Path) -> io::Result<()> {
        write(file, self)
    }
}

/// The unsaved buffers of every project, by path.
pub type UnsavedBuffers = BTreeMap<PathBuf, String>;

/// Read back what was unsaved, dropping anything whose file has since gone.
pub fn load_unsaved(file: &Path) -> UnsavedBuffers {
    let Ok(raw) = fs::read(file) else {
        return UnsavedBuffers::new();
    };
    let Ok(buffers) = serde_json::from_slice::<UnsavedBuffers>(&raw) else {
        return UnsavedBuffers::new();
    };
    buffers
        .into_iter()
        .filter(|(path, _)| path.is_file())
        .collect()
}

pub fn save_unsaved(file: &Path, buffers: &UnsavedBuffers) -> io::Result<()> {
    write(file, buffers)
}

/// Forget the unsaved buffers — a clean exit, so there is nothing to recover.
pub fn clear_unsaved(file: &Path) -> io::Result<()> {
    match fs::remove_file(file) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

/// Write beside the real file and rename over it, so a crash while writing
/// cannot leave a half-file where a session used to be.
fn write<T: Serialize>(file: &Path, value: &T) -> io::Result<()> {
    let parent = file
        .parent()
        .ok_or_else(|| io::Error::other("Invalid config path"))?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(&serde_json::to_vec(value)?)?;
    temp.as_file().sync_all()?;
    temp.persist(file).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
        let project = root.join("project");
        let file = project.join("main.rs");
        let other = project.join("notes.md");
        std::fs::create_dir_all(&project).unwrap();
        for path in [&file, &other] {
            std::fs::write(path, "// x\n").unwrap();
        }
        (project, file, other)
    }

    #[test]
    fn a_session_round_trips() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let (project, file, other) = paths(&root);
        let session = Session {
            projects: vec![SessionProject {
                root: project.clone(),
                tabs: vec![file.clone(), other.clone()],
                active: Some(other.clone()),
            }],
        };

        let path = root.join("session.json");
        session.save(&path).unwrap();
        assert_eq!(Session::load(&path), session);
    }

    #[test]
    fn what_is_gone_is_dropped_rather_than_failing() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let (project, file, other) = paths(&root);
        let session = Session {
            projects: vec![
                SessionProject {
                    root: project.clone(),
                    tabs: vec![file.clone(), other.clone()],
                    active: Some(other.clone()),
                },
                SessionProject {
                    root: root.join("missing"),
                    tabs: vec![root.join("missing/main.rs")],
                    active: Some(root.join("missing/main.rs")),
                },
            ],
        };
        let path = root.join("session.json");
        session.save(&path).unwrap();

        std::fs::remove_file(&other).unwrap();
        let loaded = Session::load(&path);
        assert_eq!(loaded.projects.len(), 1);
        assert_eq!(loaded.projects[0].tabs, vec![file.clone()]);
        // The file it was showing is gone too, so there is nothing to show.
        assert_eq!(loaded.projects[0].active, None);
    }

    #[test]
    fn a_missing_or_corrupt_file_is_an_empty_session() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        assert_eq!(
            Session::load(&root.join("nothing.json")),
            Session::default()
        );

        let corrupt = root.join("session.json");
        std::fs::write(&corrupt, "{ not json").unwrap();
        assert_eq!(Session::load(&corrupt), Session::default());
    }

    #[test]
    fn unsaved_buffers_round_trip_and_are_forgotten_on_request() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let (_, file, other) = paths(&root);
        let path = root.join("unsaved.json");

        let mut buffers = UnsavedBuffers::new();
        buffers.insert(file.clone(), "// edited\n".to_string());
        buffers.insert(other.clone(), "// also edited\n".to_string());
        save_unsaved(&path, &buffers).unwrap();
        assert_eq!(load_unsaved(&path), buffers);

        std::fs::remove_file(&other).unwrap();
        let loaded = load_unsaved(&path);
        assert_eq!(loaded.len(), 1);
        assert!(loaded.contains_key(&file));

        clear_unsaved(&path).unwrap();
        assert!(load_unsaved(&path).is_empty());
        // Clearing one that is not there is not an error.
        clear_unsaved(&path).unwrap();
    }
}
