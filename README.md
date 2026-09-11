# Folio

A local code reading editor built with Rust and GPUI. Light and dark follow the
system; one window, several projects; no WebView, no terminal, no AI service.

## Running

On macOS, after installing the Xcode Command Line Tools:

```sh
cargo run --locked
```

`rust-toolchain.toml` pins Rust 1.95.0 and rustup installs it on demand. The
first build downloads dependencies; the app itself never needs the network. The
code font, JetBrains Mono, is embedded in the binary — its license is
`assets/OFL.txt`. Interface icons come from the Lucide set bundled with
gpui-component, except two that the component library does not ship
(`panel-left-dashed` / `panel-right-dashed`), which are vendored in
`assets/icons/` under the `LICENSE.txt` in that directory.

To build a local `.app`:

```sh
./scripts/package.sh
open target/Folio.app
```

Pass `--debug` for a development build. The packaging script signs ad-hoc for
this machine only; distribution signing and notarization are not set up.

## Usage

Once a project is open, pick a file in the tree or press `⌘P` to fuzzy-match
file names. Type `:123` to go to a line. Switching files keeps each one's
cursor, contents and undo history; closing a project releases that project's
buffers. Click "Add Project" in the sidebar to bring in more folders — each
keeps its own buffers and expansion state — though the list of open projects
does not survive a restart.

| Action | macOS | Windows / Linux |
| --- | --- | --- |
| Open project | ⌘O | Ctrl+O |
| Quick open | ⌘P | Ctrl+P |
| Find in project | ⇧⌘F | Ctrl+Shift+F |
| Replace in project | ⇧⌘H | Ctrl+Shift+H |
| Save current file | ⌘S | Ctrl+S |
| Find in file | ⌘F | Ctrl+F |
| Go to line | ⌘G | Ctrl+G |
| Toggle sidebar | ⌘B | Ctrl+B |
| Settings | ⌘, | Ctrl+, |
| Close project | ⌘W | Ctrl+W |
| Quit | ⌘Q | Ctrl+Q |

The title bar and content area follow the system appearance. The sidebar
toggle, the project name and the file's relative path sit in that order to the
right of the macOS traffic lights; the old centred title and second-line path
bar are gone. The sidebar is drag-resizable. Its top row shows the project
folder's name and collapses or expands the whole tree on click, keeping
subdirectory state, and it also answers to Enter, Space and the left and right
arrows. The file tree takes the arrow keys and Enter. Editing uses
gpui-component's Rope editor and a Tree-sitter allowlist; the defaults are
14px type, 1.6 line height, a 4-space indent and no soft wrap.

Images preview in place, scaled to fit: PNG, JPEG, GIF, WebP, BMP, TIFF, ICO
and SVG. GIF and WebP show their first frame, and an image never enters the
text editing or saving path. Every in-app icon is Lucide and icon buttons draw
no background box. The sidebar toggle in the title bar's top-left uses
`panel-left-dashed` / `panel-right-dashed`; gpui-component does not ship those
two, so `src/assets.rs` wraps the bundled asset source and serves them from
`assets/icons/` — the other 99 still come from the component library, both
under the same `icons/<name>.svg` namespace.

Saving first checks that the file on disk still matches what was read, then
writes a temporary file alongside it and replaces it atomically. A failure
leaves the buffer dirty rather than overwriting an external change. Recent
projects live in a local JSON file capped at eight entries; removing one does
not touch the project's files. On macOS the data directory is
`~/Library/Application Support/Folio/`.

## Project tree

Right-click a folder row, or a project header, and the menu opens at the
pointer, flipping when it would overhang the window. It carries New File, New
Folder, Reveal in Finder, Open in Default App, Open in Terminal, Find in
Folder…, Cut, Copy, Duplicate, Paste, Rename, Move to Trash and Delete
Immediately, in that order, in six groups.

The project root's menu drops the last three. The root is the workspace's
identity — `project_order`, the recent list and every cached path key off it —
so renaming or removing it would cascade through all of them for little gain.
Renaming or deleting a folder *inside* the project is ordinary.

The shortcuts the menu prints are real bindings while it is open, claimed in
the root's key-capture phase so a bare `⇧R` or `⌘⌫` never reaches the editor
underneath. They are not global: `⌘C` opens the menu's Copy only while a menu
is up and stays the editor's Copy otherwise.

New File and New Folder splice a text field into the tree in front of the
folder's children; Rename reuses the entry's own row and preselects the name.
Enter is what touches the filesystem — Escape and clicking away create nothing.
The field is a text input rather than a hand-rolled buffer, so IME composition
works for non-ASCII names.

Rename re-keys the open buffers, the active file, the image preview, the
expansion state and the cached directory listings, so a renamed folder keeps
its expanded subtree and a file that is open stays open under its new path.

Cut, Copy and Paste use an in-app clipboard holding one path, so they move and
copy entries inside the project rather than going through the system
clipboard. A cut is spent once it lands; a copy can be pasted again. Duplicate
copies beside the original as `foo copy.rs`, then `foo copy 2.rs`.

Find in Folder… opens the search panel scoped to that subtree. The panel shows
the scope as a chip that can be cleared, and opening it from the keyboard
resets the scope rather than inheriting the last folder.

Nothing here overwrites. Creating uses an exclusive open, and renaming or
pasting onto a name that already exists is refused with a message. Pasting a
folder into its own subtree is refused too, and a cut that cannot be a rename —
across volumes — falls back to copy-then-delete.

Move to Trash hands the path to Finder, which is the only route to a real Trash
entry the user can put back without adding a dependency. Delete Immediately is
permanent and always confirms first. Trash is recoverable and only stops to
confirm when unsaved edits sit under the entry, which it would otherwise drop
silently. Both release the buffers, previews and cached directories that
pointed at the entry. Reveal, Open in Default App and Open in Terminal hand the
path to the OS, so what they launch follows the platform.

## Settings

`⌘,`, or Settings… in the app menu, opens a window of its own rather than a
panel over the editor, so it can be moved, resized and left open beside the
code. Its title bar is drawn by the app, the same way the main window's is:
handing it to AppKit means it is painted from a system material that samples
whatever is behind the window, which lands nowhere near the theme in either
appearance. The traffic lights are still the real ones.

The sidebar is a tree. Four headings collapse, and the five pages under them
are the screens: interface font and size, code font and size, tab size and
indent character, whether the sidebar starts open, and the folder names the
tree and search skip. A heading is not a page itself, so clicking one toggles
it and clicking a page selects it. Each row is the setting's name and what it
does stacked on the left and its control flush right, with a hairline
underneath. Numbers step, two-way choices toggle, and both live in one bordered
box with its actions split by dividers.

Changes apply as you make them. Fonts and sizes take effect at once;
tab size applies to files opened from then on, because the pinned
gpui-component has no way to change it on an editor that already exists. The
ignore list is the one that does real work: the tree re-reads the folders it is
holding and the quick-open index is rebuilt.

Settings are local JSON beside the recent list, written atomically. A
hand-edited file is pulled back into range rather than rejected — sizes are
clamped, and names are trimmed — a corrupt one is reported rather than
replaced, and keys an older version did not write keep their defaults.

The CJK fallback the type section calls for is set here rather than left to the
platform. GPUI's own fallback stack names `.ZedMono`, `Helvetica`, `Segoe UI`
and friends, and no CJK family at all, so before this a Chinese character in a
file was drawn in whatever the platform picked — usually a proportional face,
which breaks the character grid. The app now sets its own chain, the mono CJK
families first.

The window remembers where it was. That geometry lives in `window.json`
alongside the main window's, one key each; a file written by an older version,
holding a bare rectangle, still loads as the main window.

## Project-wide search and replace

The `⌘P` panel's "Files" and "Contents" tabs switch between the two lookups in
one place. `⇧⌘F` opens the content side directly and `⇧⌘H` also reveals the
replace row. Because both lookups share one panel, the title bar keeps a single
"Search" button as the entry point rather than a separate quick-open button;
`⌘P` and the menu items still open it on the file-name side.

Search is live: 140ms after typing stops, a background scan of the whole
project starts. Results are grouped per file with line numbers and a context
preview, and matches are drawn in the accent colour. Long lines are windowed
around the match and marked with an ellipsis, leading indentation is dropped
and tabs expand to four spaces, so the previews line up. `↑ ↓` chooses, `Enter`
opens the file with the cursor on the match, and `Esc` closes.

Three filters, following editor convention, apply to both find and replace:

- **Aa** — match case, off by default
- **ab** — whole word: neither side may be a letter, digit or underscore.
  Decided by inspecting the characters around a match, because the `regex`
  crate has no look-around and `\b` binds to the wrong side when the query
  starts or ends with a non-word character
- `.*` — regular expression; `$1` / `${name}` / `$$` expand in the replacement
  per the `regex` crate's rules. With it off the query is escaped and the
  replacement is inserted verbatim

"Replace All" confirms first, and **splits by whether the file is open**:

- **Files that are open** are edited in memory. Undo history survives, the
  buffer goes dirty and waits for `⌘S`; nothing is written to disk behind you
- **Files that are not open** are read, replaced and written back atomically on
  the background executor. Writing goes through `buffer::save`, which first
  confirms the file on disk still equals what was just read, so an external
  edit inside the read-to-write window is not overwritten. An edit made before
  the read is simply read in and the replacement applies to the newer content —
  deliberately, because a project-wide replace should not fail wholesale over
  an unrelated edit elsewhere

Open files are matched against **what is in memory**, not what is on disk;
otherwise an unsaved edit shifts every reported line and column. Files above
2 MiB, binary files and non-UTF-8 files are skipped silently, and the result
set is capped at 1000 matches over 200 files — the status line says "results
truncated" when it hits that ceiling.

## Syntax highlighting

Highlighting comes from Tree-sitter. gpui-component ships 37 grammars;
`src/syntax.rs` registers 23 more through the public `LanguageRegistry`,
without patching or forking the component library:

| Source | Languages |
| --- | --- |
| Bundled with the component | Rust, C, C++, C#, Go, Zig, Swift, Java, Kotlin, Scala, Python, Ruby, PHP, Lua, Bash, Elixir, TypeScript, TSX, JavaScript, HTML, CSS, Astro, Svelte, EJS, ERB, JSON, TOML, YAML, CMake, Make, Markdown, SQL, GraphQL, Protocol Buffers, Diff, JsDoc |
| Added here | XML, DTD, INI, Dockerfile, Nix, PowerShell, Fish, Vim script, Git commit messages, regex, Vue, Dart, R, assembly, Solidity, Haskell, OCaml, Erlang, Elm, Gleam, SCSS, Less |

The extension-to-language map is `src/buffer.rs::language`. Whole-name matches
win over extensions and cover the extensionless or easily misread files:
`Dockerfile` / `Dockerfile.dev`, `Containerfile`, `Makefile`, `CMakeLists.txt`,
`Gemfile` / `Rakefile` / `Vagrantfile`, the `.zshrc` family, `.editorconfig`,
`.vimrc`, `.Rprofile`, `Cargo.lock` / `Pipfile`, `COMMIT_EDITMSG`, `.env` /
`.env.local` and others. Uppercase `.C` / `.H` stay C++ by convention. An
unknown extension returns `text`: no highlighting, no error.

The bar a candidate has to clear is written up in `src/syntax.rs`'s module
comment. It must share the `tree-sitter-language` 0.1 ABI with the tree-sitter
0.26 that gpui-component pins, export its highlight query as a constant, and
have a build script that actually enables the cfg gating that constant.
`tree-sitter-dockerfile`, `tree-sitter-vue`, `tree-sitter-wgsl` and
`tree-sitter-cue` fail the first; `tree-sitter-glsl`, `tree-sitter-nickel`,
`tree-sitter-groovy`, `tree-sitter-perl` and `tree-sitter-vue-next` fail the
others. All were excluded.

The grammar tables are static data compiled into the binary and parsed only
when a matching file is opened, so startup, memory and scrolling are
unaffected. The cost is on disk: the release binary grows from about 21 MB to
about 73 MB. The four most expensive are OCaml at 13.8 MB, Objective-C at
11.4 MB, Haskell at 8.1 MB and Git commit messages at 4.2 MB; every other one
is under 3.4 MB. To drop one, remove its entry from `src/syntax.rs` and its
dependency from `Cargo.toml`. The `syntax` feature can be turned off as a
whole, which falls every extension back to plain text.

## Checks

```sh
cargo fmt --check
cargo test --locked --no-default-features
cargo test --locked --features desktop-tests
cargo clippy --locked --all-targets --features desktop-tests -- -D warnings
cargo build --locked
```

The tests cover non-ASCII paths, the recent-projects JSON, ignored directories,
paths escaping the project, UTF-8 and binary validation, atomic saves,
external-change conflicts, permission preservation, Git untracked and renamed
status, the extension-to-grammar map, the file operations behind the tree menu
(create collisions, a recursive duplicate and rename, paste refusals, a
case-only rename, deleting a tree), the settings file against real bytes
(defaults, round trip, clamping, unknown keys, a corrupt file left alone), the
ignore list reaching both the tree and the index, and project search (case,
whole word, regex, long-line windowing, CRLF, multi-byte columns, capture-group
replacement, skipping binary and oversized files). `desktop-tests` uses the
GPUI test executor for the rest: repeated expand and collapse, recents written
in order, stale callbacks across projects, picker exclusivity, dirty buffers
kept across projects and the save conflict on quit, image decoding and
switching between an image and a dirty text buffer, edits made during a save,
project search reading unsaved content, landing the cursor on a hit, and the
open-buffer-in-memory versus closed-file-on-disk split in replace, the tree
menu driving create, rename, cut, paste, duplicate, delete and Find in Folder
— including the entries the root menu omits, both windows' geometry being
recorded where the app was told to write it, and the settings form's every page
with its headings open and shut. What no test covers is the Trash call itself:
it would move real files and raise an automation prompt. The guard that stops
to confirm when unsaved edits are under the entry, and that cancelling leaves
the entry alone, is covered. Nor is `⌘,` opening the settings window: a test
window has no platform window behind it, so the form is driven directly
instead. None of this stands in for native IME or rendering acceptance.

Every registered grammar has its highlight query compiled and asserted not to
fall back to plain text. `SyntaxHighlighter::new` degrades silently on a bad
query, so only an assertion catches a misconfigured grammar.

## Pinned dependencies

- Upstream GPUI `0.2.2` + `gpui_platform 0.1.0`, **source commit**
  `cc053a4a6fa2fd0e8793201ed9099466af1be0b1`.
- gpui-component `0.5.2`, commit
  `f3ba893bd6a996ab0699266ba774b5bbb7f0ca1c`, and its assets at the same commit.
- The GPUI crates come from one Git source and are pinned together by
  `Cargo.lock`, so the component library and the app can never end up with two
  incompatible GPUIs. Always build with `--locked`. A blanket `cargo update`
  would drift those Git sources; a targeted `cargo update -p <crate> --precise
  <version>` is how one transitive dependency gets moved, and is what moved
  `quinn-proto` past the advisory that Dependabot raised against it. That crate
  is not compiled into Folio — it sits in the lockfile because `zed-reqwest`
  can use it for HTTP/3 and `gpui-component-assets` depends on `zed-reqwest`
  unconditionally — but a lockfile entry is what the alert reads.
- The extra grammars in `src/syntax.rs` come from crates.io, are pinned in
  `Cargo.lock`, and are gated behind the `syntax` feature (which `desktop`
  enables). Only the binary uses them, so a library-only build such as
  `cargo test --locked` does not compile them.

Only the GPUI framework and the components actually used are compiled; none of
Zed's editor, language or workspace application modules are involved. Cargo's
Git source download still fetches a checkout of the upstream repository.

## Current limits

This is a runnable development version. First-release performance and
cross-platform acceptance are not finished. A single file above 5 MiB turns
highlighting off and one above 32 MiB is refused; text must be UTF-8 and
symlinks are skipped. Quick open indexes at most 100,000 files and shows 100
matches. Project search skips files above 2 MiB and caps results at 1000
matches over 200 files, with no streaming results and no cancel button. Bitmaps
are capped at 32M pixels and 16,384 pixels per side, and SVG rasterization is
size-limited by GPUI. Git status refreshes when a project is opened or switched
and after a save. Move to Trash goes through Finder, so macOS raises an
automation permission prompt the first time and a refusal surfaces as an error;
the Recycle Bin and `gio trash` paths behind the other platforms are written
but untested on real hardware. Window geometry is written when a window closes
and again on quit, so a force-killed process loses wherever the windows were —
the same is true of the settings window. The settings sidebar has no search
box, and its pages do not scroll, which is fine at five pages and would need
fixing before there were many more.

Full progress and the outstanding acceptance items are in
[docs/开发进度.md](docs/开发进度.md); the original requirements are in
[docs/Folio需求文档.md](docs/Folio需求文档.md). Both are in Chinese.
