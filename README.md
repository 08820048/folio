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
| Replace in file | ⌥⌘F | Ctrl+Alt+F |
| Go to line | ⌘G | Ctrl+G |
| Toggle sidebar | ⌘B | Ctrl+B |
| Toggle comment | ⌘/ | Ctrl+/ |
| Fold | ⌥⌘[ | Ctrl+Alt+[ |
| Unfold | ⌥⌘] | Ctrl+Alt+] |
| Select next occurrence | ⌘D | Ctrl+D |
| Select all occurrences | ⇧⌘L | Ctrl+Shift+L |
| Add cursor above | ⌥⌘↑ | Ctrl+Alt+Up |
| Add cursor below | ⌥⌘↓ | Ctrl+Alt+Down |
| Select column up | ⇧⌥↑ | Ctrl+Shift+Alt+Up |
| Select column down | ⇧⌥↓ | Ctrl+Shift+Alt+Down |
| Show file changes | ⇧⌘D | Ctrl+Shift+D |
| Blame | ⌥⌘B | Ctrl+Alt+B |
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
gpui-component's Rope editor and a Tree-sitter allowlist; type sizes, font and
indentation come from the settings, and soft wrap is off.

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

## Sessions

What was open is put back when the app starts: every project that was open, in
the order it was opened, with the files each had showing in the order the strip
had them, and the one that was being read left on screen. A project that is no
longer a folder, or a file that is no longer there, is dropped as the session is
read rather than failing it — a session written yesterday may name a directory
that was deleted today. Closing every project still lands on the launcher.

Unsaved edits are the other half of it. Every few seconds, while there are any,
they are written to a file beside the session, so a crash, a force quit or a
power cut costs seconds rather than the work. A clean exit takes that file with
it, which is what leaves it behind only when there was no clean exit. The next
launch opens those buffers with their edits in them, marks them unsaved, and
says how many it recovered. They are unsaved in the ordinary way: `⌘S` writes
them, and the usual conflict check still protects whatever changed on disk in
the meantime.

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

## Tabs

Every file opened gets a tab, in a strip above the code. The strip is 26px and
appears only once a second file is open, so a single file looks exactly as it
did before tabs existed. A tab carries the file's name, a mark when its buffer
has edits that are not on disk, and a close button. Opening a file that already
has a tab focuses it rather than adding another.

Closing a tab with unsaved changes asks first — Save, Don't Save or Cancel —
and Save writes only that file, not every dirty buffer open alongside it. The
buffer is released when the tab goes, and the tab that slid into its place
takes over unless the closed one was in the background.

Tabs can be dragged. A tab dropped on another lands in that tab's place and
everything between shifts back towards where it came from, which reaches every
position including the last and moves a tab one slot in either direction. A
caret between the tabs shows where it will land. Pinned tabs stay at the front:
a drop can neither strand one among the unpinned ones nor put an unpinned one
ahead of them. Dropping a tab on itself changes nothing, which is how a drag is
abandoned.

The strip has its own right-click menu, acting on the tab that was clicked
rather than on the one showing:

- **Close**, **Close Others**, **Close Left**, **Close Right**, **Close Clean**
  (the tabs with nothing unsaved) and **Close All**. Every one of them leaves
  pinned tabs alone.
- **Make Tab Read-Only** locks the buffer against edits. It is view state and
  never touches the file's permissions, and the entry turns into "Make Tab
  Writable" once it is on.
- **Copy Path** and **Copy Relative Path** put one of them on the system
  clipboard.
- **Reveal in Finder** and **Open in Terminal**, the terminal opening at the
  folder holding the file.
- **Pin Tab**, which moves it to the front and takes it out of the bulk closes'
  reach.
- **Reveal In Project Panel**, which opens the folders above the file and puts
  the tree's cursor on its row.

An entry that would do nothing is disabled — Close Left on the first tab, Close
Others when everything else is pinned.

## Editing

Four things beyond plain typing. Three are bound to the code editor's own key
context, so they never reach the search box or a settings field; the fourth is
Enter, which the editor already had.

**Brackets pair.** `(`, `[`, `{`, `"`, `'` and `` ` `` insert both characters
and leave the caret between them, as one edit and so one step on the undo
stack. With a selection they wrap it instead. A closing bracket typed where one
already is steps over it rather than adding a second.

Neither happens where it would do harm, which is the part worth spelling out
because both rules come from Zed's editor. A closer is only inserted in front
of whitespace or another closer — anywhere else it would swallow the word that
is already there, turning `foo` into `()foo` where `(foo` was meant. And a
quote after a word is not opening a quote: it is an apostrophe or a lifetime,
so it is typed as itself and `don't` stays `don't`.

**Enter opens a block.** Pressed between a pair of brackets with nothing
between them — which is what typing `{`, `[` or `(` leaves behind — it lays the
pair out over three lines: the caret on the line between them, indented one
level further in, and the closer moved to the line after, at the indentation
the opener's line has. Typing a function is `{`, Enter, the body — the `}` is
already where it belongs. Everywhere else Enter breaks the line and indents
to the line it broke, as it always did.

Quotes are not brackets and do not do this: a line break inside a pair of them
is a string being written over two lines, not a block with a body in it.

**Blocks fold.** The gutter's chevron folds the block on that line away, and
`⌥⌘[` folds the block the caret is in — the innermost one, so pressing it again
takes the block around it. `⌥⌘]` unfolds the innermost folded block the caret is
in or on. Chevrons are drawn on the line the caret is on, on whatever line the
pointer is over the gutter for, and on the lines that are folded: a chevron on
every foldable line would be a column of noise down the file.

What a fold hides is not laid out at all, so a caret inside it would be on a
line nothing draws. It goes to the end of the line that stays visible.

The editor's own in-file replace answers to `⌥⌘F` rather than the `⇧⌘F` its
component binds it to. `⇧⌘F` is the project search, and a binding on the
focused element beats one further out — the editor sits inside the app — so
the search claims the key in the capture phase, which runs before bindings are
resolved at all.

**`⌘/` comments lines.** It works on every line the selection touches, so a
caret comments one line and a selection comments the lines it covers. The
marker follows the language: `//`, `#`, `--`, `%` or `;`. It goes after the
indentation rather than at the start of the line, blank lines do not block the
toggle, and a selection that is only partly commented gets commented rather
than uncommented. The block is left selected, so the same shortcut undoes it.
Languages whose comments are only a block form — HTML, XML, CSS, Markdown,
JSON — say so instead of inserting a marker the file cannot use.

## Multi-cursor

Four keys make more than one cursor and one takes it back down, all of them
bound to the code editor's own key context.

**`⌘D` takes the word, then the next place it appears.** With the caret inside
a word the first press selects that word; each press after it adds the next
occurrence of it, and a press with nothing left to add does nothing. `⇧⌘L`
takes every occurrence at once, and the selection the caret was already in
stays the one the caret is in, so the view does not jump to the last match in
the file.

**`⌥⌘↑` and `⌥⌘↓` add a caret on the line above or below**, in the column the
outermost caret was in, so holding either key grows a block of cursors a line
at a time. **Escape** goes back to one cursor — the one the caret was in — and
with one cursor it does whatever it did before.

Everything the editor does to one selection, it then does to all of them:
typing, pasting, `⌫` and `⌦`, and Enter, which breaks the line at every caret
with the indentation of the line it breaks — and lays out the pair, the way it
does for one cursor, wherever a caret sits inside a pair of brackets. Copying
joins the selections with newlines, so pasting them somewhere else gives back
one line each. However many selections the edit covered, it is one edit: one
step on the undo stack, and one change to the file.

**A rectangle puts a selection on every line it covers.** Drag with `⌥` held, or
press `⇧⌥↑` / `⇧⌥↓` at a caret to grow a column of them a line at a time.
Typing then lands on every line of it, as one edit, like any other set. A line
too short for the rectangle gives up at its own end rather than reaching into
the next one, and any movement or click ends the rectangle and leaves the
ordinary set behind.

The columns are counted in characters rather than in pixels: on lines holding
tabs or wide characters, it is a column of text rather than a drawn rectangle.

Three things do not multiply, and each says so rather than half-working. A
bracket is typed at every cursor instead of pairing around a selection, because
with several there is no single selection to wrap. `⌘/` comments the block from
the first cursor to the last as one edit, and leaves it selected the way it
does for one cursor. And a click, an arrow key or any other movement collapses
the set to one cursor — deliberately, because it means there is never a caret
somewhere the cursor commands do not know about.

`⇧⌘L` and `⌘D` stop at 1,000 selections: on a common word in a large file they
take the first thousand and go no further.

## File changes

`⇧⌘D`, or Show File Changes on the editor's right-click, replaces the editor
with the active file's changes against HEAD: both line numbers in a gutter, a
marker, the line itself, tinted green or red, with hunk headers on their own
rows and a count of what changed. The view follows the file, so a review can
walk the tree rather than reopening it per file.

A file git has never seen is shown as all new, which is the one case the view
reads the file itself. Outside a repository, or with git not installed, there
is nothing to show. Escape, `⌘⇧D` again, the close button in the header, or
Hide File Changes on the view's own right-click all put the editor back.

`⌥⌘B`, or Blame in the View menu, shows what the line the caret is on was last
written by — short hash, author, when, and the commit's summary — in a strip
under the code. It follows the caret, so reading down a function tells you who
wrote each part of it. A line that is not committed yet says so.

It is one line rather than a column in the gutter, which keeps every line of
code its full width, and blame is read against the *buffer* rather than the file
on disk: `git blame --contents` blames the text the editor is showing, so an
edit does not throw every line below it onto the wrong commit. What is read is
read when the strip is turned on, when a file is opened, and after a save;
editing in between leaves it describing the lines as they were until one of
those happens.

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

A project's own `.editorconfig` has the last word on the files it covers.
`indent_style`, `indent_size` and `tab_width` are read from the nearest such
file above the file being opened, up to one that says `root = true`, with the
settings standing in for whatever it does not mention — so a repository that
asks for two spaces gets two spaces, and everything else keeps what the window
says. The other properties the format defines — `end_of_line`,
`trim_trailing_whitespace`, `insert_final_newline` — describe edits to make when
saving. This editor does not make them, and says so rather than making some of
them.

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
`src/syntax.rs` registers 23 more through the public `LanguageRegistry`:

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
ignore list reaching both the tree and the index, the diff parser on
hand-written hunks and against a real repository, the line-comment markers and
the two rules that decide where a bracket may pair, the `.editorconfig` reader
— its globs, its sections applied in order, `unset` removing a property, `root`
ending the search and the nearest file winning — the session files against real
bytes (a round trip, paths that are gone being dropped, a corrupt file reading
as an empty session), blame against a real repository — including a buffer with
a line that was never committed — and project search (case,
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
recorded where the app was told to write it, the settings form's every page
with its headings open and shut, tabs opening, closing and guarding unsaved
changes, the tab menu's bulk closes and what pinning keeps out of them, drag
reordering in both directions, brackets pairing and stepping over, Enter
laying out a pair of brackets over three lines, folding the innermost block at
the caret and where the caret goes when the line under it stops being drawn,
a project's `.editorconfig` deciding what indentation an opened file gets,
a session put back with its projects, tab order, active file and recovered
buffer, and the writing down of what is open and unsaved,
comment toggling, the
multi-cursor commands with the edit that lands at every selection they make,
a rectangle becoming a selection on every line it covers and stopping at the
end of a short one,
and the changes view following the active file. What no test covers is the
Trash call itself: it would move real files and raise an automation prompt. The
guard that stops to confirm when unsaved edits are under the entry, and that
cancelling leaves the entry alone, is covered. Nor is `⌘,` opening the settings
window: a test window has no platform window behind it, so the form is driven
directly instead. None of this stands in for native IME or rendering
acceptance.

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

The component library may be forked. A feature that needs something its public
API does not reach — an editor with more than one cursor, say, which the pinned
input has no model for — is allowed to vendor the crate it lives in and point
at it with a `[patch]`, rather than being dropped. What keeps that reviewable
is the pin: the fork is a diff against one known revision, so an upgrade is a
rebase against a named commit rather than a merge against a moving target.
`gpui-base` is that fork today, vendored in `vendor/gpui-base` and pointed at
by a `[patch]` in the root manifest. It is the crate holding the editor's
input — its text, its selections, the element that draws them — which is the
one part of the application that has to change to hold more than one cursor.
`gpui-component` itself still comes from the pinned revision, so exactly one
package in the graph is local, and `vendor/README.md` has the procedure for
moving the copy to a new revision.

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
but untested on real hardware. Multi-cursor covers typing, pasting, deleting,
Enter and the four cursor commands, and stops there: there is no `⌥`-click to
add a cursor, an arrow key collapses the set rather than moving it, and the
word- and line-delete commands (`⌥⌫`, `⌥⌦`, `⌘⌫`, `⌘⌦`) were left as they
were, so they can act on one cursor rather than all of them. An edit through
the set is one step on the undo stack and comes back in one, though the cursors
it covered are not restored. Enter indents to the line it breaks, but not when
the line merely ends with an opener: a `{` whose pair is not there — deleted,
or never inserted because the character in front of it was a word — gets the
break and no extra level. Folding is not remembered across a restart, and there
is no fold-all: the two keys fold and unfold one block. A brace group in an
`.editorconfig` pattern holding a range rather than a list, `{1..3}`, is the
one part of that format not read. The session is written every few seconds, so a
crash can lose the last few seconds of tab changes, and a recovered buffer whose
project is no longer in the session is dropped rather than shown. Blame is one
line at a time rather than a gutter, and describes the lines as they were when
it was read. The changes view is a unified diff — there is
no side-by-side — and its counts count rows, so a changed line reads as one
gone and one arrived. There is no updater: the packaging this project has signs
ad-hoc for one machine, and there is no feed to check — so Check for Updates in
the app menu opens the page that lists the versions, which is the honest half of
it. Window geometry is written when a window closes and again
on quit, so a force-killed process loses wherever the windows were — the same
is true of the settings window. The settings sidebar has no search box, and its
pages do not scroll, which is fine at five pages and would need fixing before
there were many more.

Full progress and the outstanding acceptance items are in
[docs/开发进度.md](docs/开发进度.md); the original requirements are in
[docs/Folio需求文档.md](docs/Folio需求文档.md). Both are in Chinese.
