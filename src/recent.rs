use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentProject {
    pub path: PathBuf,
    pub last_opened: u64,
    #[serde(skip)]
    pub available: bool,
}

pub fn load(file: &Path) -> io::Result<Vec<RecentProject>> {
    match fs::read(file) {
        Ok(raw) => {
            let mut items: Vec<RecentProject> =
                serde_json::from_slice(&raw).map_err(io::Error::other)?;
            items.truncate(8);
            for item in &mut items {
                item.available = item.path.is_dir();
            }
            Ok(items)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(e),
    }
}

fn write(file: &Path, items: &[RecentProject]) -> io::Result<()> {
    let parent = file
        .parent()
        .ok_or_else(|| io::Error::other("Invalid config path"))?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(&serde_json::to_vec_pretty(items)?)?;
    temp.as_file().sync_all()?;
    temp.persist(file).map_err(|e| e.error)?;
    Ok(())
}

pub fn record(path: &Path, file: &Path) -> io::Result<Vec<RecentProject>> {
    let path = path.canonicalize()?;
    let mut items = load(file)?;
    items.retain(|x| x.path != path);
    items.insert(
        0,
        RecentProject {
            path,
            available: true,
            last_opened: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        },
    );
    items.truncate(8);
    write(file, &items)?;
    Ok(items)
}

pub fn remove(path: &Path, file: &Path) -> io::Result<Vec<RecentProject>> {
    let mut items = load(file)?;
    items.retain(|x| x.path != path);
    write(file, &items)?;
    Ok(items)
}
