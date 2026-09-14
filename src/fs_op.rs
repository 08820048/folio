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
        if fs::symlink_metadata(&candidate).is_ok() {
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
    let kind = fs::symlink_metadata(source)?.file_type();
    if (!kind.is_dir() && !kind.is_file()) || !fs::symlink_metadata(into)?.is_dir() {
        return Err(io::Error::other("Copy and move require regular files and folders"));
    }
    let name = source.file_name().ok_or_else(|| io::Error::other("Invalid path"))?;
    let destination = into.join(name);
    if destination == source {
        return Err(io::Error::other("Source and destination are the same"));
    }
    if source.is_dir() && into.starts_with(source) {
        return Err(io::Error::other("Cannot move a folder into itself"));
    }
    if fs::symlink_metadata(&destination).is_ok() {
        return Err(io::Error::other(format!(
            "{} already exists",
            name.to_string_lossy()
        )));
    }
    if cut {
        // A move across volumes cannot be a rename, so fall back to copy+delete.
        match rename_new(source, &destination) {
            Ok(()) => {}
            Err(error) if is_cross_device(&error) => {
                copy(source, &destination)?;
                remove(source)?;
            }
            Err(error) => return Err(error),
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
    rename_new(source, &destination)?;
    Ok(destination)
}

/// Atomically refuse an existing target, including symbolic links. A
/// case-only rename on an insensitive volume is the same file, so it is
/// allowed after the exclusive attempt reports that the destination exists.
fn rename_new(source: &Path, destination: &Path) -> io::Result<()> {
    match exclusive_rename(source, destination) {
        Ok(()) => Ok(()),
        Err(_) if same_identity(source, destination) => fs::rename(source, destination),
        Err(error) => Err(error),
    }
}

fn exclusive_rename(source: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        use rustix::fs::{renameat_with, CWD, RenameFlags};
        renameat_with(CWD, source, CWD, destination, RenameFlags::NOREPLACE).map_err(Into::into)
    }
    #[cfg(target_os = "windows")]
    {
        if fs::symlink_metadata(destination).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "destination exists",
            ));
        }
        fs::rename(source, destination)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (source, destination);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Atomic non-overwriting rename is unavailable",
        ))
    }
}

fn same_identity(source: &Path, destination: &Path) -> bool {
    let Ok(source) = fs::symlink_metadata(source) else {
        return false;
    };
    let Ok(destination) = fs::symlink_metadata(destination) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        source.dev() == destination.dev() && source.ino() == destination.ino()
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        source.volume_serial_number() == destination.volume_serial_number()
            && source.file_index() == destination.file_index()
            && source.volume_serial_number().is_some()
            && source.file_index().is_some()
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (source, destination);
        false
    }
}

fn is_cross_device(error: &io::Error) -> bool {
    if error.kind() == io::ErrorKind::CrossesDevices {
        return true;
    }
    #[cfg(unix)]
    {
        rustix::io::Errno::from_io_error(error) == Some(rustix::io::Errno::XDEV)
    }
    #[cfg(not(unix))]
    {
        false
    }
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
        #[cfg(unix)]
        let mut input = fs::File::from(rustix::fs::open(
            source, rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW,
            rustix::fs::Mode::empty(),
        )?);
        #[cfg(not(unix))]
        let mut input = fs::File::open(source)?;
        if !input.metadata()?.is_file() {
            return Err(io::Error::other("Only regular files can be copied"));
        }
        let mut output = fs::OpenOptions::new().write(true).create_new(true).open(destination)?;
        io::copy(&mut input, &mut output)?;
        output.set_permissions(meta.permissions())?;
        output.sync_all()
    } else if meta.file_type().is_symlink() {
        // Recreate the link itself. Following it would write wherever the
        // dangling or out-of-project target pointed.
        let target = fs::read_link(source)?;
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, destination)
        }
        #[cfg(not(unix))]
        {
            let _ = (target, destination);
            Err(io::Error::other(
                "Only regular files and folders can be copied",
            ))
        }
    } else {
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

#[cfg(all(test, unix))]
mod safety_tests {
    use super::*;
    use std::os::unix::fs::symlink;
    #[test]
    fn copy_and_rename_refuse_links_and_existing_targets() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let source = root.join("a.txt");
        let into = root.join("dest");
        let outside = root.join("outside");
        fs::write(&source, "source").unwrap();
        fs::create_dir(&into).unwrap();
        symlink(&outside, into.join("a.txt")).unwrap();
        assert!(paste(&source, &into, false).is_err());
        assert!(paste(&source, &into, true).is_err());
        assert!(!outside.exists());
        symlink(&outside, root.join("a copy.txt")).unwrap();
        assert_eq!(duplicate(&source).unwrap(), root.join("a copy 2.txt"));
        assert!(!outside.exists());
        let other = root.join("other.txt");
        fs::write(&other, "keep").unwrap();
        assert!(rename_new(&source, &other).is_err());
        assert_eq!(fs::read_to_string(other).unwrap(), "keep");
        // Covers distinct case variants on sensitive volumes; on insensitive
        // volumes the exclusive create must refuse the alias instead.
        let upper = root.join("A.txt");
        if let Ok(mut file) = fs::OpenOptions::new().write(true).create_new(true).open(&upper) {
            use std::io::Write;
            file.write_all(b"keep uppercase").unwrap();
            assert!(rename(&source, "A.txt").is_err());
            assert_eq!(fs::read_to_string(&upper).unwrap(), "keep uppercase");
        } else {
            assert_eq!(rename(&source, "A.txt").unwrap(), upper);
        }

        let case = root.join("case.txt");
        fs::write(&case, "same").unwrap();
        let renamed = rename(&case, "CASE.txt").unwrap();
        assert_eq!(fs::read_to_string(&renamed).unwrap(), "same");
        assert!(same_identity(&renamed, &root.join("CASE.txt")));

        let folder = root.join("tree");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("inside.txt"), "ok").unwrap();
        symlink(&outside, folder.join("link.txt")).unwrap();
        let copied = root.join("tree-copy");
        copy(&folder, &copied).unwrap();
        assert_eq!(fs::read_to_string(copied.join("inside.txt")).unwrap(), "ok");
        assert_eq!(fs::read_link(copied.join("link.txt")).unwrap(), outside);
        assert!(!outside.exists());
    }
}
