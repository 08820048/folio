# Folio on Windows

macOS is the shipping platform. Windows is in progress: the native GPUI window
already has Windows code paths (Explorer, Recycle Bin, Ctrl shortcuts, ConPTY),
but there is no signed installer yet. The website download is **Coming soon**.

## What already works in the source

- Config lives in `%APPDATA%\Folio` (`USERPROFILE` is the home the file
  pickers start from). Project roots drop the `\\?\` verbatim prefix so git
  and the tree see the same path.
- Context menus say File Explorer / Recycle Bin instead of Finder / Trash.
- `git` (status, clone) and Recycle Bin PowerShell start with
  `CREATE_NO_WINDOW`, so polls do not flash a console. Explorer / `start`
  stay visible.
- Reveal uses `explorer /select,`; trash uses the Recycle Bin via PowerShell;
  Open with Default App uses `cmd /C start`.
- The project terminal talks to ConPTY through `alacritty_terminal`.
- Run commands go through `cmd.exe /S /C` and quote the Windows way.

## Build (on a Windows machine)

Visual Studio Build Tools with the MSVC C++ workload, plus rustup:

```bat
rustup toolchain install 1.95.0
cargo run --locked
```

`rust-toolchain.toml` pins the same compiler as macOS. There is no
`scripts/package.sh` equivalent yet: a release `.exe` is `cargo build --locked
--release`, and Sparkle does not apply. Auto-update on Windows is a later
piece.

Creating directory junctions / file symlinks when duplicating a tree needs
Developer Mode or an elevated process; a failure there is an OS limit, not a
silent copy of the target.

## Still to do before a public build

- A real installer (signed `.exe` / `.msi`), not a raw cargo binary
- Title bar / window-chrome pass on Windows 11 DPI
- IME candidate window position
- End-to-end of trash, Explorer, the terminal, and Run
- An updater in place of Sparkle

Until those land, the site keeps the Windows button disabled.
