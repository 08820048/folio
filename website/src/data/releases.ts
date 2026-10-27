/**
 * The website's copy of the release history. Edit this by hand when a version
 * ships; it is the page's only source of truth.
 *
 * Keep it in step with the appcast that `scripts/release.sh` writes: the
 * `--notes` given on the command line end up in the Sparkle feed, and the
 * newest entry here should say the same thing in the same words. Version and
 * build live in `packaging/release.env`.
 */

export type ReleaseKind = "added" | "improved" | "fixed";

export type ReleaseNote = {
  kind: ReleaseKind;
  text: string;
};

export type Release = {
  /** Marketing version, e.g. "0.1.1" — matches `VERSION` in release.env. */
  version: string;
  /** Sparkle build number (`sparkle:version`), when it is known. */
  build?: number;
  /** Release date, `YYYY-MM-DD`. Rendered without the system locale so the
   *  prerendered HTML and the hydrated page always agree. */
  date: string;
  latest?: boolean;
  summary?: string;
  notes: ReleaseNote[];
};

/** Newest first. */
export const releases: Release[] = [
  {
    version: "0.1.7",
    build: 10,
    date: "2026-10-27",
    latest: true,
    summary: "Bigger files, a search that arrives while it runs, and the reading paths filled in.",
    notes: [
      {
        kind: "added",
        text: "Files over 32 MiB open in a read-only page viewer, and binary or non-UTF-8 files open as hex.",
      },
      {
        kind: "added",
        text: "Project search runs in parallel and streams hits as it scans; Stop keeps the partial list, and the path filter includes or excludes by fragment.",
      },
      {
        kind: "added",
        text: "Review marks in the changes list and diff header are saved with the session.",
      },
      {
        kind: "added",
        text: "Markdown relative images render in place, and relative links to project files open in Folio.",
      },
      {
        kind: "added",
        text: "Images zoom and pan: footer buttons, Cmd-scroll or pinch, 25–800%.",
      },
      {
        kind: "added",
        text: "The terminal has scrollback search with next and previous match.",
      },
      {
        kind: "improved",
        text: "The file tree and quick-open index follow .gitignore.",
      },
      {
        kind: "improved",
        text: "Quick open ranks names and remembers each project's recent files.",
      },
      {
        kind: "improved",
        text: "Tabs have keyboard navigation, a scrolling strip, and Cmd-Shift-T to reopen; Save All and code-size zoom join the shortcuts.",
      },
      {
        kind: "improved",
        text: "The file tree takes F2 rename, Delete, Shift-F10, Home/End and type-ahead.",
      },
      {
        kind: "improved",
        text: "Multi-cursor is complete: Option-click adds a caret, arrows move every caret, and word/line deletes apply to all of them.",
      },
      {
        kind: "improved",
        text: "Accessibility labels were filled in, and Windows builds now get a source-level CI check.",
      },
    ],
  },
  {
    version: "0.1.6",
    build: 9,
    date: "2026-09-22",
    summary: "Swift and C# light up, and the side terminal reads as its own panel.",
    notes: [
      {
        kind: "fixed",
        text: "Swift, C#, CMake and Protocol Buffers files are syntax highlighted again. The component registered their grammars with an empty query, so they painted as plain text.",
      },
      {
        kind: "fixed",
        text: "The terminal dock beside the editor carries its own bottom rule, so it no longer reads as part of the editor's tab strip.",
      },
    ],
  },
  {
    version: "0.1.5",
    build: 8,
    date: "2026-09-21",
    summary: "A terminal that docks beside the editor, and Chinese text that fits its cells.",
    notes: [
      {
        kind: "added",
        text: "The terminal can dock in a column beside the editor. The button left of the + in the terminal's tab strip swaps sides, and the column's edge drags its width.",
      },
      {
        kind: "fixed",
        text: "Chinese and other wide characters fill both of their grid cells in the terminal: no blank strip after each one, backgrounds and selections cover them fully, and IME composition no longer overlaps its own glyphs.",
      },
    ],
  },
  {
    version: "0.1.4",
    build: 7,
    date: "2026-09-18",
    summary: "A smoother caret, and quieter updates.",
    notes: [
      {
        kind: "added",
        text: "The editor caret glides to its new place. Turn it off in Settings if you want it to jump.",
      },
      {
        kind: "added",
        text: "Folio checks for updates quietly. A green badge in the title bar means a newer build is ready.",
      },
      {
        kind: "improved",
        text: "The About window no longer stretches its copy to the edges, and links to folioedit.dev.",
      },
    ],
  },
  {
    version: "0.1.3",
    build: 6,
    date: "2026-09-15",
    summary: "File-type icons in the tree, and a tighter file list.",
    notes: [
      {
        kind: "added",
        text: "Files in the tree show a matching icon for the language or format.",
      },
      {
        kind: "improved",
        text: "The file list is tighter, with indent that follows the tree.",
      },
    ],
  },
  {
    version: "0.1.2",
    build: 5,
    date: "2026-09-15",
    summary: "Run code from the editor, and a few places that used to get in the way.",
    notes: [
      {
        kind: "added",
        text: "Run the current file, test, or main from the gutter or ⌘R, in a dedicated Run tab. While it is running, Play becomes Stop.",
      },
      {
        kind: "added",
        text: "Rainbow brackets and matching, so nested pairs are easier to follow.",
      },
      {
        kind: "added",
        text: "Click a printed URL in the terminal to open it in the browser.",
      },
      {
        kind: "fixed",
        text: "Typing in the in-file search box no longer selects the whole query after every character.",
      },
    ],
  },
  {
    version: "0.1.1",
    build: 4,
    date: "2026-09-14",
    summary: "A quieter second build: fewer places where the app got in the way.",
    notes: [
      {
        kind: "improved",
        text: "Notices match the editor and slide in from the right.",
      },
      {
        kind: "improved",
        text: "The file tree and activity bar are toggled from the window, not from Settings.",
      },
      {
        kind: "fixed",
        text: "Opening a project no longer runs its Git hooks; copies no longer follow dangling links.",
      },
    ],
  },
  {
    version: "0.1.0",
    date: "2026-09-13",
    summary:
      "The first public macOS release: a quiet, native place to read and lightly edit code.",
    notes: [
      {
        kind: "added",
        text: "A native editor built with Rust and GPUI — the interface is drawn directly, with no browser engine and no WebView.",
      },
      {
        kind: "added",
        text: "A multi-project workspace: several folders open in one window, each keeping its own buffers, tabs and expansion state.",
      },
      {
        kind: "added",
        text: "Session restore: projects, tab order, the file on screen and unsaved edits all come back on the next launch.",
      },
      {
        kind: "added",
        text: "Syntax highlighting for 60+ languages through Tree-sitter, with the grammars compiled into the binary.",
      },
      {
        kind: "added",
        text: "Tabbed editing with code folding, bracket pairing, line comments and image preview.",
      },
      {
        kind: "added",
        text: "Multi-cursor editing, including column selection and select-all-occurrences.",
      },
      {
        kind: "added",
        text: "Project-wide find and replace with case, whole-word and regular-expression filters.",
      },
      {
        kind: "added",
        text: "Quick open by file name, and `:123` to jump straight to a line.",
      },
      {
        kind: "added",
        text: "Git changes list, a unified or split diff against HEAD or the main branch, and line-by-line blame.",
      },
      {
        kind: "added",
        text: "A two-pane split view, side by side or stacked.",
      },
      {
        kind: "added",
        text: "A built-in terminal per project, on the same PTY and VT stack Zed uses.",
      },
      {
        kind: "added",
        text: "A settings window, `.editorconfig` support, and light and dark themes that follow the system.",
      },
      {
        kind: "added",
        text: "A notarized DMG, and automatic updates through Sparkle.",
      },
    ],
  },
];

export const latestRelease = releases.find((release) => release.latest) ?? releases[0];

const MONTHS = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December",
];

/** Deterministic English date, so SSR and hydration never disagree. */
export function formatReleaseDate(iso: string): string {
  const [year, month, day] = iso.split("-").map(Number);
  return `${MONTHS[month - 1]} ${day}, ${year}`;
}
