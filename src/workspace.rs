use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
}
impl Workspace {
    pub fn open(path: &Path) -> io::Result<Self> {
        let root = canonical(path)?;
        if !root.is_dir() {
            return Err(io::Error::other("Project path is not a folder"));
        }
        Ok(Self { root })
    }
    pub fn resolve(&self, path: &Path) -> io::Result<PathBuf> {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        let path = canonical(&path)?;
        if !path.starts_with(&self.root) {
            return Err(io::Error::other("Path is outside the current project"));
        }
        Ok(path)
    }
}

/// Canonical path without Windows' `\\?\` prefix, so git, the tree, and
/// `starts_with` all see the same string the user would.
pub fn canonical(path: &Path) -> io::Result<PathBuf> {
    Ok(without_verbatim(path.canonicalize()?))
}

fn without_verbatim(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            if let Some(unc) = rest.strip_prefix(r"UNC\") {
                return PathBuf::from(format!(r"\\{unc}"));
            }
            return PathBuf::from(rest.to_string());
        }
        path
    }
    #[cfg(not(windows))]
    {
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_resolves_inside_the_folder_without_a_verbatim_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = Workspace::open(tmp.path()).unwrap();
        assert!(ws.root.is_dir());
        assert!(!ws.root.to_string_lossy().starts_with(r"\\?\"));
        std::fs::write(tmp.path().join("a.txt"), "ok").unwrap();
        let resolved = ws.resolve(Path::new("a.txt")).unwrap();
        assert!(resolved.ends_with("a.txt"));
        assert!(resolved.starts_with(&ws.root));
    }

    #[test]
    fn resolve_rejects_a_path_outside_the_project() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = Workspace::open(tmp.path()).unwrap();
        let outside = tempfile::tempdir().unwrap();
        let file = outside.path().join("x.txt");
        std::fs::write(&file, "x").unwrap();
        assert!(ws.resolve(&file).is_err());
    }
}
