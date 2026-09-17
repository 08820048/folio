# Folio on Windows

macOS still ships from a local machine (`./scripts/release.sh`). Windows
builds on GitHub Actions when a version tag is pushed, then an installer
`.exe` is uploaded to the same `folio-releases` R2 bucket. The website
download stays **Coming soon** until that installer is signed.

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
- A release `Folio.exe` is a GUI process (`windows_subsystem`), so launching
  it does not open a console.

## Build (on a Windows machine)

Visual Studio Build Tools with the MSVC C++ workload, plus rustup:

```bat
rustup toolchain install 1.95.0
cargo run --locked
```

`rust-toolchain.toml` pins the same compiler as macOS. A matching installer
needs Inno Setup 6 and ImageMagick:

```powershell
pwsh -File scripts/package-windows.ps1
```

Creating directory junctions / file symlinks when duplicating a tree needs
Developer Mode or an elevated process; a failure there is an OS limit, not a
silent copy of the target.

## Release (GitHub tag → R2)

Mac is unchanged: bump `Cargo.toml` and `packaging/release.env`, then
`./scripts/release.sh` on the Mac. After that commit is on `main`, tag the
same version (the `v` prefix is required, and it must equal `VERSION`):

```sh
git tag "v$(sed -n 's/^VERSION=//p' packaging/release.env | head -n 1)"
git push origin "v$(sed -n 's/^VERSION=//p' packaging/release.env | head -n 1)"
```

`.github/workflows/windows-release.yml` checks that the tag is `v$VERSION`,
builds with `--locked --release` on `windows-latest`, wraps `Folio.exe` in an
Inno Setup installer, and uploads:

```text
https://pub-307aebc4dfa4468d80e05e193c8f9c5b.r2.dev/Folio-<version>-<build>-windows-x64.exe
https://pub-307aebc4dfa4468d80e05e193c8f9c5b.r2.dev/Folio-latest-windows.exe
https://pub-307aebc4dfa4468d80e05e193c8f9c5b.r2.dev/windows-appcast.json
```

No GitHub Release is created. Sparkle does not apply. Installed copies
check `windows-appcast.json` quietly on launch (and daily after that); a
newer build lights a green **Update available** badge in the title bar,
left of the run control. Clicking it, or **Check for Updates…**, opens
the Install / Later prompt, which downloads the new installer and runs it
silently.

The workflow needs one repository secret:

```text
CLOUDFLARE_API_TOKEN    R2 write on the folio-releases bucket
```

`CLOUDFLARE_ACCOUNT_ID` is already in `packaging/release.env`. Re-running a
failed job rebuilds the commit the tag points at. After the workflow changes,
move the tag to the new commit (the first Windows build of `v0.1.2` never
published an artifact). Do not overwrite
`Folio-<version>-<build>-windows-x64.exe` after users have fetched it — bump
`BUILD` instead, the same rule as the Mac DMG.

## Still to do before a public download

- Authenticode signing of the installer
- Title bar / window-chrome pass on Windows 11 DPI
- IME candidate window position
- End-to-end of trash, Explorer, the terminal, and Run

Until those land, the site keeps the Windows button disabled.
