# Accessibility and cross-platform acceptance

This file separates what the repository can check from what needs a real
machine. The manual half has **not** been run yet, so the Ornata task
"无障碍与 Windows 实机验收" stays in progress.

## In the code and in CI

- Interactive elements carry an accessible role and a name: icon buttons,
  rail icons, tabs, tree rows, the changes list, quick-open rows, search rows,
  panel tabs, menu items, notices and the terminal search bar.
- Selection state is exposed (`aria_selected`) on tabs, tree rows, changes
  rows, quick-open rows and search rows; expanded state on project headers;
  the lookup panel is a dialog.
- Icon-only controls keep a keyboard focus target and answer Enter/Space;
  tooltips repeat the accessible name.
- The file tree, tabs, search panel, terminal, quick open, menus and dialogs
  all have keyboard paths; the tree and editor also keep the platform focus
  ring visible when reached by keyboard.
- The Windows job in `.github/workflows/ci.yml` runs
  `cargo check --locked --features desktop-tests` and the library tests on
  `windows-latest` for every push and pull request.
- `cargo fmt`, clippy with warnings denied and the desktop regression suite
  run on macOS in the same workflow.

## Manual: macOS

Not yet run. Check, in order:

1. **VoiceOver** — open a project, walk the title bar, rail, tree, tab strip,
   editor, quick-open panel, search panel, changes list, menus, settings and
   the large-file/terminal search bars. Every control announces a useful name
   and role; the selected tab, row and panel state are announced.
2. **Keyboard only** — open a project, open a file, save, close a tab, reopen
   it, split a pane, switch panes, search the project, replace, close the
   project, quit. No step should need the pointer.
3. **System appearance and contrast** — light and dark; check body text,
   muted text, selection, match highlight, diff additions/removals, git marks
   and focus rings against the effective background.
4. **Display scaling** — 100%, 125% and a Retina display; text and the
   terminal grid stay aligned, nothing is clipped.
5. **Reduced motion** — Settings → Type → Cursor animation off; notices and
   the sidebar are already instant.
6. **Input methods** — Chinese IME in the editor and the terminal: candidates,
   composition, Escape, undo after commit.
7. **Large file and image** — page a >32 MiB file, open a binary as hex, zoom
   an image and pan it; the page controls are reachable by keyboard.

## Manual: Windows

Not yet run on real hardware. `docs/windows.md` already lists the source
paths. Check:

1. Build with MSVC and launch `cargo run --locked`; no console window.
2. Title bar and window chrome at 100%, 125% and 150% DPI.
3. File picker, Explorer reveal, Recycle Bin, Delete Immediately, Open in
   Terminal, Open with Default App.
4. Git status, diff, blame and the GitHub clone flow with Windows paths and
   `\\?\` prefixes.
5. ConPTY terminal, IME, copy/paste, URL opening and Run (`cmd.exe /S /C`).
6. Session restore, atomic save and the unsaved-recovery file under
   `%APPDATA%\Folio`.
7. Keyboard-only pass, then Narrator.

Record the build, OS version and any failure here when it is run.
