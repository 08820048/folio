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
        let root = path.canonicalize()?;
        if !root.is_dir() {
            return Err(io::Error::other("项目路径不是文件夹"));
        }
        Ok(Self { root })
    }
    pub fn resolve(&self, path: &Path) -> io::Result<PathBuf> {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        }
        .canonicalize()?;
        if !path.starts_with(&self.root) {
            return Err(io::Error::other("路径不在当前项目中"));
        }
        Ok(path)
    }
}
