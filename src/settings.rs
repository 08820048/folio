//! User settings, stored as local JSON beside the recent-projects file.
//!
//! Nothing here leaves the machine, and nothing read out of a project is ever
//! written to it — the file holds preferences and an ignore list only.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    path::Path,
};

/// The ranges the settings surface clamps to. A hand-edited file should not be
/// able to ask for a type size that makes the window unreadable.
pub const FONT_SIZE: (f32, f32) = (10., 20.);
pub const CODE_FONT_SIZE: (f32, f32) = (10., 24.);
pub const TAB_SIZE: (usize, usize) = (1, 8);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Interface font. `None` keeps whatever the component theme picked.
    pub font_family: Option<String>,
    /// Code font. `None` uses the JetBrains Mono embedded in the binary.
    pub code_font_family: Option<String>,
    pub font_size: f32,
    pub code_font_size: f32,
    pub tab_size: usize,
    pub hard_tabs: bool,
    /// Whether the file tree starts open. The `⌘B` toggle writes this back.
    pub sidebar: bool,
    /// Whether the icon rail on the far left starts open. The title-bar
    /// toggle writes this back.
    pub activity_bar: bool,
    /// Folder names the tree never descends into and the index never walks.
    pub ignored: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            font_family: None,
            code_font_family: None,
            font_size: 13.,
            code_font_size: 14.,
            tab_size: 4,
            hard_tabs: false,
            sidebar: true,
            activity_bar: true,
            ignored: crate::tree::DEFAULT_IGNORED
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
        }
    }
}

impl Settings {
    /// Pull a hand-edited file back into range. Every other field is taken as
    /// written, so an unknown key in the file is simply ignored by serde.
    pub fn clamped(mut self) -> Self {
        self.font_size = self.font_size.clamp(FONT_SIZE.0, FONT_SIZE.1);
        self.code_font_size = self
            .code_font_size
            .clamp(CODE_FONT_SIZE.0, CODE_FONT_SIZE.1);
        self.tab_size = self.tab_size.clamp(TAB_SIZE.0, TAB_SIZE.1);
        self.font_family = self.font_family.filter(|name| !name.trim().is_empty());
        self.code_font_family = self.code_font_family.filter(|name| !name.trim().is_empty());
        // An empty ignore list is legitimate — it shows everything — but a
        // stray space would silently match nothing, so names are trimmed.
        self.ignored = self
            .ignored
            .iter()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect();
        self
    }
}

/// Read the settings. A missing file is a fresh install and not an error; a
/// corrupt one is reported rather than quietly replaced, so the user can see
/// what was wrong before it is overwritten.
pub fn load(file: &Path) -> io::Result<Settings> {
    match fs::read(file) {
        Ok(raw) => serde_json::from_slice::<Settings>(&raw)
            .map(Settings::clamped)
            .map_err(io::Error::other),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(e) => Err(e),
    }
}

/// Write beside the original and then replace it, so an interrupted write
/// cannot leave half a settings file behind.
pub fn save(file: &Path, settings: &Settings) -> io::Result<()> {
    let parent = file
        .parent()
        .ok_or_else(|| io::Error::other("Invalid config path"))?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(&serde_json::to_vec_pretty(settings)?)?;
    temp.as_file().sync_all()?;
    temp.persist(file).map_err(|e| e.error)?;
    Ok(())
}
