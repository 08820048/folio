//! Creating, copying and launching entries from the project tree.
//!
//! Everything here is blocking, so UI callers run it on the background
//! executor, the same rule `buffer`, `search` and `tree` follow.

use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

/// Finder's suffix for a copy, so a second duplicate never collides.
const COPY: &str = "copy";

/// Reject names that would escape the containing folder or address nothing.
pub fn validate_name(name: &str) -> io::Result<()> {
    if name.trim().is_empty() {
        return Err(io::Error::other("Name cannot be empty"));
    }
    if name == "." || name == ".." {
        return Err(io::Error::other("Invalid name"));
    }
    if name.contains(['/', '\\']) {
        return Err(io::Error::other("Name cannot contain a path separator"));
    }
    if name.contains('\0') {
        return Err(io::Error::other("Name contains an invalid character"));
    }
    Ok(())
}

/// Create an empty file. Fails rather than overwriting an existing one.
pub fn create_file(dir: &Path, name: &str) -> io::Result<PathBuf> {
    validate_name(name)?;
    let path = dir.join(name);
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    Ok(path)
}

/// Create a folder. Fails rather than reusing an existing one.
pub fn create_dir(dir: &Path, name: &str) -> io::Result<PathBuf> {
    validate_name(name)?;
    let path = dir.join(name);
    fs::create_dir(&path)?;
    Ok(path)
}

/// Copy an entry beside itself, named "`stem` copy", bumping the suffix until
/// the name is free.
pub fn duplicate(source: &Path) -> io::Result<PathBuf> {
    let parent = source
        .parent()
        .ok_or_else(|| io::Error::other("Invalid path"))?;
    let stem = source
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .ok_or_else(|| io::Error::other("Invalid path"))?;
    let extension = source.extension().map(|e| e.to_string_lossy().into_owned());
    for count in 0..1000 {
        let suffix = match count {
            0 => COPY.to_string(),
            // Finder numbers the second copy 2, not 1.
            count => format!("{COPY} {}", count + 1),
        };
        let name = match &extension {
            Some(extension) => format!("{stem} {suffix}.{extension}"),
            None => format!("{stem} {suffix}"),
        };
        let candidate = parent.join(name);
        if candidate.exists() {
            continue;
        }
        copy(source, &candidate)?;
        return Ok(candidate);
    }
    Err(io::Error::other("Could not find a free name for the copy"))
}

/// Copy or move `source` into the folder `into`. A cut only removes the source
/// once the copy has landed.
pub fn paste(source: &Path, into: &Path, cut: bool) -> io::Result<PathBuf> {
    let name = source
        .file_name()
        .ok_or_else(|| io::Error::other("Invalid path"))?;
    let destination = into.join(name);
    if destination == source {
        return Err(io::Error::other("Source and destination are the same"));
    }
    if source.is_dir() && into.starts_with(source) {
        return Err(io::Error::other("Cannot move a folder into itself"));
    }
    if destination.exists() {
        return Err(io::Error::other(format!(
            "{} already exists",
            name.to_string_lossy()
        )));
    }
    if cut {
        // A move across volumes cannot be a rename, so fall back to copy+delete.
        if fs::rename(source, &destination).is_err() {
            copy(source, &destination)?;
            remove(source)?;
        }
    } else {
        copy(source, &destination)?;
    }
    Ok(destination)
}

/// Rename an entry in place. Refuses a name that is already taken.
pub fn rename(source: &Path, name: &str) -> io::Result<PathBuf> {
    validate_name(name)?;
    let parent = source
        .parent()
        .ok_or_else(|| io::Error::other("Invalid path"))?;
    let destination = parent.join(name);
    if destination == source {
        return Ok(destination);
    }
    // A case-only rename asks for a name that already "exists" on a
    // case-insensitive volume, but it is the entry being renamed.
    let same_entry = source
        .file_name()
        .is_some_and(|current| current.to_string_lossy().to_lowercase() == name.to_lowercase());
    if destination.exists() && !same_entry {
        return Err(io::Error::other(format!("{name} already exists")));
    }
    fs::rename(source, &destination)?;
    Ok(destination)
}

/// Delete permanently. There is no undo — `trash` is the reversible one.
pub fn delete(path: &Path) -> io::Result<()> {
    remove(path)
}

/// Move an entry to the platform's trash, where the user can recover it.
pub fn trash(path: &Path) -> io::Result<()> {
    run(trash_command(path))
}

#[cfg(target_os = "macos")]
fn trash_command(path: &Path) -> Command {
    // Asking Finder to delete is the only dependency-free route to a real
    // Trash entry — one the user can "Put Back" — rather than a hand-rolled
    // move into ~/.Trash, which loses the metadata Finder keeps.
    let path = path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let mut command = Command::new("osascript");
    command.arg("-e").arg(format!(
        "tell application \"Finder\" to delete POSIX file \"{path}\""
    ));
    command
}

#[cfg(target_os = "windows")]
fn trash_command(path: &Path) -> Command {
    // The VisualBasic file API is the shortest route to the Recycle Bin, and
    // it needs a different call for a folder than for a file.
    let path = path.to_string_lossy().replace('\'', "''");
    let mut command = Command::new("powershell");
    command.args(["-NoProfile", "-Command"]).arg(format!(
        "Add-Type -AssemblyName Microsoft.VisualBasic; \
         if (Test-Path -PathType Container '{path}') {{ \
         [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory('{path}','OnlyErrorDialogs','SendToRecycleBin') }} \
         else {{ \
         [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile('{path}','OnlyErrorDialogs','SendToRecycleBin') }}"
    ));
    command
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn trash_command(path: &Path) -> Command {
    let mut command = Command::new("gio");
    command.arg("trash").arg(path);
    command
}

/// Recursively copy a file, or a whole folder tree.
fn copy(source: &Path, destination: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(source)?;
    if meta.is_dir() {
        fs::create_dir(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy(&entry.path(), &destination.join(entry.file_name()))?;
        }
        Ok(())
    } else if meta.is_file() {
        fs::copy(source, destination).map(|_| ())
    } else {
        // Symlinks are hidden from the tree, so they never arrive here in
        // practice; refuse rather than follow one out of the project.
        Err(io::Error::other(
            "Only regular files and folders can be copied",
        ))
    }
}

fn remove(path: &Path) -> io::Result<()> {
    if fs::symlink_metadata(path)?.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Show the entry in the platform's file manager.
pub fn reveal(path: &Path) -> io::Result<()> {
    run(reveal_command(path))
}

/// Hand the entry to the platform's default application.
pub fn open_default(path: &Path) -> io::Result<()> {
    run(open_command(path))
}

/// Open a terminal whose working directory is `dir`.
pub fn open_terminal(dir: &Path) -> io::Result<()> {
    run(terminal_command(dir))
}

#[cfg(target_os = "macos")]
fn reveal_command(path: &Path) -> Command {
    let mut command = Command::new("open");
    command.arg("-R").arg(path);
    command
}

#[cfg(target_os = "windows")]
fn reveal_command(path: &Path) -> Command {
    let mut command = Command::new("explorer");
    command.arg(format!("/select,{}", path.display()));
    command
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn reveal_command(path: &Path) -> Command {
    // No file manager offers "select this item" portably; open its folder.
    let mut command = Command::new("xdg-open");
    command.arg(path.parent().unwrap_or(path));
    command
}

#[cfg(target_os = "macos")]
fn open_command(path: &Path) -> Command {
    let mut command = Command::new("open");
    command.arg(path);
    command
}

#[cfg(target_os = "windows")]
fn open_command(path: &Path) -> Command {
    // `start` needs its first argument to be the window title, not the path.
    let mut command = Command::new("cmd");
    command.args(["/C", "start", ""]).arg(path);
    command
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn open_command(path: &Path) -> Command {
    let mut command = Command::new("xdg-open");
    command.arg(path);
    command
}

#[cfg(target_os = "macos")]
fn terminal_command(dir: &Path) -> Command {
    let mut command = Command::new("open");
    command.arg("-a").arg("Terminal").arg(dir);
    command
}

#[cfg(target_os = "windows")]
fn terminal_command(dir: &Path) -> Command {
    let mut command = Command::new("cmd");
    command.current_dir(dir).args(["/C", "start", "cmd"]);
    command
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn terminal_command(dir: &Path) -> Command {
    let mut command = Command::new("x-terminal-emulator");
    command.current_dir(dir);
    command
}

fn run(mut command: Command) -> io::Result<()> {
    let status = command.status()?;
    // Windows' `explorer` exits non-zero even when it opened the window, so
    // only the spawn result is trustworthy there.
    if cfg!(target_os = "windows") || status.success() {
        Ok(())
    } else {
        Err(io::Error::other("External command failed"))
    }
}
