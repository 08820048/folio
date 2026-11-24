# Changelog

Notable changes to Folio. The detailed, dated implementation log is
`docs/开发进度.md`; this file is the shorter public-facing view.

## Unreleased

### Removed

- Run (`⌘R` / `⇧⌘R`, the gutter and title-bar Play, the Run tab). It was a
  task runner, which the product brief rules out; the `⌘J` terminal is the way
  to run a command. Typing no longer rescans the whole buffer for runnable
  lines on every keystroke.
- OCaml, Objective-C, Haskell and Git commit message highlighting. Those four
  grammars were a fifth of the binary, and binary size is first-launch time on
  macOS; the files now open as plain text.

### Changed

- Release builds link precompiled Metal shaders instead of compiling them from
  source when the first window opens (318–642 ms whenever the Metal cache
  misses). `cargo run` still compiles them at runtime, so building from source
  needs only the Command Line Tools.
- Rainbow brackets in files over 256 KB are recomputed once typing pauses
  rather than on every keystroke.
- Release binary 75.7 → 60.7 MiB; first launch of a new build 1320 → 1105 ms
  median. `scripts/package.sh` enforces a 64 MiB budget.

## 0.1.7 — 2026-10-27

### Added

- Streaming, cancellable project search with parallel file readers and a path
  filter.
- `.gitignore`-aware file tree and quick-open index.
- Review marks in the changes list and diff header, saved with the session.
- Markdown relative images and in-app document links.
- Image zoom and pan (25%–800%, fit, `⌘`-scroll, pinch).
- Read-only paged viewer for files over 32 MiB, and a hex view for binary or
  non-UTF-8 files.
- Terminal scrollback search (`⌘F`), with next/previous match and highlight.
- Complete multi-cursor: option-click adds a caret, arrows move every caret, and
  word/line deletes apply to all of them.
- Quick-open relevance ordering and per-project recent files.
- Reopen closed tab, tab keyboard navigation and a scrolling tab strip.
- Save-all (`⌥⌘S`) and code-size zoom (`⌘+` / `⌘-` / `⌘0`).
- macOS CI for formatting, tests and clippy.

### Security

- Pruned the website's unused `geist` and runtime `shadcn` dependency chains
  (Next.js, MCP SDK, undici and friends), updated Vite/source-map-js and
  rustls to their patched releases. `npm audit` is clean; see
  `docs/dependency-audit-2026-10-27.md`.

### Changed

- Dependency updates: Actions v7, `notify` 7, `sha2` 0.11, tree-sitter
  Haskell/OCaml/Erlang grammars, and a patch/minor group. `tree-sitter`
  itself stays pinned until the grammar ABI moves with it.
- `src/app.rs` split into `src/app/mod.rs` plus focused modules and `tests.rs`.
- Recovery snapshots compare per-buffer revisions instead of every dirty Rope.
- File-level watch events update the quick-open index without a full walk.
- `README`, the PRD and `docs/开发进度.md` brought back in line with the code.

## 0.1.6

The last tagged release. See `docs/开发进度.md` for the complete history and
`website/src/data/releases.ts` for the release notes shown on the site.
